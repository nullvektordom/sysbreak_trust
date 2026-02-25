# Security Audit Report — CorporationDAO.sol

**Contract System:** CorporationGovernanceToken, CorporationDAO, CorporationDAOFactory
**Lines of Code:** ~284
**Compiler:** `^0.8.20` (floating pragma)
**Framework:** OpenZeppelin Governor + ERC20Votes
**Auditor:** Claude (AI-Assisted)
**Date:** 2026-02-23

---

## Pre-Audit Checklist

| Item | Status |
|------|--------|
| Compiler version | `^0.8.20` — floating pragma, exact version unknown |
| Deployment chain | Unknown — SYSBREAK context suggests custom/L2 |
| External contracts | OpenZeppelin Governor, ERC20Votes, ERC20Permit |
| Trust assumptions | `backend` address has significant privileges |
| Upgrade pattern | **None** — immutable deployment |
| Test coverage | Not provided |
| Prior audits | Not provided |

**Assumptions made:** OpenZeppelin 5.x is used (based on `Ownable(msg.sender)` constructor pattern). No proxy/upgrade pattern. Contracts are intended for an on-chain game (SYSBREAK).

---

## Findings

---

### 🔴 CRITICAL — Compilation Error: Incorrect Override Type Name

- **Category:** Logic Error / Code Correctness
- **Location:** `CorporationGovernanceToken` → `nonces()` → Line 43
- **Description:** The override specifies `ERC20Permits` (with an 's'), but the correct type name is `ERC20Permit`. This contract **will not compile**. The nonces function override also references `Nonces`, which may or may not be correct depending on the exact OZ version — in OZ 5.x the `Nonces` base is correct, but `ERC20Permits` is not a valid contract name in any OZ version.
- **Impact:** Deployment is impossible. The entire system is blocked.
- **Recommendation:**
  ```solidity
  function nonces(address owner) public view override(ERC20Permit, Nonces) returns (uint256) {
      return super.nonces(owner);
  }
  ```

---

### 🔴 CRITICAL — `backend` Address is an Unprotected Single Point of Failure (Factory)

- **Category:** Access Control / Centralization Risk
- **Location:** `CorporationDAOFactory` → `backend` state variable → Line 202
- **Description:** The factory's `backend` address controls all DAO creation and is set once in the constructor with **no mechanism to ever change it**. If the backend key is compromised, lost, or rotated, the factory is permanently bricked or permanently exploitable.
- **Impact:**
  - **Key compromise:** Attacker can create unlimited DAOs, mint arbitrary token distributions, and set themselves as backend on all new DAOs.
  - **Key loss:** No new DAOs can ever be created.
- **Recommendation:** Add an ownership/governance mechanism to update the factory backend:
  ```solidity
  address public owner;

  function setBackend(address _backend) external {
      require(msg.sender == owner, "Only owner");
      backend = _backend;
  }
  ```
  Or better: use `Ownable` with a timelock for backend rotation.

---

### 🟠 HIGH — `backend` Can Unilaterally Control Governance via Officer Manipulation

- **Category:** Access Control
- **Location:** `CorporationDAO` → `setOfficer()` → Line 100
- **Description:** The `backend` can grant/revoke officer status at will. Officers are the only addresses that can create proposals (line 116). This means the backend has unilateral power to:
  1. Remove all officers → governance is dead (no new proposals).
  2. Add a colluding officer → propose malicious actions (e.g., drain the treasury).
  Combined with the fact that `backend` is a single EOA/address with no timelock or multisig requirement, this creates a severe trust assumption.
- **Impact:** Complete governance bypass. The backend operator can effectively control the DAO by choosing who can propose.
- **Recommendation:**
  - Implement a timelock on officer changes (e.g., 24h delay before changes take effect).
  - Emit events with sufficient lead time for token holders to react.
  - Consider making officer management a governance-controlled action (proposal-based), not purely backend-controlled.

---

### 🟠 HIGH — `proposeSimple` Uses `abi.encodePacked` for Function Call Encoding

- **Category:** Logic Error
- **Location:** `CorporationDAO` → `proposeSimple()` → Line 144
- **Description:** The function encodes calldata using:
  ```solidity
  abi.encodePacked(bytes4(keccak256(bytes(signature))), data)
  ```
  This is **incorrect ABI encoding**. The EVM expects `abi.encodeWithSelector` or `abi.encodeWithSignature` for proper function calls. `abi.encodePacked` does not apply ABI padding to dynamic types, which means:
  - Calls with dynamic-type arguments (strings, bytes, arrays) will produce malformed calldata.
  - The target contract will revert or behave unexpectedly.
