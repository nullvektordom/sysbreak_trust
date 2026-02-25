// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/token/common/ERC2981.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title SysbreakItemNFT
 * @dev ERC721 NFT contract for SYSBREAK in-game items
 * @author Nex Qiros / Nullvektor Dominion
 *
 * Features:
 * - Mintable by authorized minter (backend) or owner
 * - Items can be transferred/traded freely
 * - Metadata stored on IPFS
 * - EIP-2981 royalty support for marketplace trades
 * - Timelock on sensitive admin operations
 * - Pausable for emergency circuit-breaking
 * - Ownable2Step for safer ownership transfers
 *
 * Security:
 * - Pinned compiler version (0.8.24)
 * - ReentrancyGuard on mint functions
 * - Timelock delay on minter, marketplace, and royalty changes
 * - No deprecated dependencies (Counters removed)
 * - Ownable2Step prevents accidental ownership loss
 */
contract SysbreakItemNFT is
    ERC721,
    ERC721URIStorage,
    ERC2981,
    Ownable2Step,
    ReentrancyGuard,
    Pausable
{
    // =========================================================================
    //                              STATE
    // =========================================================================

    /// @dev Next token ID to mint (replaces deprecated Counters)
    uint256 private _nextTokenId;

    /// @dev Authorized minter (backend wallet)
    address public minter;

    /// @dev Marketplace contract address (for future integration)
    address public marketplace;

    /// @dev Timelock delay for admin operations (default 24 hours)
    uint256 public constant TIMELOCK_DELAY = 24 hours;

    /// @dev Maximum batch mint size
    uint256 public constant MAX_BATCH_SIZE = 50;

    /// @dev Maximum royalty in basis points (10%)
    uint256 public constant MAX_ROYALTY_BPS = 1000;

    // =========================================================================
    //                          TIMELOCK STORAGE
    // =========================================================================

    struct TimelockRequest {
        address newAddress;
        uint256 newValue;
        uint256 executeAfter;
        bool exists;
    }

    /// @dev Pending minter change
    TimelockRequest public pendingMinter;

    /// @dev Pending marketplace change
    TimelockRequest public pendingMarketplace;

    /// @dev Pending royalty change
    TimelockRequest public pendingRoyalty;

    // =========================================================================
    //                              EVENTS
    // =========================================================================

    event ItemMinted(address indexed to, uint256 indexed tokenId, string tokenURI);
    event BatchMinted(address indexed minter, uint256 count, uint256 firstTokenId);

    // Timelock events
    event MinterChangeRequested(address indexed newMinter, uint256 executeAfter);
    event MinterChangeExecuted(address indexed oldMinter, address indexed newMinter);
    event MinterChangeCancelled(address indexed cancelledMinter);

    event MarketplaceChangeRequested(address indexed newMarketplace, uint256 executeAfter);
    event MarketplaceChangeExecuted(address indexed oldMarketplace, address indexed newMarketplace);
    event MarketplaceChangeCancelled(address indexed cancelledMarketplace);

    event RoyaltyChangeRequested(uint256 newBps, uint256 executeAfter);
    event RoyaltyChangeExecuted(uint256 oldBps, uint256 newBps);
    event RoyaltyChangeCancelled(uint256 cancelledBps);

    // =========================================================================
    //                              ERRORS
    // =========================================================================

    error NotAuthorizedToMint();
    error InvalidAddress();
    error ArrayLengthMismatch();
    error BatchTooLarge();
    error RoyaltyTooHigh();
    error NoTimelockPending();
    error TimelockNotReady();
    error TimelockAlreadyPending();

    // =========================================================================
    //                           CONSTRUCTOR
    // =========================================================================

    constructor() ERC721("SYSBREAK Item", "SYSI") Ownable(msg.sender) {
        minter = msg.sender;
        // Set default royalty: 5% to contract deployer
        _setDefaultRoyalty(msg.sender, 500);
    }

    // =========================================================================
    //                            MODIFIERS
    // =========================================================================

    modifier onlyMinterOrOwner() {
        if (msg.sender != minter && msg.sender != owner()) {
            revert NotAuthorizedToMint();
        }
        _;
    }

    // =========================================================================
    //                          MINT FUNCTIONS
    // =========================================================================

    /**
     * @dev Mint a new item NFT
     * @param to The address to mint the NFT to
     * @param uri The metadata URI (IPFS)
     * @return tokenId The token ID of the minted NFT
     */
    function mintItem(
        address to,
        string calldata uri
    ) external onlyMinterOrOwner nonReentrant whenNotPaused returns (uint256 tokenId) {
        tokenId = _mintItemInternal(to, uri);
    }

    /**
     * @dev Batch mint items — uses internal function to avoid external self-call
     * @param recipients Array of recipient addresses
     * @param tokenURIs Array of metadata URIs
     */
    function batchMintItems(
        address[] calldata recipients,
        string[] calldata tokenURIs
    ) external onlyMinterOrOwner nonReentrant whenNotPaused {
        uint256 len = recipients.length;
        if (len != tokenURIs.length) revert ArrayLengthMismatch();
        if (len > MAX_BATCH_SIZE) revert BatchTooLarge();

        uint256 firstTokenId = _nextTokenId;

        for (uint256 i = 0; i < len; ) {
            _mintItemInternal(recipients[i], tokenURIs[i]);
            unchecked { ++i; }
        }

        emit BatchMinted(msg.sender, len, firstTokenId);
    }

    /**
     * @dev Internal mint logic — shared by mintItem and batchMintItems
     */
    function _mintItemInternal(
        address to,
        string calldata uri
    ) internal returns (uint256 tokenId) {
        if (to == address(0)) revert InvalidAddress();

        tokenId = _nextTokenId++;
        _safeMint(to, tokenId);
        _setTokenURI(tokenId, uri);

        emit ItemMinted(to, tokenId, uri);
    }

    // =========================================================================
    //                    TIMELOCKED ADMIN FUNCTIONS
    // =========================================================================

    // --- Minter ---

    /**
     * @dev Request a minter change (starts timelock)
     */
    function requestMinterChange(address _minter) external onlyOwner {
        if (_minter == address(0)) revert InvalidAddress();
        if (pendingMinter.exists) revert TimelockAlreadyPending();

        uint256 executeAfter = block.timestamp + TIMELOCK_DELAY;
        pendingMinter = TimelockRequest({
            newAddress: _minter,
            newValue: 0,
            executeAfter: executeAfter,
            exists: true
        });

        emit MinterChangeRequested(_minter, executeAfter);
    }

    /**
     * @dev Execute a pending minter change after timelock expires
     */
    function executeMinterChange() external onlyOwner {
        if (!pendingMinter.exists) revert NoTimelockPending();
        if (block.timestamp < pendingMinter.executeAfter) revert TimelockNotReady();

        address oldMinter = minter;
        minter = pendingMinter.newAddress;
        delete pendingMinter;

        emit MinterChangeExecuted(oldMinter, minter);
    }

    /**
     * @dev Cancel a pending minter change
     */
    function cancelMinterChange() external onlyOwner {
        if (!pendingMinter.exists) revert NoTimelockPending();

        address cancelled = pendingMinter.newAddress;
        delete pendingMinter;

        emit MinterChangeCancelled(cancelled);
    }

    // --- Marketplace ---

    /**
     * @dev Request a marketplace change (starts timelock)
     */
    function requestMarketplaceChange(address _marketplace) external onlyOwner {
        if (_marketplace == address(0)) revert InvalidAddress();
        if (pendingMarketplace.exists) revert TimelockAlreadyPending();

        uint256 executeAfter = block.timestamp + TIMELOCK_DELAY;
        pendingMarketplace = TimelockRequest({
            newAddress: _marketplace,
            newValue: 0,
            executeAfter: executeAfter,
            exists: true
        });

        emit MarketplaceChangeRequested(_marketplace, executeAfter);
    }

    /**
     * @dev Execute a pending marketplace change after timelock expires
     */
    function executeMarketplaceChange() external onlyOwner {
        if (!pendingMarketplace.exists) revert NoTimelockPending();
        if (block.timestamp < pendingMarketplace.executeAfter) revert TimelockNotReady();

        address oldMarketplace = marketplace;
        marketplace = pendingMarketplace.newAddress;
        delete pendingMarketplace;

        emit MarketplaceChangeExecuted(oldMarketplace, marketplace);
    }

    /**
     * @dev Cancel a pending marketplace change
     */
    function cancelMarketplaceChange() external onlyOwner {
        if (!pendingMarketplace.exists) revert NoTimelockPending();

        address cancelled = pendingMarketplace.newAddress;
        delete pendingMarketplace;

        emit MarketplaceChangeCancelled(cancelled);
    }

    // --- Royalty ---

    /**
     * @dev Request a royalty change (starts timelock)
     */
    function requestRoyaltyChange(uint256 _royaltyBps) external onlyOwner {
        if (_royaltyBps > MAX_ROYALTY_BPS) revert RoyaltyTooHigh();
        if (pendingRoyalty.exists) revert TimelockAlreadyPending();

        uint256 executeAfter = block.timestamp + TIMELOCK_DELAY;
        pendingRoyalty = TimelockRequest({
            newAddress: address(0),
            newValue: _royaltyBps,
            executeAfter: executeAfter,
            exists: true
        });

        emit RoyaltyChangeRequested(_royaltyBps, executeAfter);
    }

    /**
     * @dev Execute a pending royalty change after timelock expires
     */
    function executeRoyaltyChange() external onlyOwner {
        if (!pendingRoyalty.exists) revert NoTimelockPending();
        if (block.timestamp < pendingRoyalty.executeAfter) revert TimelockNotReady();

        uint256 newBps = pendingRoyalty.newValue;
        (, uint256 oldBps) = royaltyInfo(0, 10000); // Get current bps
        delete pendingRoyalty;
        _setDefaultRoyalty(owner(), uint96(newBps));

        emit RoyaltyChangeExecuted(oldBps, newBps);
    }

    /**
     * @dev Cancel a pending royalty change
     */
    function cancelRoyaltyChange() external onlyOwner {
        if (!pendingRoyalty.exists) revert NoTimelockPending();

        uint256 cancelled = pendingRoyalty.newValue;
        delete pendingRoyalty;

        emit RoyaltyChangeCancelled(cancelled);
    }

    // =========================================================================
    //                        EMERGENCY FUNCTIONS
    // =========================================================================

    /**
     * @dev Pause minting (emergency circuit breaker)
     */
    function pause() external onlyOwner {
        _pause();
    }

    /**
     * @dev Unpause minting
     */
    function unpause() external onlyOwner {
        _unpause();
    }

    // =========================================================================
    //                          VIEW FUNCTIONS
    // =========================================================================

    /**
     * @dev Get total supply of minted tokens
     */
    function totalSupply() external view returns (uint256) {
        return _nextTokenId;
    }

    /**
     * @dev Check if a timelock is ready to execute
     */
    function isTimelockReady(
        string calldata timelockType
    ) external view returns (bool ready, uint256 executeAfter) {
        bytes32 t = keccak256(bytes(timelockType));

        TimelockRequest storage req;
        if (t == keccak256("minter")) req = pendingMinter;
        else if (t == keccak256("marketplace")) req = pendingMarketplace;
        else if (t == keccak256("royalty")) req = pendingRoyalty;
        else return (false, 0);

        if (!req.exists) return (false, 0);
        return (block.timestamp >= req.executeAfter, req.executeAfter);
    }

    // =========================================================================
    //                     REQUIRED OVERRIDES
    // =========================================================================

    function tokenURI(
        uint256 tokenId
    ) public view override(ERC721, ERC721URIStorage) returns (string memory) {
        return super.tokenURI(tokenId);
    }

    function supportsInterface(
        bytes4 interfaceId
    ) public view override(ERC721, ERC721URIStorage, ERC2981) returns (bool) {
        return super.supportsInterface(interfaceId);
    }
}
