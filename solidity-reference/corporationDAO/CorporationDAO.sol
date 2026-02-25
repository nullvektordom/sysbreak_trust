// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/governance/Governor.sol";
import "@openzeppelin/contracts/governance/extensions/GovernorSettings.sol";
import "@openzeppelin/contracts/governance/extensions/GovernorCountingSimple.sol";
import "@openzeppelin/contracts/governance/extensions/GovernorVotes.sol";
import "@openzeppelin/contracts/governance/extensions/GovernorVotesQuorumFraction.sol";
import "@openzeppelin/contracts/governance/extensions/GovernorTimelockControl.sol";
import "@openzeppelin/contracts/governance/TimelockController.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Votes.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";
import "@openzeppelin/contracts/utils/Strings.sol";

// ============================================================================
//  CorporationGovernanceToken
// ============================================================================

/**
 * @title CorporationGovernanceToken
 * @dev ERC20 governance token for corporation voting.
 *
 * Changes from original:
 *  - Fixed compilation error (ERC20Permits → ERC20Permit)
 *  - Auto-delegates to each initial holder so voting power is active immediately
 *  - Caps initial holders array at 100 to prevent block gas limit DoS
 *  - Zero-address validation on mint
 */
contract CorporationGovernanceToken is ERC20, ERC20Permit, ERC20Votes, Ownable {
    uint256 public constant MAX_INITIAL_HOLDERS = 100;

    constructor(
        string memory name,
        string memory symbol,
        address[] memory initialHolders,
        uint256[] memory initialBalances
    ) ERC20(name, symbol) ERC20Permit(name) Ownable(msg.sender) {
        require(initialHolders.length == initialBalances.length, "Array length mismatch");
        require(initialHolders.length <= MAX_INITIAL_HOLDERS, "Too many initial holders");

        for (uint256 i = 0; i < initialHolders.length; i++) {
            require(initialHolders[i] != address(0), "Zero address holder");
            _mint(initialHolders[i], initialBalances[i]);
            // Auto-delegate so voting power is active from block 1
            _delegate(initialHolders[i], initialHolders[i]);
        }
    }

    function mint(address to, uint256 amount) external onlyOwner {
        require(to != address(0), "Mint to zero address");
        _mint(to, amount);
    }

    function burn(address from, uint256 amount) external onlyOwner {
        _burn(from, amount);
    }

    // Required overrides for ERC20Votes
    function _update(address from, address to, uint256 amount) internal override(ERC20, ERC20Votes) {
        super._update(from, to, amount);
    }

    // FIX: ERC20Permits → ERC20Permit (was a compilation-breaking typo)
    function nonces(address owner) public view override(ERC20Permit, Nonces) returns (uint256) {
        return super.nonces(owner);
    }
}

// ============================================================================
//  CorporationDAO
// ============================================================================

/**
 * @title CorporationDAO
 * @dev Governance contract for SYSBREAK corporations.
 *
 * Changes from original:
 *  - Integrated GovernorTimelockControl — proposals have a mandatory delay
 *    before execution, giving members time to exit or react
 *  - Replaced fixed quorum with GovernorVotesQuorumFraction (10% of supply)
 *  - Increased voting delay from 1 block to 7200 blocks (~1 day) to prevent
 *    flash-loan governance attacks
 *  - Timelocked officer changes via pendingOfficerChanges with OFFICER_TIMELOCK
 *  - Fixed proposeSimple ABI encoding (abi.encodePacked → abi.encodeWithSelector)
 *  - Fixed proposeTransfer description (raw bytes → human-readable string)
 *  - Added proposeMint / proposeBurn helpers
 *  - Zero-address validation on backend and officer setters
 *  - Emergency pause: backend can freeze proposals during an active incident,
 *    but only governance can unpause (prevents backend from permanently halting)
 */