- **Impact:** Any proposal created via `proposeSimple` with dynamic-type arguments will silently produce incorrect calldata. When executed, the call will either revert (best case) or hit an unintended function/selector (worst case).
- **Proof of Concept:**
  1. Officer calls `proposeSimple(target, 0, "transfer(address,uint256)", abi.encode(addr, amt), "desc")`
  2. The `data` parameter is already ABI-encoded, but `abi.encodePacked` with `bytes4` creates non-standard encoding.
  3. The resulting calldata may not match what the target expects.
- **Recommendation:**
  ```solidity
  if (bytes(signature).length == 0) {
      calldatas[0] = data;
  } else {
      calldatas[0] = abi.encodeWithSignature(signature, data);
      // Or better: require callers to pass pre-encoded calldata and remove signature param
  }
  ```
  Simplest fix: remove the `signature` parameter entirely and always pass pre-encoded calldata (as `propose()` already expects).

---

### 🟠 HIGH — `proposeTransfer` Produces Unreadable/Broken Description String

- **Category:** Logic Error
- **Location:** `CorporationDAO` → `proposeTransfer()` → Line 167
- **Description:** The function constructs a description string via:
  ```solidity
  string(abi.encodePacked("Transfer ", amount, " to ", recipient, ": ", reason))
  ```
  `abi.encodePacked` on a `uint256` produces 32 raw bytes, not a decimal string. On an `address` it produces 20 raw bytes, not a hex string. The resulting "description" will contain non-printable binary data, making it unreadable in any UI or indexer.

  More critically: in OpenZeppelin Governor, the proposal description is part of the **proposal ID hash**. A garbled description means proposal IDs become unpredictable and proposals cannot be easily identified or verified off-chain.
- **Impact:** Broken governance UX. Proposals created via `proposeTransfer` will have garbled descriptions containing raw bytes. Off-chain tools and block explorers will display garbage.
- **Recommendation:**
  ```solidity
  // Use Strings library from OpenZeppelin
  import "@openzeppelin/contracts/utils/Strings.sol";

  string memory description = string.concat(
      "Transfer ",
      Strings.toString(amount),
      " to ",
      Strings.toHexString(recipient),
      ": ",
      reason
  );
  ```

---

### 🟠 HIGH — Flash Loan Governance Attack via Unrestricted Token Transfers

- **Category:** Flash Loan Attack Vectors
- **Location:** `CorporationGovernanceToken` (inherits ERC20 — freely transferable) + `CorporationDAO` → `quorum()` → Line 181
- **Description:** The governance token is freely transferable with no transfer restrictions. The quorum is a **fixed absolute number** (1000e18) rather than a percentage of total supply. Combined with the 1-block voting delay (line 78), this enables a flash loan governance attack:
  1. Attacker flash-borrows or temporarily acquires 1000+ tokens.
  2. Delegates to self (or already has delegation).
  3. Waits 1 block for snapshot.
  4. Votes on a malicious proposal (e.g., drain treasury).
  5. Returns tokens.

  The 1-block voting delay is dangerously short — on most EVM chains this is 12 seconds.
- **Impact:** Complete treasury drainage via flash-loan-assisted governance attack.
- **Recommendation:**
  - Increase voting delay to at least 1 day (~7200 blocks on Ethereum).
  - Use a percentage-based quorum (e.g., `GovernorVotesQuorumFraction`) instead of a fixed amount.
  - Consider adding transfer restrictions or a token lockup requirement for voting eligibility.

---

### 🟡 MEDIUM — Token Owner (`mint`/`burn`) is the DAO, but DAO Has No Way to Call Them

- **Category:** Logic Error / Access Control
- **Location:** `CorporationGovernanceToken` → `mint()` / `burn()` → Lines 30-36; Factory → Line 254
- **Description:** The factory transfers token ownership to the DAO (line 254). `mint` and `burn` require `onlyOwner`. For the DAO to call these, a governance proposal must target the token contract with the appropriate calldata. This works in theory, but:
  1. The `proposeSimple` function has broken ABI encoding (see HIGH finding above), so it can't reliably be used for this.
  2. There's no dedicated helper function for mint/burn proposals.
  3. If `propose()` is used directly, it requires technical knowledge to encode calldata correctly.

  This means the intended token management flow (DAO-governed minting/burning) is practically inaccessible without external tooling.
