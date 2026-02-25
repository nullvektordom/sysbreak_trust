// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721Enumerable.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title SysbreakAchievementNFT
 * @author Nullvektor Dominion
 * @notice ERC-721 achievement NFTs for SYSBREAK with soulbound support (ERC-5192)
 *
 * @dev Features:
 *   - Soulbound (non-transferable) tokens via ERC-5192
 *   - Role-based minting (owner + authorized minter)
 *   - Timelock on privileged operations (minter change, unpause)
 *   - Batch minting with bounded loops
 *   - ERC721Enumerable for gas-efficient on-chain enumeration
 *   - Admin burn/revoke capability
 *   - Emergency pause
 *   - Reentrancy protection on all state-changing externals
 *   - Metadata freeze per token for provenance integrity
 */
contract SysbreakAchievementNFT is
    ERC721,
    ERC721Enumerable,
    ERC721URIStorage,
    Ownable2Step,
    ReentrancyGuard,
    Pausable
{
    // ──────────────────────────────────────────────
    //  Constants
    // ──────────────────────────────────────────────

    /// @notice Maximum batch size to prevent block gas limit issues
    uint256 public constant MAX_BATCH_SIZE = 50;

    /// @notice Timelock delay for privileged operations (48 hours)
    uint256 public constant TIMELOCK_DELAY = 48 hours;

    // ──────────────────────────────────────────────
    //  State
    // ──────────────────────────────────────────────

    /// @notice Next token ID to mint (replaces removed OZ Counters)
    uint256 private _nextTokenId;

    /// @notice Authorized minter address (backend wallet)
    address public minter;

    /// @dev Soulbound flag per token
    mapping(uint256 => bool) private _soulbound;

    /// @dev Frozen metadata flag per token — once set, URI is immutable
    mapping(uint256 => bool) private _metadataFrozen;

    // ──────────────────────────────────────────────
    //  Timelock
    // ──────────────────────────────────────────────

    struct TimelockOp {
        bytes32 dataHash;   // keccak256 of the operation payload
        uint256 readyAt;    // timestamp when executable
        bool exists;
    }

    /// @notice Pending timelock operations keyed by an operation ID
    mapping(bytes32 => TimelockOp) public timelockOps;

    // ──────────────────────────────────────────────
    //  Events
    // ──────────────────────────────────────────────

    event AchievementMinted(
        address indexed to,
        uint256 indexed tokenId,
        string tokenURI,
        bool isSoulbound
    );
    event AchievementRevoked(uint256 indexed tokenId, address indexed from);
    event MinterUpdated(address indexed oldMinter, address indexed newMinter);
    event MetadataFrozen(uint256 indexed tokenId);

    // ERC-5192: Minimal Soulbound NFTs
    event Locked(uint256 indexed tokenId);
    event Unlocked(uint256 indexed tokenId);

    // Timelock events
    event TimelockScheduled(bytes32 indexed opId, uint256 readyAt, string description);
    event TimelockExecuted(bytes32 indexed opId);
    event TimelockCancelled(bytes32 indexed opId);

    // ──────────────────────────────────────────────
    //  Errors
    // ──────────────────────────────────────────────

    error NotAuthorizedToMint();
    error InvalidAddress();
    error SoulboundTransferBlocked(uint256 tokenId);
    error SoulboundApprovalBlocked(uint256 tokenId);
    error MetadataIsFrozen(uint256 tokenId);
    error ArrayLengthMismatch();
    error BatchTooLarge(uint256 provided, uint256 max);
    error TimelockNotReady(bytes32 opId, uint256 readyAt);
    error TimelockNotFound(bytes32 opId);
    error TimelockAlreadyExists(bytes32 opId);
    error TimelockDataMismatch();

    // ──────────────────────────────────────────────
    //  Modifiers
    // ──────────────────────────────────────────────

    modifier onlyMinterOrOwner() {
        if (msg.sender != minter && msg.sender != owner()) {
            revert NotAuthorizedToMint();
        }
        _;
    }

    // ──────────────────────────────────────────────
    //  Constructor
    // ──────────────────────────────────────────────

    constructor() ERC721("SYSBREAK Achievement", "SYSA") Ownable(msg.sender) {
        minter = msg.sender;
    }

    // ══════════════════════════════════════════════
    //  TIMELOCK INTERNALS
    // ══════════════════════════════════════════════

    /**
     * @dev Schedule a timelocked operation.
     * @param opId    Unique identifier for this operation
     * @param data    Arbitrary payload whose hash is stored (verified on execute)
     * @param desc    Human-readable label emitted in the event
     */
    function _scheduleTimelock(bytes32 opId, bytes memory data, string memory desc) internal {
        if (timelockOps[opId].exists) revert TimelockAlreadyExists(opId);

        timelockOps[opId] = TimelockOp({
            dataHash: keccak256(data),
            readyAt: block.timestamp + TIMELOCK_DELAY,
            exists: true
        });

        emit TimelockScheduled(opId, block.timestamp + TIMELOCK_DELAY, desc);
    }

    /**
     * @dev Consume a matured timelock. Reverts if not ready or data doesn't match.
     */
    function _executeTimelock(bytes32 opId, bytes memory data) internal {
        TimelockOp storage op = timelockOps[opId];
        if (!op.exists) revert TimelockNotFound(opId);
        if (block.timestamp < op.readyAt) revert TimelockNotReady(opId, op.readyAt);
        if (keccak256(data) != op.dataHash) revert TimelockDataMismatch();

        delete timelockOps[opId];
        emit TimelockExecuted(opId);
    }

    /**
     * @notice Cancel a pending timelocked operation.
     * @param opId The operation ID to cancel
     */
    function cancelTimelock(bytes32 opId) external onlyOwner {
        if (!timelockOps[opId].exists) revert TimelockNotFound(opId);
        delete timelockOps[opId];
        emit TimelockCancelled(opId);
    }

    // ══════════════════════════════════════════════
    //  TIMELOCKED ADMIN FUNCTIONS
    // ══════════════════════════════════════════════

    /**
     * @notice Schedule a minter change (48h delay).
     * @param newMinter The proposed new minter address
     */
    function scheduleMinterChange(address newMinter) external onlyOwner {
        if (newMinter == address(0)) revert InvalidAddress();

        bytes32 opId = keccak256(abi.encodePacked("setMinter", newMinter));
        bytes memory data = abi.encode(newMinter);

        _scheduleTimelock(opId, data, "Minter change");
    }

    /**
     * @notice Execute a matured minter change.
     * @param newMinter Must match the address used in scheduling
     */
    function executeMinterChange(address newMinter) external onlyOwner {
        bytes32 opId = keccak256(abi.encodePacked("setMinter", newMinter));
        bytes memory data = abi.encode(newMinter);

        _executeTimelock(opId, data);

        address oldMinter = minter;
        minter = newMinter;
        emit MinterUpdated(oldMinter, newMinter);
    }

    /**
     * @notice Schedule an unpause (48h delay). Pausing is instant for emergencies.
     */
    function scheduleUnpause() external onlyOwner {
        bytes32 opId = keccak256("unpause");
        _scheduleTimelock(opId, "", "Unpause");
    }

    /**
     * @notice Execute a matured unpause.
     */
    function executeUnpause() external onlyOwner {
        bytes32 opId = keccak256("unpause");
        _executeTimelock(opId, "");
        _unpause();
    }

    /**
     * @notice Emergency pause — instant, no timelock.
     */
    function pause() external onlyOwner {
        _pause();
    }

    // ══════════════════════════════════════════════
    //  MINTING
    // ══════════════════════════════════════════════

    /**
     * @notice Mint a single achievement NFT.
     * @param to           Recipient address
     * @param uri          IPFS metadata URI
     * @param isSoulbound  Whether the token is non-transferable
     * @return tokenId     The newly minted token ID
     */
    function mintAchievement(
        address to,
        string calldata uri,
        bool isSoulbound
    )
        public
        onlyMinterOrOwner
        nonReentrant
        whenNotPaused
        returns (uint256)
    {
        return _mintAchievementInternal(to, uri, isSoulbound);
    }

    /**
     * @notice Batch mint achievements (max 50 per call).
     * @param recipients     Array of recipient addresses
     * @param uris           Array of IPFS metadata URIs
     * @param soulboundFlags Array of soulbound flags
     */
    function batchMintAchievements(
        address[] calldata recipients,
        string[] calldata uris,
        bool[] calldata soulboundFlags
    )
        external
        onlyMinterOrOwner
        nonReentrant
        whenNotPaused
    {
        uint256 len = recipients.length;
        if (len != uris.length || len != soulboundFlags.length) {
            revert ArrayLengthMismatch();
        }
        if (len > MAX_BATCH_SIZE) {
            revert BatchTooLarge(len, MAX_BATCH_SIZE);
        }

        for (uint256 i = 0; i < len; ) {
            _mintAchievementInternal(recipients[i], uris[i], soulboundFlags[i]);
            unchecked { ++i; }
        }
    }

    /**
     * @dev Shared internal mint logic — no auth check (callers handle that).
     *      Uses _mint instead of _safeMint to eliminate reentrancy from
     *      onERC721Received callbacks. Safe because recipients are game players
     *      controlled by the backend minter.
     */
    function _mintAchievementInternal(
        address to,
        string calldata uri,
        bool isSoulbound
    ) private returns (uint256) {
        if (to == address(0)) revert InvalidAddress();

        uint256 tokenId = _nextTokenId++;

        _mint(to, tokenId);
        _setTokenURI(tokenId, uri);

        if (isSoulbound) {
            _soulbound[tokenId] = true;
            emit Locked(tokenId);
        }

        emit AchievementMinted(to, tokenId, uri, isSoulbound);
        return tokenId;
    }

    // ══════════════════════════════════════════════
    //  ADMIN — BURN / REVOKE
    // ══════════════════════════════════════════════

    /**
     * @notice Revoke/burn an achievement. Owner-only for dispute resolution.
     * @param tokenId The token to burn
     */
    function revokeAchievement(uint256 tokenId) external onlyOwner nonReentrant {
        address tokenOwner = ownerOf(tokenId); // reverts if nonexistent
        _burn(tokenId);
        // Clean up soulbound & freeze state
        delete _soulbound[tokenId];
        delete _metadataFrozen[tokenId];
        emit AchievementRevoked(tokenId, tokenOwner);
    }

    // ══════════════════════════════════════════════
    //  METADATA FREEZE
    // ══════════════════════════════════════════════

    /**
     * @notice Permanently freeze a token's metadata URI. Irreversible.
     * @param tokenId The token whose metadata to freeze
     */
    function freezeMetadata(uint256 tokenId) external onlyOwner {
        ownerOf(tokenId); // existence check
        _metadataFrozen[tokenId] = true;
        emit MetadataFrozen(tokenId);
    }

    /**
     * @notice Check if a token's metadata is frozen.
     */
    function isMetadataFrozen(uint256 tokenId) external view returns (bool) {
        ownerOf(tokenId); // existence check
        return _metadataFrozen[tokenId];
    }

    // ══════════════════════════════════════════════
    //  VIEWS
    // ══════════════════════════════════════════════

    /**
     * @notice Check if a token is soulbound (ERC-5192 compatible).
     * @param tokenId The token to check
     * @return True if the token is locked/soulbound
     */
    function locked(uint256 tokenId) external view returns (bool) {
        ownerOf(tokenId); // existence check — reverts if nonexistent
        return _soulbound[tokenId];
    }

    /**
     * @notice Get all achievement token IDs for an address.
     * @dev Uses ERC721Enumerable — O(balance) not O(totalSupply).
     */
    function getAchievements(address owner) external view returns (uint256[] memory) {
        uint256 balance = balanceOf(owner);
        uint256[] memory tokens = new uint256[](balance);

        for (uint256 i = 0; i < balance; ) {
            tokens[i] = tokenOfOwnerByIndex(owner, i);
            unchecked { ++i; }
        }

        return tokens;
    }

    // ══════════════════════════════════════════════
    //  SOULBOUND ENFORCEMENT
    // ══════════════════════════════════════════════

    /**
     * @dev Override to block transfers of soulbound tokens.
     *      Minting (from == 0) and burning (to == 0) are always allowed.
     */
    function _update(
        address to,
        uint256 tokenId,
        address auth
    ) internal virtual override(ERC721, ERC721Enumerable) returns (address) {
        address from = _ownerOf(tokenId);

        // Block transfer (not mint/burn) if soulbound
        if (from != address(0) && to != address(0) && _soulbound[tokenId]) {
            revert SoulboundTransferBlocked(tokenId);
        }

        return super._update(to, tokenId, auth);
    }

    /**
     * @dev Block approvals on soulbound tokens — prevents misleading marketplace listings.
     */
    function approve(address to, uint256 tokenId) public virtual override(ERC721, IERC721) {
        if (_soulbound[tokenId]) revert SoulboundApprovalBlocked(tokenId);
        super.approve(to, tokenId);
    }

    // ══════════════════════════════════════════════
    //  REQUIRED OVERRIDES
    // ══════════════════════════════════════════════

    function _increaseBalance(
        address account,
        uint128 value
    ) internal virtual override(ERC721, ERC721Enumerable) {
        super._increaseBalance(account, value);
    }

    function tokenURI(
        uint256 tokenId
    ) public view override(ERC721, ERC721URIStorage) returns (string memory) {
        return super.tokenURI(tokenId);
    }

    function supportsInterface(
        bytes4 interfaceId
    ) public view override(ERC721, ERC721Enumerable, ERC721URIStorage) returns (bool) {
        // ERC-5192 interface ID: 0xb45a3c0e
        return interfaceId == 0xb45a3c0e || super.supportsInterface(interfaceId);
    }
}