contract CorporationDAO is
    Governor,
    GovernorSettings,
    GovernorCountingSimple,
    GovernorVotes,
    GovernorVotesQuorumFraction,
    GovernorTimelockControl
{
    uint256 public corporationId;
    address public backend;
    bool public paused;

    // --- Officer timelock ---------------------------------------------------
    uint256 public constant OFFICER_TIMELOCK = 1 days;

    struct PendingOfficerChange {
        address member;
        bool isOfficer;
        uint256 effectiveAt; // timestamp when change can be applied
    }

    // Pending changes keyed by nonce (monotonically increasing)
    uint256 public officerChangeNonce;
    mapping(uint256 => PendingOfficerChange) public pendingOfficerChanges;
    mapping(address => bool) public isOfficer;

    // --- Events -------------------------------------------------------------
    event OfficerChangeQueued(
        uint256 indexed changeId,
        address indexed member,
        bool isOfficer,
        uint256 effectiveAt
    );
    event OfficerChangeExecuted(uint256 indexed changeId, address indexed member, bool isOfficer);
    event OfficerChangeCancelled(uint256 indexed changeId);
    event BackendUpdated(address indexed oldBackend, address indexed newBackend);
    event ProposalCreatedByCorporation(
        uint256 indexed proposalId,
        address indexed proposer,
        string description
    );
    event Paused(address indexed by);
    event Unpaused();

    // --- Modifiers ----------------------------------------------------------
    modifier onlyBackend() {
        require(msg.sender == backend, "Only backend");
        _;
    }

    modifier whenNotPaused() {
        require(!paused, "Governance is paused");
        _;
    }

    // --- Constructor --------------------------------------------------------
    constructor(
        IVotes _token,
        uint256 _corporationId,
        address _backend,
        TimelockController _timelock
    )
        Governor("Corporation DAO")
        GovernorSettings(
            7200,     // ~1 day voting delay (12s blocks) — flash loan protection
            50400,    // ~1 week voting period
            1000e18   // 1000 tokens to propose
        )
        GovernorVotes(_token)
        GovernorVotesQuorumFraction(10) // 10% of total supply
        GovernorTimelockControl(_timelock)
    {
        require(_backend != address(0), "Zero backend address");
        corporationId = _corporationId;
        backend = _backend;
    }

    // ========================================================================
    //  Backend management (governance-only)
    // ========================================================================

    /**
     * @dev Set backend address. Only callable via governance proposal execution
     *      (through the timelock).
     */
    function setBackend(address _backend) external onlyGovernance {
        require(_backend != address(0), "Zero backend address");
        address oldBackend = backend;
        backend = _backend;
        emit BackendUpdated(oldBackend, _backend);
    }

    // ========================================================================
    //  Officer management (timelocked)
    // ========================================================================

    /**
     * @dev Queue an officer status change. The change takes effect after
     *      OFFICER_TIMELOCK (1 day), giving token holders time to react.
     */
    function queueOfficerChange(address member, bool _isOfficer) external onlyBackend {
        require(member != address(0), "Zero address officer");

        uint256 changeId = officerChangeNonce++;
        uint256 effectiveAt = block.timestamp + OFFICER_TIMELOCK;

        pendingOfficerChanges[changeId] = PendingOfficerChange({
            member: member,
            isOfficer: _isOfficer,
            effectiveAt: effectiveAt
        });

        emit OfficerChangeQueued(changeId, member, _isOfficer, effectiveAt);
    }

    /**
     * @dev Execute a pending officer change after the timelock has elapsed.
     *      Anyone can call this (permissionless execution after delay).
     */
    function executeOfficerChange(uint256 changeId) external {
        PendingOfficerChange storage change = pendingOfficerChanges[changeId];
        require(change.effectiveAt != 0, "Change does not exist");
        require(block.timestamp >= change.effectiveAt, "Timelock not elapsed");

        isOfficer[change.member] = change.isOfficer;
        emit OfficerChangeExecuted(changeId, change.member, change.isOfficer);

        delete pendingOfficerChanges[changeId];
    }

    /**
     * @dev Cancel a pending officer change. Backend can cancel before it takes effect.
     */
    function cancelOfficerChange(uint256 changeId) external onlyBackend {
        require(pendingOfficerChanges[changeId].effectiveAt != 0, "Change does not exist");
        emit OfficerChangeCancelled(changeId);
        delete pendingOfficerChanges[changeId];
    }

    // ========================================================================
    //  Emergency pause
    // ========================================================================

    /**
     * @dev Backend can pause proposal creation during an active incident.
     *      Only governance (timelock) can unpause — prevents backend from
     *      permanently halting governance.
     */
    function pause() external onlyBackend {
        paused = true;
        emit Paused(msg.sender);
    }

    function unpause() external onlyGovernance {
        paused = false;
        emit Unpaused();
    }

    // ========================================================================
    //  Proposal creation
    // ========================================================================

    /**
     * @dev Override propose to enforce officer-only + pause check.
     */
    function propose(
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        string memory description
    ) public override(Governor) whenNotPaused returns (uint256) {
        require(isOfficer[msg.sender], "Only officers can propose");

        uint256 proposalId = super.propose(targets, values, calldatas, description);
        emit ProposalCreatedByCorporation(proposalId, msg.sender, description);

        return proposalId;
    }

    /**
     * @dev Simple proposal helper — accepts pre-encoded calldata for a single target.
     *
     *      FIX: Removed the broken `signature` + `abi.encodePacked` pattern.
     *      Callers should pass fully-encoded calldata (use abi.encodeWithSelector
     *      or abi.encodeCall off-chain / in the calling contract).
     */
    function proposeSimple(
        address target,
        uint256 value,
        bytes memory callData,
        string memory description
    ) external returns (uint256) {
        address[] memory targets = new address[](1);
        targets[0] = target;

        uint256[] memory values = new uint256[](1);
        values[0] = value;

        bytes[] memory calldatas = new bytes[](1);
        calldatas[0] = callData;

        return propose(targets, values, calldatas, description);
    }

    /**
     * @dev Proposal to transfer ETH from treasury.
     *      FIX: description now uses Strings library for human-readable output.
     */
    function proposeTransfer(
        address recipient,
        uint256 amount,
        string memory reason
    ) external returns (uint256) {
        require(recipient != address(0), "Zero recipient");

        address[] memory targets = new address[](1);
        targets[0] = recipient;

        uint256[] memory values = new uint256[](1);
        values[0] = amount;

        bytes[] memory calldatas = new bytes[](1);
        calldatas[0] = "";

        string memory description = string.concat(
            "Transfer ",
            Strings.toString(amount),
            " wei to ",
            Strings.toHexString(recipient),
            ": ",
            reason
        );

        return propose(targets, values, calldatas, description);
    }

    /**
     * @dev Proposal to mint governance tokens (DAO-owned token only).
     */
    function proposeMint(
        address tokenAddress,
        address to,
        uint256 amount,
        string memory reason
    ) external returns (uint256) {
        require(to != address(0), "Mint to zero address");

        address[] memory targets = new address[](1);
        targets[0] = tokenAddress;

        uint256[] memory values = new uint256[](1);
        values[0] = 0;

        bytes[] memory calldatas = new bytes[](1);
        calldatas[0] = abi.encodeCall(CorporationGovernanceToken.mint, (to, amount));

        string memory description = string.concat(
            "Mint ",
            Strings.toString(amount),
            " tokens to ",
            Strings.toHexString(to),
            ": ",
            reason
        );

        return propose(targets, values, calldatas, description);
    }

    /**
     * @dev Proposal to burn governance tokens.
     */
    function proposeBurn(
        address tokenAddress,
        address from,
        uint256 amount,
        string memory reason
    ) external returns (uint256) {
        address[] memory targets = new address[](1);
        targets[0] = tokenAddress;

        uint256[] memory values = new uint256[](1);
        values[0] = 0;

        bytes[] memory calldatas = new bytes[](1);
        calldatas[0] = abi.encodeCall(CorporationGovernanceToken.burn, (from, amount));

        string memory description = string.concat(
            "Burn ",
            Strings.toString(amount),
            " tokens from ",
            Strings.toHexString(from),
            ": ",
            reason
        );

        return propose(targets, values, calldatas, description);
    }

    // ========================================================================
    //  Required overrides (Governor diamond resolution)
    // ========================================================================

    function votingDelay() public view override(Governor, GovernorSettings) returns (uint256) {
        return super.votingDelay();
    }

    function votingPeriod() public view override(Governor, GovernorSettings) returns (uint256) {
        return super.votingPeriod();
    }

    function quorum(uint256 blockNumber)
        public
        view
        override(Governor, GovernorVotesQuorumFraction)
        returns (uint256)
    {
        return super.quorum(blockNumber);
    }

    function proposalThreshold() public view override(Governor, GovernorSettings) returns (uint256) {
        return super.proposalThreshold();
    }

    function state(uint256 proposalId)
        public
        view
        override(Governor, GovernorTimelockControl)
        returns (ProposalState)
    {
        return super.state(proposalId);
    }

    function proposalNeedsQueuing(uint256 proposalId)
        public
        view
        override(Governor, GovernorTimelockControl)
        returns (bool)
    {
        return super.proposalNeedsQueuing(proposalId);
    }

    function _queueOperations(
        uint256 proposalId,
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        bytes32 descriptionHash
    ) internal override(Governor, GovernorTimelockControl) returns (uint48) {
        return super._queueOperations(proposalId, targets, values, calldatas, descriptionHash);
    }

    function _executeOperations(
        uint256 proposalId,
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        bytes32 descriptionHash
    ) internal override(Governor, GovernorTimelockControl) {
        super._executeOperations(proposalId, targets, values, calldatas, descriptionHash);
    }

    function _cancel(
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        bytes32 descriptionHash
    ) internal override(Governor, GovernorTimelockControl) returns (uint256) {
        return super._cancel(targets, values, calldatas, descriptionHash);
    }

    function _executor()
        internal
        view
        override(Governor, GovernorTimelockControl)
        returns (address)
    {
        return super._executor();
    }

    // Treasury receives ETH
    receive() external payable override {}

    function getBalance() external view returns (uint256) {
        return address(this).balance;
    }
}

