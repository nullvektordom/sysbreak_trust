# CosmWasm Smart Contract Security Audit Report

**Audit Date:** 2026-02-25
**Audited Contract(s):**
- `sysbreak-item-nft` v0.1.0 (`contracts/sysbreak-item-nft/`)
- `sysbreak-achievement-nft` v0.1.0 (`contracts/sysbreak-achievement-nft/`)
- `sysbreak-credit-bridge` v0.1.0 (`contracts/sysbreak-credit-bridge/`)
- `sysbreak-corporation-dao` v0.1.0 (`contracts/sysbreak-corporation-dao/`)

**Target Chain:** Shido Network (Cosmos SDK + CometBFT + CosmWasm x/wasm)
**CosmWasm Version:** cosmwasm-std v2.2
**Rust Toolchain:** Edition 2021, MSRV 1.80
**Dependencies:** cw-storage-plus v2.0, cw2 v2.0, cw721 v0.21, thiserror v2, sha2 v0.10, schemars v0.8

---

## Executive Summary

The SYSBREAK Trust suite consists of four CosmWasm smart contracts forming the on-chain layer of a game economy: two CW-721-compatible NFT contracts (items and achievements), a native token bridge with oracle-signed withdrawals, and a guild/corporation DAO. The codebase demonstrates solid fundamentals: checked arithmetic via workspace `overflow-checks = true`, proper use of `cw2::set_contract_version`, two-step transfer patterns for minter and oracle roles, correct check-effects-interactions ordering on fund-dispatching handlers, and comprehensive error types.

However, the audit identified **16 findings** across all severity levels. The most significant issues center on the Corporation DAO contract, which has a **high-severity** design flaw where governance quorum is evaluated against execution-time membership counts rather than creation-time snapshots, enabling quorum manipulation by timing member departures. The DAO also permanently locks creation fees and failed proposal deposits with no recovery mechanism, and permits a ChangeSettings proposal to set quorum_bps/voting_period to degenerate values (0 or u64::MAX) that break governance. Across all contracts, the owner role --- the most powerful administrative role --- lacks the two-step transfer pattern that protects the minter and oracle roles.

**Recommendation:** Deploy with fixes. The HIGH findings should be addressed before mainnet deployment. The MEDIUM and LOW findings represent incremental improvements that reduce attack surface and operational risk.

**Findings Summary:**

| Severity | Count |
|----------|-------|
| Critical | 0 |
| High | 4 |
| Medium | 8 |
| Low | 4 |
| Informational | 4 |

---

## Findings

### [H-01] DAO creation fees and failed proposal deposits permanently locked

**Severity:** High
**Location:** `sysbreak-corporation-dao::contract::execute_create_corporation()` at `src/contract.rs:L106`; `execute_execute_proposal()` at `src/contract.rs:L502-L507`
**Status:** Resolved

**Description:**
When a corporation is created, the `creation_fee` is sent as `info.funds` to the contract. These tokens are accepted and validated but never forwarded to any recipient and no withdrawal mechanism exists for the contract owner. Similarly, when proposals fail (line 504), the deposit is "burned" by simply not refunding it --- the tokens remain in the contract with no way to recover them. Over time, this creates an ever-growing pool of locked native tokens.

**Attack Scenario:**
1. 100 corporations are created at 1000 ushido each = 100,000 ushido locked
2. 200 proposals fail with 500 ushido deposits each = 100,000 ushido locked
3. Total: 200,000 ushido permanently inaccessible

**Impact:**
Permanent loss of native tokens proportional to platform usage. No direct exploit, but economically wasteful and operationally problematic.

**Recommendation:**
Add an owner-callable `WithdrawFees` handler that can sweep accumulated fees, or forward creation fees and burned deposits to a treasury address at the time they're collected.

```rust
pub fn execute_withdraw_fees(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config = load_config(deps.as_ref())?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized { role: "owner".into() });
    }
    // Calculate total tracked treasury across all corps
    // Only allow withdrawing the untracked surplus
    let total_corp_treasury: Uint128 = /* sum of all corp.treasury_balance */;
    let contract_balance = deps.querier.query_balance(&env.contract.address, &config.denom)?.amount;
    let surplus = contract_balance.checked_sub(total_corp_treasury)?;
    if amount > surplus {
        return Err(ContractError::Overflow);
    }
    // ... send amount to owner
}
```

