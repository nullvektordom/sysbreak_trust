Let me start by reading the contract and the audit methodology skill if available.Now let me perform the full audit.

---

## Pre-Audit Confirmation

- **Contract**: `SysbreakCreditBridge.sol` — 257 lines
- **Compiler**: `^0.8.20` (Solidity 0.8.20+)
- **Imports**: OpenZeppelin `Ownable`, `ReentrancyGuard`, `Pausable` (v5.x based on `Ownable(msg.sender)` constructor pattern)
- **Chain**: Not specified — appears to target Shido Network (native token bridge)
- **Upgrade pattern**: None (immutable deployment)
- **Prior audits**: None mentioned
- **External dependencies**: Backend oracle (off-chain), no on-chain oracle contracts

**Assumptions**: Deploying on an EVM-compatible chain where `transfer()` forwards 2300 gas. The `$SHIDO` referenced is the chain's native token (ETH-like), not an ERC-20.

---

## Findings

---

### 🔴 [CRITICAL] Request ID Collision Allows Withdrawal Hijacking / Silently Overwrites Pending Withdrawals

- **Category**: Logic Errors
- **Location**: `requestWithdrawal()` → line 151
- **Description**: The `requestId` is generated as `keccak256(abi.encodePacked(msg.sender, creditAmount, block.timestamp))`. If the same player submits two withdrawal requests for the same `creditAmount` in the same block (or same second), they produce **identical** `requestId` values. The second call silently overwrites the first `PendingWithdrawal` struct in storage. The first request is lost — the oracle can only execute one, and the player's first withdrawal disappears without a trace.

  Worse: on chains with sub-second block times or if `block.timestamp` has low granularity, collisions become more likely even across different blocks.

- **Impact**: Players lose pending withdrawals with no recourse. An attacker could intentionally grief themselves or (via contract interaction) create a scenario where the oracle executes a stale/manipulated request.
- **Proof of Concept**:
  1. Player calls `requestWithdrawal(10000)` in block N.
  2. In the same transaction bundle (or same block), player calls `requestWithdrawal(10000)` again.
  3. Both produce the same `requestId`. The second overwrites the first.
  4. Only one withdrawal can ever be executed — the other is gone.
- **Recommendation**: Add a nonce per user:
  ```solidity
  mapping(address => uint256) public withdrawalNonce;
  
  bytes32 requestId = keccak256(abi.encodePacked(
      msg.sender, creditAmount, block.timestamp, withdrawalNonce[msg.sender]++
  ));
  ```
  Also add `require(pendingWithdrawals[requestId].player == address(0), "Request ID exists");` as a safety check.

---

### 🔴 [CRITICAL] Daily Withdrawal Limit Bypassed — Enforced at Execution, Not Request Time

- **Category**: Logic Errors
- **Location**: `requestWithdrawal()` line 138–140 vs `executeWithdrawal()` line 180–181
- **Description**: The daily limit check happens in `requestWithdrawal()` (line 140), but the `dailyWithdrawals` counter is only incremented in `executeWithdrawal()` (line 181). A player can submit **unlimited** withdrawal requests within the same day as long as none have been *executed* yet — each request passes the limit check because the counter is still zero. The oracle then has no on-chain mechanism to prevent executing all of them.

  Additionally, `requestWithdrawal` and `executeWithdrawal` may occur on different days (up to 1 hour apart per line 174). A request made at 23:59 UTC and executed at 00:01 UTC the next day charges the *new* day's limit, effectively doubling the daily allowance.

- **Impact**: Complete bypass of the daily withdrawal limit. A player could drain the contract balance in a single day.
- **Proof of Concept**:
  1. Player submits 10 requests of 100,000 credits each in rapid succession (total: 1,000,000 credits).
  2. Each `requestWithdrawal` passes the limit check because `dailyWithdrawals` is still 0 — it's never incremented during requests.
  3. Oracle executes all 10. Each execution increments the counter, but the checks already passed.
- **Recommendation**: Track *requested* amounts separately and include them in the daily limit check:
  ```solidity
  mapping(address => mapping(uint256 => uint256)) public dailyRequested;
  
  // In requestWithdrawal:
  uint256 today = block.timestamp / 1 days;
  require(dailyRequested[msg.sender][today] + creditAmount <= dailyWithdrawalLimit, "Daily limit exceeded");
  dailyRequested[msg.sender][today] += creditAmount;
  ```
  On cancellation, decrement `dailyRequested`. On execution, optionally also track `dailyWithdrawals` for auditing.