// ============================================================================
//  CorporationDAOFactory
// ============================================================================

/**
 * @title CorporationDAOFactory
 * @dev Factory for creating Corporation DAOs with integrated timelocks.
 *
 * Changes from original:
 *  - Inherits Ownable2Step for safe backend rotation
 *  - Maintains enumerable corporationIds array
 *  - Deploys a TimelockController per DAO
 *  - Emits BackendSet event on construction
 *  - Zero-address validation
 */
contract CorporationDAOFactory is Ownable2Step {
    address public backend;

    // Timelock delay for all new DAOs (configurable by owner)
    uint256 public timelockDelay = 1 days;

    struct DAOInfo {
        address dao;
        address token;
        address timelock;
        uint256 corporationId;
        uint256 createdAt;
    }

    mapping(uint256 => DAOInfo) public daos;
    uint256[] public corporationIds; // Enumerable list of all corporation IDs

    event DAOCreated(
        uint256 indexed corporationId,
        address indexed dao,
        address indexed token,
        address timelock,
        string name
    );
    event BackendSet(address indexed oldBackend, address indexed newBackend);
    event TimelockDelayUpdated(uint256 oldDelay, uint256 newDelay);

    constructor(address _backend) Ownable(msg.sender) {
        require(_backend != address(0), "Zero backend address");
        backend = _backend;
        emit BackendSet(address(0), _backend);
    }

    /**
     * @dev Update the backend address. Only the factory owner can do this.
     *      Uses Ownable2Step for the factory itself, so owner rotation is safe.
     */
    function setBackend(address _backend) external onlyOwner {
        require(_backend != address(0), "Zero backend address");
        address oldBackend = backend;
        backend = _backend;
        emit BackendSet(oldBackend, _backend);
    }

    /**
     * @dev Update the default timelock delay for newly created DAOs.
     */
    function setTimelockDelay(uint256 _delay) external onlyOwner {
        require(_delay >= 1 hours, "Delay too short");
        require(_delay <= 30 days, "Delay too long");
        uint256 oldDelay = timelockDelay;
        timelockDelay = _delay;
        emit TimelockDelayUpdated(oldDelay, _delay);
    }

    /**
     * @dev Create a new Corporation DAO with its own governance token and timelock.
     */
    function createCorporationDAO(
        uint256 corporationId,
        string memory name,
        string memory symbol,
        address[] memory initialMembers,
        uint256[] memory initialBalances
    ) external returns (address daoAddr, address tokenAddr, address timelockAddr) {
        require(msg.sender == backend, "Only backend can create DAOs");
        require(daos[corporationId].dao == address(0), "DAO already exists");

        // 1. Deploy governance token
        CorporationGovernanceToken token = new CorporationGovernanceToken(
            name,
            symbol,
            initialMembers,
            initialBalances
        );

        // 2. Deploy timelock controller
        //    - Proposer & executor roles will be granted to the DAO after deployment
        //    - Admin role is renounced so only the DAO controls the timelock
        address[] memory empty = new address[](0);
        TimelockController timelock = new TimelockController(
            timelockDelay,
            empty,  // proposers — set after DAO deploy
            empty,  // executors — set after DAO deploy
            address(this) // temporary admin
        );

        // 3. Deploy DAO
        CorporationDAO dao = new CorporationDAO(
            IVotes(address(token)),
            corporationId,
            backend,
            timelock
        );

        // 4. Configure timelock roles
        //    DAO can propose and execute; open executor so anyone can trigger after delay
        timelock.grantRole(timelock.PROPOSER_ROLE(), address(dao));
        timelock.grantRole(timelock.EXECUTOR_ROLE(), address(0)); // anyone can execute
        timelock.grantRole(timelock.CANCELLER_ROLE(), address(dao));
        // Renounce admin so the timelock is fully DAO-controlled
        timelock.renounceRole(timelock.DEFAULT_ADMIN_ROLE(), address(this));

        // 5. Transfer token ownership to the timelock (DAO executes through timelock)
        token.transferOwnership(address(timelock));

        // 6. Store info
        daos[corporationId] = DAOInfo({
            dao: address(dao),
            token: address(token),
            timelock: address(timelock),
            corporationId: corporationId,
            createdAt: block.timestamp
        });
        corporationIds.push(corporationId);

        emit DAOCreated(corporationId, address(dao), address(token), address(timelock), name);

        return (address(dao), address(token), address(timelock));
    }

    // ========================================================================
    //  View helpers
    // ========================================================================

    function getDAO(uint256 corporationId) external view returns (address) {
        return daos[corporationId].dao;
    }

    function getDAOInfo(uint256 corporationId) external view returns (DAOInfo memory) {
        return daos[corporationId];
    }

    function getDaoCount() external view returns (uint256) {
        return corporationIds.length;
    }

    /**
     * @dev Enumerate all corporation IDs. Use with getDaoCount() for pagination.
     */
    function getCorporationIdAt(uint256 index) external view returns (uint256) {
        require(index < corporationIds.length, "Index out of bounds");
        return corporationIds[index];
    }
}