---

### [H-02] Governance quorum evaluated at execution-time membership, not creation-time

**Severity:** High
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L491-L495`
**Status:** Resolved

**Description:**
When a proposal is executed, `check_proposal_passed` is called with `corp.member_count` --- the **current** member count at execution time. If members leave between proposal creation and execution, the quorum denominator shrinks, making it easier to pass proposals. Conversely, if members join, the quorum becomes harder to reach.

**Attack Scenario:**
1. Corporation has 10 members. Quorum is 51% (need 6 total votes with majority yes).
2. A TreasurySpend proposal is created. 4 members vote yes, 1 votes no.
3. 5 members leave the corporation (non-voters).
4. Corporation now has 5 members. Quorum check: `5 * 10000 >= 5 * 5100` = `50000 >= 25500` --- quorum reached.
5. Majority check: `4 > 1` --- passed. Proposal executes with support from only 4 of the original 10 members.

**Impact:**
Governance decisions can be pushed through with minority support by coordinating member departures. A colluding group could drain 25% of the treasury per proposal with as few as 2 yes votes if enough members leave.

**Recommendation:**
Snapshot `member_count` at proposal creation time and store it on the Proposal struct. Use this snapshot for quorum calculations.

```rust
pub struct Proposal {
    // ... existing fields ...
    pub member_count_snapshot: u32,  // captured at creation time
}
```

---

### [H-03] PromoteMember proposal can create multiple Founders

**Severity:** High
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L596-L608`
**Status:** Resolved

**Description:**
The `PromoteMember` proposal type accepts any `MemberRole` including `Founder`. There is no validation preventing promotion to the Founder role. The Founder role has special privileges: `execute_update_description` (line 721) allows only Founders to update descriptions without a proposal, and `execute_leave_corporation` (line 285) prevents Founders from leaving while other members exist. Multiple Founders break the implicit assumption that each corporation has exactly one Founder.

**Attack Scenario:**
1. An officer or member is promoted to Founder via proposal.
2. The new "Founder" can now unilaterally update the corporation description.
3. Both Founders are unable to leave the corporation while any other members exist, potentially creating a deadlock.

**Impact:**
Privilege escalation and potential governance deadlock. The Founder role was designed as a singleton.

**Recommendation:**
Validate in the proposal execution that `new_role != MemberRole::Founder`:

```rust
ProposalType::PromoteMember { member, new_role } => {
    if *new_role == MemberRole::Founder {
        return Err(ContractError::CannotPromoteToFounder);
    }
    // ... rest of handler
}
```

---

### [H-04] Owner role lacks two-step transfer across all contracts

**Severity:** High
**Location:** All contracts --- `state.rs` Config struct
**Status:** Resolved

**Description:**
The `minter` role (in NFT contracts) and the `oracle` role (in credit-bridge) both implement a secure two-step propose/accept transfer pattern. However, the `owner` role --- which has the broadest privileges in every contract (pause/unpause, update rates, update limits, update royalties, withdraw treasury) --- has **no transfer mechanism at all**. If the owner key needs to be rotated (e.g., migrating to a multisig), the only option is a contract migration via the x/wasm admin, which may not be the same entity.

**Impact:**
- If the owner private key is lost, all admin functions are permanently inaccessible.
- If the owner key is compromised, there is no way to revoke access without a chain-level migration.
- The less-privileged minter/oracle roles are better protected than the most-privileged owner role.

**Recommendation:**
Implement a two-step `ProposeOwner`/`AcceptOwner` pattern for the owner role in all contracts, or integrate `cw-ownable` (already in workspace dependencies but unused).

---

### [M-01] DAO proposal/creation fee overpayment silently locked

**Severity:** Medium
**Location:** `sysbreak-corporation-dao::helpers::validate_funds()` at `src/helpers.rs:L74-L97`
**Status:** Resolved