- **Impact:** Token supply management is effectively broken without off-chain calldata encoding tools.
- **Recommendation:** Add helper functions:
  ```solidity
  function proposeMint(address to, uint256 amount) external returns (uint256) { ... }
  function proposeBurn(address from, uint256 amount) external returns (uint256) { ... }
  ```

---

### 🟡 MEDIUM — No `disableInitializers` Protection, but N/A (Non-Upgradeable)

- **Category:** Upgradability Risks
- **Location:** All three contracts
- **Description:** Contracts are not upgradeable, so `disableInitializers` is not needed. However, if these contracts are ever wrapped in a proxy pattern in the future, the constructors would not run and all state would be uninitialized. **This is informational for now** but worth noting given that the SYSBREAK project may evolve.
- **Impact:** No current impact. Future risk if upgrade patterns are adopted.
- **Recommendation:** If proxy patterns are ever considered, refactor to initializer-based patterns.

---

### 🟡 MEDIUM — Unbounded Loop in Token Constructor

- **Category:** Denial of Service
- **Location:** `CorporationGovernanceToken` → constructor → Lines 25-27
- **Description:** The constructor iterates over `initialHolders` with no upper bound. If the backend passes a sufficiently large array, the deployment transaction could exceed the block gas limit and fail.
- **Impact:** DoS on DAO creation for large initial member sets.
- **Recommendation:** Add a reasonable upper bound:
  ```solidity
  require(initialHolders.length <= 100, "Too many initial holders");
  ```

---

### 🟡 MEDIUM — No Zero-Address Validation

- **Category:** Input Validation
- **Location:** Multiple:
  - `CorporationDAO` constructor → `_backend` (line 74)
  - `CorporationDAO` → `setBackend()` (line 91)
  - `CorporationDAO` → `setOfficer()` (line 100)
  - `CorporationDAOFactory` constructor → `_backend` (line 221)
  - `CorporationGovernanceToken` → `mint()` (line 30)
- **Description:** None of these functions validate against `address(0)`. Setting backend to the zero address permanently bricks officer management and DAO creation.
- **Impact:** Accidental or malicious zero-address assignment could permanently disable critical functionality.
- **Recommendation:**
  ```solidity
  require(_backend != address(0), "Zero address");
  ```

---

### 🟡 MEDIUM — `daoCount` Can Desync from Actual DAO Count

- **Category:** Logic Error
- **Location:** `CorporationDAOFactory` → `createCorporationDAO()` → Line 264
- **Description:** `daoCount` is incremented on each creation and serves as a counter, but DAOs are indexed by `corporationId` (which is arbitrary), not by sequential index. There is no way to enumerate all DAOs by iterating `0..daoCount` since the mapping is keyed by `corporationId`. The `daoCount` variable is misleading and not useful for enumeration.
- **Impact:** Off-chain tools relying on `daoCount` for enumeration will miss DAOs or iterate incorrectly.
- **Recommendation:** Either remove `daoCount` or add an array of `corporationId`s for enumeration:
  ```solidity
  uint256[] public corporationIds;
  ```

---

### 🔵 LOW — Floating Pragma `^0.8.20`

- **Category:** Compiler Version
- **Location:** Line 2
- **Description:** A floating pragma allows compilation with any 0.8.x version ≥ 0.8.20. Versions 0.8.20–0.8.21 have a known Yul optimizer bug when using `via-IR`. The deployed bytecode could vary depending on the compiler used.
- **Impact:** Potential unexpected behavior if compiled with a buggy compiler version.
- **Recommendation:** Pin to a specific version:
  ```solidity
  pragma solidity 0.8.24;
  ```

---

### 🔵 LOW — No Event Emitted on Factory Backend Assignment

- **Category:** Best Practices
- **Location:** `CorporationDAOFactory` → constructor → Line 222
- **Description:** The factory's `backend` is set in the constructor without emitting an event. This makes it harder for off-chain monitoring to track the trusted backend address.
- **Impact:** Reduced auditability and monitoring capability.
- **Recommendation:** Emit a `BackendSet(address)` event in the constructor.

---

### 🔵 LOW — Missing `receive()` Guard or Withdrawal Mechanism

- **Category:** Denial of Service / Logic Error
- **Location:** `CorporationDAO` → `receive()` → Line 190
- **Description:** The DAO can receive ETH via `receive()`, but the only way to send ETH out is through a governance proposal targeting a recipient with a value. This is correct by design, but there's no emergency withdrawal mechanism. If governance becomes deadlocked (e.g., quorum can never be reached because tokens are lost), funds are permanently locked.
- **Impact:** Potential permanent fund loss under governance failure conditions.
- **Recommendation:** Consider an emergency withdrawal mechanism with a long timelock, or a "rage quit" pattern allowing token holders to withdraw proportional treasury shares.