---

### 🟠 [HIGH] Deposit Function Logic Error — Fee Applied to Gross but Refund Ignores Fee

- **Category**: Logic Errors / Precision Loss
- **Location**: `deposit()` → lines 112–127
- **Description**: The deposit flow has a logic inconsistency:
  1. `tokenAmount` is calculated from `creditAmount` (line 112) — this is the "expected" cost.
  2. `msg.value >= tokenAmount` is required (line 113).
  3. Fee is calculated on `msg.value` (line 116), and `netCredits` is derived from the post-fee amount.
  4. But the refund on line 125–127 refunds `msg.value - tokenAmount`.

  This means the fee is calculated on the entire `msg.value` (including any overpayment), but the refund gives back the overpayment *before* fee deduction. The contract keeps `tokenAmount`, charges a fee on `msg.value`, but refunds the excess. If `msg.value > tokenAmount`, the fee is applied to the excess too even though it's refunded — resulting in the player receiving **fewer credits** than they should for the amount actually retained.

  Additionally, the emitted `netCredits` in the `Deposit` event is based on `msg.value - fee`, not on the actual retained amount after refund, so the backend oracle would credit incorrect amounts.

- **Impact**: Players are systematically short-changed on credits when overpaying. The off-chain system receives incorrect credit amounts via events, causing accounting mismatches between on-chain and in-game state.
- **Recommendation**: Restructure the deposit to either (a) require exact payment and remove the refund, or (b) calculate fee only on the retained amount:
  ```solidity
  function deposit(uint256 creditAmount) external payable nonReentrant whenNotPaused {
      require(creditAmount >= 1000, "Minimum 1,000 credits");
      uint256 tokenAmount = (creditAmount * 1 ether) / CREDITS_PER_TOKEN;
      require(msg.value >= tokenAmount, "Insufficient tokens sent");
  
      // Refund excess first
      if (msg.value > tokenAmount) {
          payable(msg.sender).transfer(msg.value - tokenAmount);
      }
  
      // Fee on retained amount only
      uint256 fee = (tokenAmount * feeBps) / 10000;
      uint256 netCredits = ((tokenAmount - fee) * CREDITS_PER_TOKEN) / 1 ether;
  
      emit Deposit(msg.sender, tokenAmount, netCredits, fee);
  }
  ```

---

### 🟠 [HIGH] `transfer()` Used for ETH Sends — Breaks on Non-EOA Recipients and Some L2s

- **Category**: Denial of Service / Unchecked External Calls
- **Location**: Lines 127, 184, 234, 255
- **Description**: `payable(...).transfer(amount)` is used throughout for sending native tokens. `transfer()` forwards only 2300 gas, which is insufficient for:
  - Smart contract wallets (Gnosis Safe, Argent, etc.) with non-trivial `receive()` functions.
  - Some L2 chains where gas costs for opcodes differ from Ethereum mainnet.
  - Any recipient with a proxy pattern.

  On `executeWithdrawal()` (line 184), if the player's address is a smart contract wallet that requires >2300 gas, the transfer reverts, and the withdrawal is **permanently stuck** — it's marked `executed = true` before the transfer (line 177), but wait — actually it's marked before transfer on line 177 which is correct for CEI, but the `transfer` revert will revert the entire transaction including the state change. So it's not stuck, but the oracle cannot execute it, causing a DoS for that player.

- **Impact**: Players using smart contract wallets (increasingly common with account abstraction / ERC-4337) cannot withdraw. The oracle would repeatedly fail to execute their withdrawals until expiry.
- **Recommendation**: Replace all `transfer()` calls with `call()`:
  ```solidity
  (bool success, ) = payable(withdrawal.player).call{value: withdrawal.tokenAmount}("");
  require(success, "Transfer failed");
  ```

---

### 🟡 [MEDIUM] `withdrawFees()` Can Drain Funds Earmarked for Pending Withdrawals

- **Category**: Access Control / Logic Errors
- **Location**: `withdrawFees()` → line 232, `emergencyWithdraw()` → line 254
- **Description**: There is no accounting separation between collected fees and funds backing pending withdrawals. `withdrawFees()` lets the owner withdraw **any** amount up to the full contract balance, including funds reserved for pending withdrawals. Same for `emergencyWithdraw()` which drains everything.