**Description:**
`validate_funds` checks that `coin.amount >= min_amount` but the contract stores `config.proposal_deposit` (not the actual amount sent) as the refundable deposit. If a user sends 2x the required deposit, only 1x is refundable --- the excess is locked.

**Recommendation:**
Either check for exact amount (`coin.amount != expected_amount`) or track the actual amount sent.

---

### [M-02] No validation on quorum_bps and voting_period in ChangeSettings

**Severity:** Medium
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L556-L578`
**Status:** Resolved

**Description:**
A `ChangeSettings` proposal can set `quorum_bps` to 0 (any single vote passes anything) or to a value > 10000 (making it mathematically impossible to reach quorum, permanently bricking governance). Similarly, `voting_period` can be set to 0 (instant proposal creation + execution in same block) or to `u64::MAX` (proposals never expire).

**Recommendation:**
Add validation bounds: `quorum_bps` should be in range `[1, 10000]`, `voting_period` should have a sensible minimum (e.g., 3600 seconds) and maximum.

---

### [M-03] USED_NONCES storage grows unboundedly

**Severity:** Medium
**Location:** `sysbreak-credit-bridge::state` at `src/state.rs:L56`
**Status:** Resolved

**Description:**
Every withdrawal permanently stores a nonce entry in the `USED_NONCES` map. These entries are never cleaned up. Over the lifetime of the contract, this storage grows monotonically. While each entry is small (~32-64 bytes of key + 1 byte value), at high usage this represents permanent storage bloat with associated gas costs for state management.

**Recommendation:**
Consider a nonce scheme that's self-expiring, such as requiring nonces to include a timestamp component and rejecting nonces older than a window (e.g., 7 days). This allows old entries to be pruned.

---

### [M-04] GLOBAL_WITHDRAWALS vector may grow large within 24h window

**Severity:** Medium
**Location:** `sysbreak-credit-bridge::state::GLOBAL_WITHDRAWALS` at `src/state.rs:L66`
**Status:** Resolved

**Description:**
`GLOBAL_WITHDRAWALS` is stored as `Item<Vec<WithdrawalRecord>>` --- a single serialized vector. Every withdrawal within the 24h window adds an entry. If the bridge is heavily used (e.g., 1000 withdrawals/day), the entire vector must be loaded, deserialized, pruned, appended to, serialized, and saved on every withdrawal. This is O(n) per withdrawal where n is the number of withdrawals in the window.

**Recommendation:**
Use a `Map<u64, WithdrawalRecord>` with a counter, and iterate/prune during checks. Alternatively, maintain a running total with periodic checkpoints.

---

### [M-05] Item NFT InstantiateMsg accepts name/symbol but never stores them

**Severity:** Medium
**Location:** `sysbreak-item-nft::msg::InstantiateMsg` at `src/msg.rs:L6-L19`; `contract::instantiate()` at `src/contract.rs:L28-L56`
**Status:** Resolved

**Description:**
The `InstantiateMsg` includes `name` and `symbol` fields (standard CW-721 metadata) but the `Config` struct and `instantiate` handler never store these values. Any marketplace or indexer querying for collection name/symbol will get no response. The achievement-nft contract correctly stores these fields.

**Recommendation:**
Add `name` and `symbol` fields to the item-nft `Config` struct and store them in `instantiate`.

---

### [M-06] NFT query_tokens performs O(n) full table scan

**Severity:** Medium
**Location:** `sysbreak-item-nft::contract::query_tokens()` at `src/contract.rs:L478-L503`; `sysbreak-achievement-nft::contract::query_tokens()` at `src/contract.rs:L509-L535`
**Status:** Resolved

**Description:**
Both NFT contracts implement `query_tokens(owner)` by iterating over ALL tokens and filtering by owner. This is O(n) where n is the total number of tokens, not the number of tokens owned by the queried address. At scale (e.g., 100,000 minted items), querying any user's tokens scans the entire collection, likely exceeding gas limits.

**Recommendation:**
Add an owner-indexed map: `Map<(&Addr, &str), bool>` keyed by `(owner, token_id)` to enable efficient range queries by owner.

---

### [M-07] DAO proposal query scans all proposals across all corporations

**Severity:** Medium
**Location:** `sysbreak-corporation-dao::contract::query_proposals()` at `src/contract.rs:L827-L847`
**Status:** Resolved

**Description:**
`query_proposals` iterates ALL proposals in the global `PROPOSALS` map and filters by `corp_id`. As the total number of proposals grows across all corporations, this query becomes increasingly expensive. The pagination `start_after` is based on global proposal_id, not per-corporation.

**Recommendation:**
Use an `IndexedMap` or a secondary `Map<(u64, u64), ()>` keyed by `(corp_id, proposal_id)` to enable efficient per-corporation proposal queries.

---

### [M-08] Credit bridge Withdraw handler doesn't reject unexpected funds

**Severity:** Medium
**Location:** `sysbreak-credit-bridge::contract::execute_withdraw()` at `src/contract.rs:L118-L264`
**Status:** Resolved

**Description:**
The `Withdraw` handler processes an oracle-signed withdrawal but never checks `info.funds`. If a user accidentally sends native tokens along with their withdrawal call, those tokens are silently absorbed by the contract and not accounted for. Similarly, `UpdateRate`, `UpdateFee`, `UpdateLimits`, `Pause`, `Unpause`, `ProposeOracle`, `AcceptOracle`, `CancelOracleTransfer` all accept unexpected funds. The NFT contracts (`Mint`, `BatchMint`, etc.) have the same issue.

**Recommendation:**
Add a `reject_funds` helper for handlers that should not accept payment:

```rust
fn reject_funds(info: &MessageInfo) -> Result<(), ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::UnexpectedFunds);
    }
    Ok(())
}
```

---

### [L-01] Dissolution share truncation leaves remainder locked

**Severity:** Low
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L618-L621`
**Status:** Resolved