---

### ⚪ INFORMATIONAL — Token Holders Must Self-Delegate to Vote

- **Category:** ERC Standard Compliance
- **Location:** `CorporationGovernanceToken` (ERC20Votes)
- **Description:** ERC20Votes requires holders to explicitly delegate (including to themselves) before their tokens count as voting power. The constructor mints tokens but does not auto-delegate. New token holders who are unfamiliar with this pattern will have zero voting power until they call `delegate(self)`.
- **Impact:** Governance participation may be lower than expected.
- **Recommendation:** Auto-delegate during initial minting in the constructor:
  ```solidity
  for (uint256 i = 0; i < initialHolders.length; i++) {
      _mint(initialHolders[i], initialBalances[i]);
      _delegate(initialHolders[i], initialHolders[i]);
  }
  ```

---

### ⚪ INFORMATIONAL — No Timelock on Proposal Execution

- **Category:** Centralization Risk
- **Location:** `CorporationDAO` (missing `GovernorTimelockControl`)
- **Description:** The DAO does not use `GovernorTimelockControl`. Proposals are executed immediately after voting concludes. This gives token holders no window to exit if a malicious proposal passes.
- **Impact:** No protection window for minority token holders against hostile proposals.
- **Recommendation:** Integrate `GovernorTimelockControl` with a minimum delay (e.g., 24-48h).

---

## Executive Summary

| Severity | Count |
|----------|-------|
| 🔴 CRITICAL | 2 |
| 🟠 HIGH | 4 |
| 🟡 MEDIUM | 4 |
| 🔵 LOW | 3 |
| ⚪ INFORMATIONAL | 2 |
| **Total** | **15** |

**Overall Risk Level: HIGH**

The contract system has a compilation error that prevents deployment (line 43 typo). Beyond that, the architecture has significant centralization risk around the `backend` address, broken ABI encoding in `proposeSimple`, and is vulnerable to flash loan governance attacks due to the 1-block voting delay combined with a fixed quorum.

**Top 3 Priorities:**
1. Fix the compilation error (`ERC20Permits` → `ERC20Permit`).
2. Fix `proposeSimple` ABI encoding (or remove the function entirely).
3. Increase voting delay and switch to percentage-based quorum to mitigate flash loan attacks.

---

## Architecture Review

- **Trust Model:** The `backend` address is a critical trusted entity that controls officer management and DAO creation. It operates as a centralized authority within a decentralized governance framework — this is a deliberate design choice for a game (SYSBREAK) but should be clearly documented.
- **Upgrade Path:** None. Contracts are immutable once deployed.
- **Admin Powers:** `backend` can add/remove officers (controls who proposes). Token owner (the DAO itself) can mint/burn tokens.
- **External Dependencies:** OpenZeppelin Governor suite, ERC20Votes, ERC20Permit.

---

## Dependency Audit

- **OpenZeppelin Version:** Assumed 5.x based on `Ownable(msg.sender)` pattern. Should be pinned in `package.json`.
- **Risk:** OZ 5.x Governor is well-audited. No known vulnerabilities in the used extensions.
- **Oracle Integrations:** None.

---

## Suggested Tooling

| Tool | Purpose |
|------|---------|
| **Slither** | Static analysis — will catch the ABI encoding issue and missing zero-checks |
| **Foundry fuzz tests** | Fuzz `proposeSimple` calldata encoding, quorum edge cases |
| **Echidna** | Invariant testing — "treasury balance should only decrease via executed proposals" |
| **Forta** | Monitor backend address activity, unusual officer changes |

---

## OWASP SC Top 10 (2026) Coverage

| Category | Covered? |
|----------|----------|
| Access Control | ⚠️ Partially — backend centralization risk |
| Logic Errors | ❌ Multiple encoding bugs found |
| Reentrancy | ✅ No external calls with state changes in custom code |
| Signature Vulnerabilities | ✅ Delegated to OZ ERC20Permit (standard) |
| Flash Loan Vectors | ❌ Vulnerable — 1-block delay + fixed quorum |
| Input Validation | ❌ Missing zero-address checks |
| Oracle Manipulation | N/A — No oracles used |
| Front-Running / MEV | ⚠️ Standard Governor MEV considerations apply |
| Integer Overflow | ✅ Solidity ≥0.8.20 default checks |
| Denial of Service | ⚠️ Unbounded constructor loop |