- **Impact**: Owner (intentionally or accidentally) withdraws funds, causing subsequent `executeWithdrawal()` calls to fail due to insufficient balance. This is both a centralization risk and a potential rug vector.
- **Recommendation**: Track total pending withdrawal obligations:
  ```solidity
  uint256 public totalPendingTokens;
  
  // In requestWithdrawal: totalPendingTokens += tokenAmount;
  // In executeWithdrawal: totalPendingTokens -= tokenAmount;
  // In cancelWithdrawal: totalPendingTokens -= withdrawal.tokenAmount;
  
  function withdrawFees(uint256 amount) external onlyOwner {
      require(address(this).balance - amount >= totalPendingTokens, "Would underfund pending withdrawals");
      payable(owner()).transfer(amount);
  }
  ```

---

### 🟡 [MEDIUM] No Timelock on Critical Admin Functions

- **Category**: Centralization Risks
- **Location**: `setOracle()` line 63, `setFee()` line 73, `setDailyLimit()` line 83, `emergencyWithdraw()` line 254
- **Description**: All admin functions execute immediately with no timelock or multi-sig requirement. The owner can instantly change the oracle to a malicious address, set fees to 10%, set the daily limit to 0 (blocking all withdrawals), or drain the contract. For a bridge handling real player funds, this is a significant trust assumption.

- **Impact**: Single compromised key can drain all funds or manipulate the system. Players have no advance warning of parameter changes.
- **Recommendation**: Implement a timelock (e.g., OpenZeppelin `TimelockController`) for parameter changes, and consider a multi-sig for ownership. At minimum, add a 24–48 hour delay on `setOracle()` and `emergencyWithdraw()`.

---

### 🟡 [MEDIUM] Cancelled Withdrawals Don't Free Balance Reservation

- **Category**: Logic Errors
- **Location**: `cancelWithdrawal()` → line 192, `requestWithdrawal()` → line 148
- **Description**: `requestWithdrawal()` checks `address(this).balance >= tokenAmount` (line 148) but doesn't reserve/lock those tokens. Multiple concurrent requests can all pass the balance check, but the contract may not have enough to fulfill all of them. This is related to the lack of `totalPendingTokens` tracking mentioned above.

  Furthermore, cancellation deletes the struct but has no effect on any balance tracking (because none exists).

- **Impact**: Race condition where multiple players request withdrawals against the same balance. Later executions fail, causing unpredictable DoS.
- **Recommendation**: Same as the `withdrawFees` finding — implement `totalPendingTokens` tracking and check `address(this).balance - totalPendingTokens >= tokenAmount` in `requestWithdrawal()`.

---

### 🟡 [MEDIUM] `setDailyLimit()` Accepts Zero — Permanently Blocks All Withdrawals

- **Category**: Input Validation
- **Location**: `setDailyLimit()` → line 83
- **Description**: No minimum value check. Setting `dailyWithdrawalLimit = 0` would cause all `requestWithdrawal()` calls to revert on the daily limit check (since `0 + creditAmount > 0` is always true for `creditAmount >= 1000`).

- **Impact**: Owner can silently disable all withdrawals. While this might be intentional as a kill switch, it should be documented or use `pause()` instead for transparency.
- **Recommendation**: Add `require(_limit >= 1000, "Limit too low");` or document that zero is an intentional pause mechanism.

---

### 🔵 [LOW] Compiler Version Range Includes Yul Optimizer Bug Versions

- **Category**: Compiler Version
- **Location**: Line 2 — `pragma solidity ^0.8.20;`
- **Description**: The `^0.8.20` pragma allows compilation with 0.8.20 and 0.8.21, which have a known Yul optimizer bug affecting `via-IR` compilation. While this contract may not use `via-IR`, the pragma doesn't prevent it.

- **Recommendation**: Pin to `pragma solidity 0.8.24;` or `^0.8.22` minimum.

---

### 🔵 [LOW] OpenZeppelin Import Paths Suggest v4.x, Not v5.x

- **Category**: Dependency Audit
- **Location**: Lines 4–6
- **Description**: The import paths `@openzeppelin/contracts/security/ReentrancyGuard.sol` and `@openzeppelin/contracts/security/Pausable.sol` are the OZ v4.x paths. In OZ v5.x, these moved to `@openzeppelin/contracts/utils/ReentrancyGuard.sol` and `@openzeppelin/contracts/utils/Pausable.sol`. However, the `Ownable(msg.sender)` constructor pattern is v5.x. This inconsistency suggests either a mixed version setup or that the code hasn't been compiled yet.