**Description:**
Dissolution share is calculated as `treasury_balance / member_count` using integer division. The remainder (`treasury_balance % member_count`) is not distributed and remains locked in the contract. For example, 100 ushido / 3 members = 33 each, 1 ushido locked forever.

**Recommendation:**
Assign the remainder to the last claimant, or to the founder.

---

### [L-02] Neither NFT contract has a burn function

**Severity:** Low
**Location:** Both NFT contracts
**Status:** Resolved

**Description:**
Once minted, tokens exist permanently. There is no `Burn` handler. If the game needs to remove items (e.g., consumed potions, revoked achievements), this is impossible on-chain.

**Recommendation:**
Add a minter-callable `Burn { token_id }` handler if the game design requires item destruction.

---

### [L-03] Oracle public key not validated on instantiate or propose

**Severity:** Low
**Location:** `sysbreak-credit-bridge::contract::instantiate()` at `src/contract.rs:L17-L61`; `execute_propose_oracle()` at `src/contract.rs:L356-L381`
**Status:** Resolved

**Description:**
The `oracle_pubkey` binary is stored as-is without checking that it's a valid 33-byte compressed secp256k1 public key. An incorrectly sized or malformed key would cause all `secp256k1_verify` calls to fail, effectively bricking withdrawals until the oracle is replaced.

**Recommendation:**
Validate pubkey length (33 bytes for compressed, 65 for uncompressed) in both `instantiate` and `execute_propose_oracle`.

---

### [L-04] Kicked member's votes remain counted on other active proposals

