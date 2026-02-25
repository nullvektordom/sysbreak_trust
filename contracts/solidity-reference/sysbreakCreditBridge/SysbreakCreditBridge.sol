// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title SysbreakCreditBridge
 * @author Nex Qiros / Nullvektor Dominion
 * @dev Bridge contract for converting between in-game credits and $SHIDO native tokens.
 *
 * Features:
 *   - Deposit:    Send $SHIDO → receive in-game credits (off-chain via event)
 *   - Withdrawal: Request credit→token conversion, oracle verifies & executes
 *   - Dynamic conversion rate: weiPerCredit (oracle-adjusted to peg 1 credit ≈ $0.001 USD)
 *   - Oracle rate updates with 50% max delta protection
 *   - Owner rate override via 24h timelock
 *   - Daily withdrawal limits per player (enforced at request time)
 *   - Fee system (basis points) for sustainability
 *   - Backend oracle for credit verification and rate updates
 *   - Timelock on sensitive admin operations (oracle, fee, rate changes)
 *   - Pending withdrawal balance tracking to prevent fund misallocation
 *
 * Audit fixes applied:
 *   [CRITICAL] Request ID collision — added per-user nonce
 *   [CRITICAL] Daily limit bypass — enforced at request time, decremented on cancel
 *   [HIGH]     Deposit fee/refund logic — fee calculated on retained amount only
 *   [HIGH]     transfer() → call() for L2/smart-wallet compatibility
 *   [MEDIUM]   withdrawFees() can no longer drain pending withdrawal funds
 *   [MEDIUM]   Timelock on oracle, fee, and daily limit changes
 *   [MEDIUM]   setDailyLimit() rejects zero
 *   [LOW]      Compiler pinned to 0.8.24
 *   [LOW]      OZ v5 import paths
 *   [LOW]      Added receive() for direct funding
 *   [INFO]     cancelWithdrawal() works even when paused
 *   [INFO]     Events on all fund movements
 */