- **Recommendation**: Verify OpenZeppelin version and use consistent import paths. If v5.x, update the security imports.

---

### 🔵 [LOW] Missing `receive()` Function — Contract Cannot Accept Plain ETH Transfers

- **Category**: ERC Standard Compliance
- **Location**: Contract-level
- **Description**: There's no `receive()` or `fallback()` function. The contract can only receive ETH via `deposit()` (which is `payable`) and `addLiquidity()`. If anyone sends ETH directly (e.g., from an exchange or a contract self-destructing), it will revert. This might be intentional, but it also means the oracle or owner cannot simply send ETH to fund the contract without calling `addLiquidity()`.

- **Recommendation**: Add a `receive() external payable {}` if direct funding should be supported, or document this is intentional.

---

### ⚪ [INFORMATIONAL] `cancelWithdrawal()` Missing `whenNotPaused` Modifier

- **Location**: `cancelWithdrawal()` → line 192
- **Description**: When the contract is paused, players cannot cancel their pending withdrawals. This may be intentional (full freeze), but could trap players in a pending state during maintenance.
- **Recommendation**: Consider allowing cancellations even when paused, as it's a non-value-transferring operation that favors the user.

---

### ⚪ [INFORMATIONAL] No Event Emitted on `emergencyWithdraw()` or `withdrawFees()`

- **Location**: Lines 232, 254
- **Description**: Owner fund movements are not logged on-chain, making off-chain monitoring and transparency harder.
- **Recommendation**: Add events for both.

---

## Executive Summary

**Overall Risk Level: HIGH**

| Severity | Count |
|----------|-------|
| 🔴 Critical | 2 |
| 🟠 High | 2 |
| 🟡 Medium | 3 |
| 🔵 Low | 3 |
| ⚪ Info | 2 |

**Top 3 Priorities:**
1. **Fix request ID collision** — add a nonce to prevent silent overwrites of pending withdrawals.
2. **Fix daily limit bypass** — track requested amounts at request time, not execution time.
3. **Fix deposit fee/refund logic** — ensure credits emitted to the backend match the actual retained funds.

---

## Architecture Review

- **Trust assumptions**: Heavy reliance on a single `oracle` address and single `owner`. Both are single points of failure with no timelock or multi-sig.
- **Upgrade path**: None — immutable deployment. Bugs require redeployment and migration.
- **Admin powers**: Owner can change oracle, fees, limits, pause, and drain all funds instantly.
- **External dependencies**: Off-chain backend reads `Deposit` events to credit players — if event data is wrong (as noted in the deposit bug), the entire credit system is wrong.

## Dependency Audit

- **OpenZeppelin**: Version ambiguous (mixed v4/v5 import paths). Verify and pin.
- **No external oracle contracts** — oracle is a trusted EOA/backend, which is appropriate for a game bridge but is a centralization vector.

## Suggested Tooling

- **Slither**: Will catch the `transfer()` usage, missing zero-checks, and reentrancy patterns.
- **Foundry fuzz tests**: Fuzz `deposit()` with various `msg.value`/`creditAmount` combinations to verify fee/refund accounting. Fuzz concurrent `requestWithdrawal` calls to demonstrate the daily limit bypass.
- **Forta / OpenZeppelin Defender**: Monitor for `OracleUpdated`, `emergencyWithdraw`, and anomalous withdrawal volumes post-deployment.

## OWASP SC Top 10 (2026) Coverage

| Category | Covered? |
|----------|----------|
| Access Control | ⚠️ Partial — roles exist but no timelock/multi-sig |
| Logic Errors | ❌ Multiple issues found |
| Reentrancy | ✅ `ReentrancyGuard` applied correctly |
| Signature/Auth | N/A — no signatures used |
| Flash Loan | ⚠️ Low risk — native token bridge, but no flash-loan guards |
| Input Validation | ⚠️ Partial — missing bounds on some params |
| Oracle Manipulation | N/A — off-chain oracle, not price oracle |
| Front-running/MEV | ⚠️ Request IDs are predictable, could be front-run |
| Integer Overflow | ✅ Solidity 0.8.x built-in checks |
| Denial of Service | ❌ `transfer()` issue across multiple functions |