**Severity:** Low
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L583-L593`
**Status:** Resolved

**Description:**
When a member is kicked via a `KickMember` proposal, their votes on other active proposals remain in the VOTES map and are still counted toward yes/no tallies. The member_count is decremented but the votes persist. This can create situations where a kicked member's vote contributes to passing or failing a subsequent proposal, despite them no longer being a member.

**Recommendation:**
This is an accepted trade-off in many DAO designs (vote finality). If it's not desired, consider tracking a "kicked" flag and excluding their votes at execution time.

---

### [I-01] No emergency withdrawal / fund sweeper in NFT contracts

**Severity:** Informational
**Location:** Both NFT contracts
**Status:** Resolved

**Description:**
If tokens are accidentally sent to the NFT contracts (via `BankMsg::Send` from another contract, or IBC transfer), they are permanently locked. There is no admin function to recover accidentally sent funds.

---

### [I-02] Migrate functions are version-update-only no-ops

**Severity:** Informational
**Location:** All contracts, `migrate()` functions
**Status:** Resolved

**Description:**
All `migrate()` functions only call `set_contract_version` and return. They perform no state migration. This is appropriate for v0.1.0 but should be noted: if the state schema changes in a future version, the migrate function must be updated to transform storage before deploying the migration.

---

### [I-03] DonateTreasury allows non-member donations

**Severity:** Informational
**Location:** `sysbreak-corporation-dao::contract::execute_donate_treasury()` at `src/contract.rs:L307-L333`
**Status:** Resolved

**Description:**
Any address can donate to any active corporation's treasury, not just members. This is likely by design (public funding) but could be surprising. A non-member could inflate the treasury balance for economic manipulation (e.g., to increase the 25% max-spend ceiling).

---

### [I-04] ExecuteProposal callable by any address

**Severity:** Informational
**Location:** `sysbreak-corporation-dao::contract::execute_execute_proposal()` at `src/contract.rs:L472-L477`
**Status:** Resolved

**Description:**
The `_info` parameter is unused --- anyone can trigger execution of a passed proposal. This is common in DAO designs (permissionless execution after quorum) but should be documented as intentional.

---

## Trust Model & Privileged Roles

| Role | Contract | Capabilities | Compromise Impact |
|------|----------|-------------|-------------------|
| **Owner** | All 4 contracts | Pause/unpause, update config (rates, fees, limits, royalties), withdraw treasury (bridge), propose minter/oracle changes | **CRITICAL** --- Full admin control. Can pause all operations, drain bridge treasury (above min_reserve), change conversion rates, change fees to 100%. No two-step transfer. |
| **Minter** | item-nft, achievement-nft | Mint arbitrary tokens to any address | **HIGH** --- Can mint unlimited NFTs. Mitigated by two-step transfer. |
| **Oracle** | credit-bridge | Sign withdrawal authorizations | **HIGH** --- Can authorize arbitrary withdrawals up to daily limits. Mitigated by daily limits, per-player limits, min_reserve, and two-step transfer. |
| **x/wasm Admin** | All contracts | Migrate to any code, change admin | **CRITICAL** --- Can replace contract code entirely. Separate from contract-level owner. |
| **Corporation Founder** | corporation-dao | Update description without proposal, cannot leave while others exist | **LOW** --- Limited unilateral power; major actions require proposals. |

## State Schema Review

**Storage Key Uniqueness:** All contracts use distinct prefixes. No collisions detected:
- item-nft: `config`, `token_count`, `pending_minter`, `item_tokens`, `item_owners`, `item_approvals`, `item_operators`
- achievement-nft: `config`, `token_count`, `pending_minter`, `ach_tokens`, `ach_approvals`, `ach_operators`, `ach_idx`
- credit-bridge: `config`, `pending_oracle`, `used_nonces`, `player_wd`, `player_last_wd`, `global_wd`, `peak_balance`
- corporation-dao: `dao_config`, `corp_count`, `prop_count`, `corps`, `members`, `invites`, `proposals`, `votes`, `diss_claims`

**Note:** The item-nft and achievement-nft both use `"config"` and `"pending_minter"` as storage keys. If these contracts were ever combined into a single contract, keys would collide. As separate contracts, this is not an issue.

**Unbounded Growth Concerns:**
- `USED_NONCES` (credit-bridge): Grows permanently with each withdrawal. No cleanup.
- `GLOBAL_WITHDRAWALS` (credit-bridge): Vec loaded/saved atomically; expensive at high throughput.
- `PLAYER_WITHDRAWALS` (credit-bridge): Per-player vec; self-pruning via rolling window but could spike.
- `TOKENS` / `TOKEN_OWNERS` (NFTs): Normal growth, bounded by minting rate.
- `PROPOSALS` / `VOTES` (DAO): Normal growth, bounded by deposit cost.

## Cosmos-Specific Assessment

**Chain Governance Risk:** Shido chain governance can modify x/wasm parameters (max gas, upload permissions). Contracts are well within normal size and complexity bounds. No unusual Stargate queries or module interactions.

**IBC Token Handling:** The credit-bridge validates denom strictly (`sent.denom != config.denom`). IBC-wrapped tokens with `ibc/{hash}` denoms would be correctly rejected. No IBC-specific attack surface.

**Gas Limit Concerns:** The O(n) queries (query_tokens, query_proposals, query_achievements_by_owner) are the primary gas risk. Minting batch sizes are capped (50 items, 25 achievements). The rolling window vector load/save pattern in credit-bridge is O(k) where k is withdrawals in 24h, which could approach gas limits under heavy usage.

**Validator MEV:** Deposits to the credit-bridge emit events that trigger off-chain credit grants. A validator could front-run a rate change by depositing at the old rate. The impact is limited because credits are granted off-chain by the backend (which observes the token amount, not the credit calculation). Withdrawals require oracle signatures, preventing MEV exploitation on the withdrawal side.

**Block Time:** The credit-bridge cooldown uses `env.block.time` (seconds). Shido's ~5-6 second block time means cooldowns have 5-6 second granularity. This is acceptable for the configured use cases.

**overflow-checks = true:** The workspace Cargo.toml enables overflow checks in release mode. This means raw arithmetic (`+`, `-`, `*`) will panic on overflow rather than wrapping. Combined with the checked math used in critical paths (credit-bridge), arithmetic safety is well-handled.

## Inter-Contract Interaction Review

The four contracts are independent --- none reference or call each other directly. The item-nft and achievement-nft dispatch CW-721 `ReceiveNft` callbacks via `WasmMsg::Execute` (not SubMsg), meaning receiver failures fully revert the send. This is standard CW-721 behavior and not a vulnerability.

The credit-bridge dispatches `BankMsg::Send` for withdrawals and fee transfers. These are non-contract calls (native sends) and cannot be grief-blocked by the recipient.

The DAO dispatches `BankMsg::Send` for treasury spends, deposit refunds, and dissolution claims. Same analysis applies.

## Test Coverage Assessment

No test files were found in the repository (`tests/` directories, `#[cfg(test)]` modules). This is a **critical gap**.