contract SysbreakCreditBridge is Ownable, ReentrancyGuard, Pausable {

    // ─── Constants ───────────────────────────────────────────────────────

    /// @dev Maximum fee: 10% (1000 basis points)
    uint256 public constant MAX_FEE_BPS = 1000;

    /// @dev Bounds to prevent catastrophic misconfiguration
    uint256 public constant MIN_WEI_PER_CREDIT = 0.0001 ether;
    uint256 public constant MAX_WEI_PER_CREDIT = 1_000_000 ether;

    /// @dev Max rate change per oracle update: 50%
    uint256 public constant MAX_RATE_CHANGE_BPS = 5_000;

    /// @dev Minimum daily withdrawal limit (in credits)
    uint256 public constant MIN_DAILY_LIMIT = 1000;

    /// @dev Timelock delay for admin changes
    uint256 public constant TIMELOCK_DELAY = 24 hours;

    /// @dev Withdrawal request expiry
    uint256 public constant WITHDRAWAL_EXPIRY = 1 hours;

    // ─── State ───────────────────────────────────────────────────────────

    /// @dev Wei of $SHIDO per 1 credit. Oracle adjusts to maintain $0.001 USD peg.
    uint256 public weiPerCredit = 4 ether; // Initial: 4 SHIDO = 1 credit

    /// @dev Fee in basis points (50 = 0.5%)
    uint256 public feeBps = 50;

    /// @dev Daily withdrawal limit per player (in credits)
    uint256 public dailyWithdrawalLimit = 100_000;

    /// @dev Backend oracle address (authorized to confirm withdrawals)
    address public oracle;

    /// @dev Total tokens reserved for pending (unexecuted) withdrawals
    uint256 public totalPendingTokens;

    /// @dev Per-user nonce to prevent request ID collisions
    mapping(address => uint256) public withdrawalNonce;

    /// @dev Daily requested credits per player (tracked at request time)
    mapping(address => mapping(uint256 => uint256)) public dailyRequested;

    /// @dev Pending withdrawal requests
    mapping(bytes32 => PendingWithdrawal) public pendingWithdrawals;

    // ─── Timelock ────────────────────────────────────────────────────────

    struct TimelockProposal {
        bytes32 actionHash;
        uint256 readyAt;
        bool exists;
    }

    mapping(bytes32 => TimelockProposal) public timelockProposals;

    // ─── Structs ─────────────────────────────────────────────────────────

    struct PendingWithdrawal {
        address player;
        uint256 creditAmount;
        uint256 tokenAmount;
        uint256 timestamp;
        bool executed;
        bool cancelled;
    }

    // ─── Events ──────────────────────────────────────────────────────────

    event Deposit(
        address indexed player,
        uint256 tokenAmount,
        uint256 creditAmount,
        uint256 fee
    );
    event WithdrawalRequested(
        bytes32 indexed requestId,
        address indexed player,
        uint256 creditAmount,
        uint256 tokenAmount
    );
    event WithdrawalExecuted(
        bytes32 indexed requestId,
        address indexed player,
        uint256 tokenAmount
    );
    event WithdrawalCancelled(
        bytes32 indexed requestId,
        address indexed player
    );
    event WithdrawalExpired(
        bytes32 indexed requestId,
        address indexed player
    );
    event FeeUpdated(uint256 oldFee, uint256 newFee);
    event OracleUpdated(address indexed oldOracle, address indexed newOracle);
    event DailyLimitUpdated(uint256 oldLimit, uint256 newLimit);
    event FeesWithdrawn(address indexed to, uint256 amount);
    event LiquidityAdded(address indexed from, uint256 amount);
    event EmergencyWithdrawal(address indexed to, uint256 amount);
    event TimelockQueued(bytes32 indexed proposalId, string action, uint256 readyAt);
    event TimelockExecuted(bytes32 indexed proposalId, string action);
    event TimelockCancelled(bytes32 indexed proposalId, string action);
    event RateUpdated(uint256 oldRate, uint256 newRate);

    // ─── Errors ──────────────────────────────────────────────────────────

    error ZeroAddress();
    error FeeTooHigh(uint256 requested, uint256 maximum);
    error DailyLimitTooLow(uint256 requested, uint256 minimum);
    error InsufficientPayment(uint256 sent, uint256 required);
    error InsufficientContractBalance(uint256 available, uint256 required);
    error DailyLimitExceeded(uint256 used, uint256 limit);
    error InvalidRequest();
    error RequestAlreadyFinalized();
    error RequestExpired();
    error RequestNotExpired();
    error NotYourRequest();
    error OnlyOracle();
    error TransferFailed();
    error TimelockNotReady();
    error TimelockNotFound();
    error TimelockAlreadyExists();
    error RequestIdCollision();
    error RateOutOfBounds(uint256 requested, uint256 min, uint256 max);
    error RateChangeTooLarge(uint256 delta, uint256 maxDelta);

    // ─── Constructor ─────────────────────────────────────────────────────

    constructor() Ownable(msg.sender) {
        oracle = msg.sender;
    }

    // ─── Timelock Internals ──────────────────────────────────────────────

    function _queueTimelock(
        string memory action,
        bytes memory params
    ) internal returns (bytes32 proposalId) {
        proposalId = keccak256(abi.encodePacked(action, params));
        if (timelockProposals[proposalId].exists) revert TimelockAlreadyExists();

        uint256 readyAt = block.timestamp + TIMELOCK_DELAY;
        timelockProposals[proposalId] = TimelockProposal({
            actionHash: proposalId,
            readyAt: readyAt,
            exists: true
        });

        emit TimelockQueued(proposalId, action, readyAt);
    }

    function _executeTimelock(
        string memory action,
        bytes memory params
    ) internal returns (bytes32 proposalId) {
        proposalId = keccak256(abi.encodePacked(action, params));
        TimelockProposal storage proposal = timelockProposals[proposalId];

        if (!proposal.exists) revert TimelockNotFound();
        if (block.timestamp < proposal.readyAt) revert TimelockNotReady();

        delete timelockProposals[proposalId];
        emit TimelockExecuted(proposalId, action);
    }

    function cancelTimelock(
        string calldata action,
        bytes calldata params
    ) external onlyOwner {
        bytes32 proposalId = keccak256(abi.encodePacked(action, params));
        if (!timelockProposals[proposalId].exists) revert TimelockNotFound();

        delete timelockProposals[proposalId];
        emit TimelockCancelled(proposalId, action);
    }

    // ─── Admin: Timelocked ───────────────────────────────────────────────

    /// @notice Queue an oracle change. Must wait TIMELOCK_DELAY before executing.
    function queueSetOracle(address _oracle) external onlyOwner {
        if (_oracle == address(0)) revert ZeroAddress();
        _queueTimelock("setOracle", abi.encode(_oracle));
    }

    /// @notice Execute a previously queued oracle change.
    function executeSetOracle(address _oracle) external onlyOwner {
        if (_oracle == address(0)) revert ZeroAddress();
        _executeTimelock("setOracle", abi.encode(_oracle));

        address oldOracle = oracle;
        oracle = _oracle;
        emit OracleUpdated(oldOracle, _oracle);
    }

    /// @notice Queue a fee change. Must wait TIMELOCK_DELAY before executing.
    function queueSetFee(uint256 _feeBps) external onlyOwner {
        if (_feeBps > MAX_FEE_BPS) revert FeeTooHigh(_feeBps, MAX_FEE_BPS);
        _queueTimelock("setFee", abi.encode(_feeBps));
    }

    /// @notice Execute a previously queued fee change.
    function executeSetFee(uint256 _feeBps) external onlyOwner {
        if (_feeBps > MAX_FEE_BPS) revert FeeTooHigh(_feeBps, MAX_FEE_BPS);
        _executeTimelock("setFee", abi.encode(_feeBps));

        uint256 oldFee = feeBps;
        feeBps = _feeBps;
        emit FeeUpdated(oldFee, _feeBps);
    }

    /// @notice Queue a daily limit change. Must wait TIMELOCK_DELAY before executing.
    function queueSetDailyLimit(uint256 _limit) external onlyOwner {
        if (_limit < MIN_DAILY_LIMIT) revert DailyLimitTooLow(_limit, MIN_DAILY_LIMIT);
        _queueTimelock("setDailyLimit", abi.encode(_limit));
    }

    /// @notice Execute a previously queued daily limit change.
    function executeSetDailyLimit(uint256 _limit) external onlyOwner {
        if (_limit < MIN_DAILY_LIMIT) revert DailyLimitTooLow(_limit, MIN_DAILY_LIMIT);
        _executeTimelock("setDailyLimit", abi.encode(_limit));

        uint256 oldLimit = dailyWithdrawalLimit;
        dailyWithdrawalLimit = _limit;
        emit DailyLimitUpdated(oldLimit, _limit);
    }

    /// @notice Queue a rate change. Must wait TIMELOCK_DELAY before executing.
    /// @dev Owner-only override; bypasses max delta check since it's timelocked.
    function queueSetRate(uint256 _weiPerCredit) external onlyOwner {
        if (_weiPerCredit < MIN_WEI_PER_CREDIT || _weiPerCredit > MAX_WEI_PER_CREDIT)
            revert RateOutOfBounds(_weiPerCredit, MIN_WEI_PER_CREDIT, MAX_WEI_PER_CREDIT);
        _queueTimelock("setRate", abi.encode(_weiPerCredit));
    }

    /// @notice Execute a previously queued rate change.
    function executeSetRate(uint256 _weiPerCredit) external onlyOwner {
        if (_weiPerCredit < MIN_WEI_PER_CREDIT || _weiPerCredit > MAX_WEI_PER_CREDIT)
            revert RateOutOfBounds(_weiPerCredit, MIN_WEI_PER_CREDIT, MAX_WEI_PER_CREDIT);
        _executeTimelock("setRate", abi.encode(_weiPerCredit));

        uint256 oldRate = weiPerCredit;
        weiPerCredit = _weiPerCredit;
        emit RateUpdated(oldRate, _weiPerCredit);
    }

    // ─── Oracle: Rate Update ────────────────────────────────────────────

    /// @notice Update the credit rate. Oracle only. Enforces max 50% change per update.
    function updateRate(uint256 newWeiPerCredit) external {
        if (msg.sender != oracle) revert OnlyOracle();
        if (newWeiPerCredit < MIN_WEI_PER_CREDIT || newWeiPerCredit > MAX_WEI_PER_CREDIT)
            revert RateOutOfBounds(newWeiPerCredit, MIN_WEI_PER_CREDIT, MAX_WEI_PER_CREDIT);

        uint256 current = weiPerCredit;
        uint256 delta = newWeiPerCredit > current
            ? newWeiPerCredit - current
            : current - newWeiPerCredit;
        uint256 maxDelta = (current * MAX_RATE_CHANGE_BPS) / 10_000;
        if (delta > maxDelta) revert RateChangeTooLarge(delta, maxDelta);

        uint256 oldRate = weiPerCredit;
        weiPerCredit = newWeiPerCredit;
        emit RateUpdated(oldRate, newWeiPerCredit);
    }

    // ─── Admin: Instant (safety operations) ──────────────────────────────

    /// @notice Pause the contract. Instant — no timelock (protective action).
    function pause() external onlyOwner {
        _pause();
    }

    /// @notice Unpause the contract.
    function unpause() external onlyOwner {
        _unpause();
    }

    /// @notice Add liquidity to the bridge. Anyone can call.
    function addLiquidity() external payable {
        require(msg.value > 0, "Must send tokens");
        emit LiquidityAdded(msg.sender, msg.value);
    }

    /// @notice Withdraw collected fees. Cannot withdraw funds reserved for pending withdrawals.
    function withdrawFees(uint256 amount) external onlyOwner {
        uint256 available = address(this).balance - totalPendingTokens;
        if (amount > available)
            revert InsufficientContractBalance(available, amount);

        emit FeesWithdrawn(owner(), amount);

        (bool success, ) = payable(owner()).call{value: amount}("");
        if (!success) revert TransferFailed();
    }

    /// @notice Emergency withdrawal. Pauses the contract first.
    /// @dev Drains everything including pending — use only in true emergencies.
    function emergencyWithdraw() external onlyOwner {
        _pause();
        uint256 balance = address(this).balance;

        emit EmergencyWithdrawal(owner(), balance);

        (bool success, ) = payable(owner()).call{value: balance}("");
        if (!success) revert TransferFailed();
    }

    // ─── Player: Deposit ─────────────────────────────────────────────────

    /**
     * @notice Deposit $SHIDO to receive in-game credits.
     * @dev Exact payment required for the requested creditAmount.
     *      Fee is deducted from the retained amount. Excess is refunded.
     *      The backend oracle reads the Deposit event to credit the player.
     * @param creditAmount The gross number of credits to purchase (before fee).
     */
    function deposit(uint256 creditAmount) external payable nonReentrant whenNotPaused {
        require(creditAmount >= 1000, "Minimum 1,000 credits");

        // Calculate required token amount for requested credits
        uint256 tokenAmount = creditAmount * weiPerCredit;
        if (msg.value < tokenAmount)
            revert InsufficientPayment(msg.value, tokenAmount);

        // Refund excess FIRST (before fee calculation)
        uint256 excess = msg.value - tokenAmount;
        if (excess > 0) {
            (bool refunded, ) = payable(msg.sender).call{value: excess}("");
            if (!refunded) revert TransferFailed();
        }

        // Fee on retained amount only
        uint256 fee = (tokenAmount * feeBps) / 10_000;
        uint256 netCredits = (tokenAmount - fee) / weiPerCredit;

        emit Deposit(msg.sender, tokenAmount, netCredits, fee);
    }

    // ─── Player: Withdrawal Flow ─────────────────────────────────────────

    /**
     * @notice Request withdrawal of in-game credits to $SHIDO tokens.
     * @dev Daily limit is enforced and tracked at request time (not execution).
     *      Token amount is reserved from contract balance.
     *      Oracle must call executeWithdrawal() within 1 hour.
     * @param creditAmount The number of credits to withdraw.
     */
    function requestWithdrawal(uint256 creditAmount) external nonReentrant whenNotPaused {
        require(creditAmount >= 1000, "Minimum 1,000 credits");

        // ── Daily limit (enforced at request time) ──
        uint256 today = block.timestamp / 1 days;
        uint256 usedToday = dailyRequested[msg.sender][today];
        if (usedToday + creditAmount > dailyWithdrawalLimit)
            revert DailyLimitExceeded(usedToday, dailyWithdrawalLimit);

        // ── Calculate token amount after fee ──
        uint256 fee = (creditAmount * feeBps) / 10_000;
        uint256 creditsAfterFee = creditAmount - fee;
        uint256 tokenAmount = creditsAfterFee * weiPerCredit;

        // ── Check available balance (excluding already-reserved tokens) ──
        uint256 available = address(this).balance - totalPendingTokens;
        if (available < tokenAmount)
            revert InsufficientContractBalance(available, tokenAmount);

        // ── Generate collision-resistant request ID ──
        bytes32 requestId = keccak256(
            abi.encodePacked(
                msg.sender,
                creditAmount,
                block.timestamp,
                withdrawalNonce[msg.sender]++
            )
        );

        // Safety: should never happen with nonce, but guard anyway
        if (pendingWithdrawals[requestId].player != address(0))
            revert RequestIdCollision();

        // ── Reserve tokens and track daily usage ──
        totalPendingTokens += tokenAmount;
        dailyRequested[msg.sender][today] += creditAmount;

        pendingWithdrawals[requestId] = PendingWithdrawal({
            player: msg.sender,
            creditAmount: creditAmount,
            tokenAmount: tokenAmount,
            timestamp: block.timestamp,
            executed: false,
            cancelled: false
        });

        emit WithdrawalRequested(requestId, msg.sender, creditAmount, tokenAmount);
    }

    /**
     * @notice Execute a pending withdrawal. Oracle only.
     * @dev Transfers reserved tokens to the player. Must be within expiry window.
     * @param requestId The withdrawal request ID.
     */
    function executeWithdrawal(bytes32 requestId) external nonReentrant {
        if (msg.sender != oracle) revert OnlyOracle();

        PendingWithdrawal storage w = pendingWithdrawals[requestId];
        if (w.player == address(0)) revert InvalidRequest();
        if (w.executed || w.cancelled) revert RequestAlreadyFinalized();
        if (block.timestamp > w.timestamp + WITHDRAWAL_EXPIRY) revert RequestExpired();

        // ── Finalize (CEI: state changes before external call) ──
        w.executed = true;
        totalPendingTokens -= w.tokenAmount;

        emit WithdrawalExecuted(requestId, w.player, w.tokenAmount);

        // ── Transfer ──
        (bool success, ) = payable(w.player).call{value: w.tokenAmount}("");
        if (!success) revert TransferFailed();
    }

    /**
     * @notice Cancel a pending withdrawal. Player only. Works even when paused.
     * @dev Releases reserved tokens and daily limit allocation.
     * @param requestId The withdrawal request ID.
     */
    function cancelWithdrawal(bytes32 requestId) external {
        PendingWithdrawal storage w = pendingWithdrawals[requestId];
        if (w.player != msg.sender) revert NotYourRequest();
        if (w.executed || w.cancelled) revert RequestAlreadyFinalized();

        // ── Release reserves ──
        w.cancelled = true;
        totalPendingTokens -= w.tokenAmount;

        // Release daily limit
        uint256 day = w.timestamp / 1 days;
        dailyRequested[w.player][day] -= w.creditAmount;

        emit WithdrawalCancelled(requestId, w.player);
    }

    /**
     * @notice Clean up an expired withdrawal. Anyone can call.
     * @dev Releases reserved tokens and daily limit for expired requests.
     * @param requestId The withdrawal request ID.
     */
    function cleanupExpiredWithdrawal(bytes32 requestId) external {
        PendingWithdrawal storage w = pendingWithdrawals[requestId];
        if (w.player == address(0)) revert InvalidRequest();
        if (w.executed || w.cancelled) revert RequestAlreadyFinalized();
        if (block.timestamp <= w.timestamp + WITHDRAWAL_EXPIRY) revert RequestNotExpired();

        // ── Release reserves ──
        w.cancelled = true;
        totalPendingTokens -= w.tokenAmount;

        // Release daily limit
        uint256 day = w.timestamp / 1 days;
        dailyRequested[w.player][day] -= w.creditAmount;

        emit WithdrawalExpired(requestId, w.player);
    }

    // ─── Views ───────────────────────────────────────────────────────────

    /// @notice Get player's remaining daily withdrawal limit (in credits).
    function getRemainingDailyLimit(address player) external view returns (uint256) {
        uint256 today = block.timestamp / 1 days;
        uint256 usedToday = dailyRequested[player][today];
        if (usedToday >= dailyWithdrawalLimit) return 0;
        return dailyWithdrawalLimit - usedToday;
    }

    /// @notice Calculate how many tokens (in wei) a credit amount converts to (before fee).
    function calculateTokenAmount(uint256 creditAmount) external view returns (uint256) {
        return creditAmount * weiPerCredit;
    }

    /// @notice Calculate how many credits a token amount (in wei) converts to.
    function calculateCreditAmount(uint256 tokenAmount) external view returns (uint256) {
        return tokenAmount / weiPerCredit;
    }

    /// @notice Available balance (total minus reserved for pending withdrawals).
    function getAvailableBalance() external view returns (uint256) {
        return address(this).balance - totalPendingTokens;
    }

    /// @notice Total contract balance.
    function getBalance() external view returns (uint256) {
        return address(this).balance;
    }

    // ─── Receive ─────────────────────────────────────────────────────────

    /// @dev Accept direct ETH transfers (for funding/liquidity).
    receive() external payable {
        emit LiquidityAdded(msg.sender, msg.value);
    }
}