**Recommended test cases:**

1. **Credit Bridge:**
   - Signature verification with valid/invalid/malformed signatures
   - Nonce replay rejection
   - Player daily limit at boundary (exactly at limit, 1 over limit)
   - Global daily limit at boundary
   - Cooldown enforcement (withdraw, then immediately attempt again)
   - Rate change + withdrawal interaction
   - Treasury withdrawal respecting min_reserve
   - Zero amount edge cases

2. **Corporation DAO:**
   - Flash-join vote rejection (join after proposal creation)
   - Quorum threshold at boundary values
   - Dissolution supermajority at boundary (exactly 75%, just under)
   - TreasurySpend at exactly 25% limit
   - Member departure effect on active proposal quorum
   - Multiple Founder promotion (should be blocked after fix)
   - ChangeSettings with edge-case quorum_bps/voting_period values

3. **NFT Contracts:**
   - Soulbound transfer rejection (all paths: TransferNft, SendNft, Approve)
   - Operator approval: approve all, then transfer via operator
   - Achievement dedup: same achievement_id to same address
   - Achievement index update on transfer
   - Batch mint at max size, over max size, empty batch
   - Pause: all paused operations rejected, unpause resumes

4. **Fuzz Targets:**
   - Credit bridge: arbitrary credit/token amounts with various rates
   - DAO: random member join/leave/vote sequences
   - NFT: random approval/transfer/revoke sequences

---

## Disclaimer

> This audit is a point-in-time review and does not guarantee the absence of all vulnerabilities. It should be used as one component of a comprehensive security strategy that includes formal verification, property-based testing, bug bounties, on-chain monitoring, and incident response planning. CosmWasm contracts can be migrated by the x/wasm admin --- the security of the deployed contract depends on the security of the admin key.
