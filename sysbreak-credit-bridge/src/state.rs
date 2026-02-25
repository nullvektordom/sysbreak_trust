use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    /// Backend oracle wallet that signs withdrawal authorizations
    pub oracle: Addr,
    pub paused: bool,
    /// Native token denomination (e.g. "ushido")
    pub denom: String,
    /// Credits per token micro-unit (e.g. 10_000 credits = 1_000_000 ushido means rate = 10_000 / 1_000_000)
    /// Stored as: credits_per_token_unit and tokens_per_credit_unit to avoid division
    /// Rate: `credit_amount` credits = `token_amount` ushido
    /// We store both sides of the ratio to avoid precision loss
    pub rate_credits: Uint128,
    pub rate_tokens: Uint128,
    /// Fee in basis points (e.g. 50 = 0.5%)
    pub fee_bps: u16,
    /// Fee recipient
    pub treasury: Addr,
    /// Minimum deposit in token micro-units
    pub min_deposit: Uint128,
    /// Per-player daily withdrawal limit in credits
    pub player_daily_limit: Uint128,
    /// Global daily withdrawal limit in credits
    pub global_daily_limit: Uint128,
    /// Minimum seconds between withdrawals per player
    pub cooldown_seconds: u64,
    /// Minimum reserve in token micro-units (contract refuses to go below this)
    pub min_reserve: Uint128,
    /// The oracle's secp256k1 public key (33 bytes compressed, stored as Binary)
    pub oracle_pubkey: cosmwasm_std::Binary,
    /// Chain ID included in signed payloads to prevent cross-chain replay
    pub chain_id: String,
}

#[cw_serde]
pub struct PendingOracleTransfer {
    pub proposed_oracle: Addr,
    pub proposed_pubkey: cosmwasm_std::Binary,
}

// FIX: H-04 — two-step owner transfer state
#[cw_serde]
pub struct PendingOwnerTransfer {
    pub proposed_owner: Addr,
}

/// Per-player withdrawal tracking for rolling 24h window
#[cw_serde]
pub struct WithdrawalRecord {
    pub amount_credits: Uint128,
    pub timestamp: Timestamp,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const PENDING_ORACLE: Item<PendingOracleTransfer> = Item::new("pending_oracle");

/// Nonce replay protection: nonce_string -> true
pub const USED_NONCES: Map<&str, bool> = Map::new("used_nonces");

/// Per-player withdrawal history: player_addr -> Vec<WithdrawalRecord>
/// We store recent withdrawal records for rolling window calculation
pub const PLAYER_WITHDRAWALS: Map<&Addr, Vec<WithdrawalRecord>> = Map::new("player_wd");

/// Per-player last withdrawal timestamp for cooldown
pub const PLAYER_LAST_WITHDRAWAL: Map<&Addr, Timestamp> = Map::new("player_last_wd");

/// Global withdrawal records for rolling 24h window
pub const GLOBAL_WITHDRAWALS: Item<Vec<WithdrawalRecord>> = Item::new("global_wd");

/// Peak treasury balance tracking for reserve ratio calculation
pub const PEAK_BALANCE: Item<Uint128> = Item::new("peak_balance");

// FIX: H-04 — pending owner transfer storage
pub const PENDING_OWNER: Item<PendingOwnerTransfer> = Item::new("pending_owner");

// FIX: M-04 — Map-based global withdrawals for scalability
/// Global withdrawal records: counter -> WithdrawalRecord
pub const GLOBAL_WITHDRAWAL_RECORDS: Map<u64, WithdrawalRecord> = Map::new("global_wd_map");
/// Counter for global withdrawal record IDs
pub const GLOBAL_WD_COUNTER: Item<u64> = Item::new("global_wd_counter");
/// Oldest un-pruned entry index for efficient iteration
pub const GLOBAL_WD_OLDEST: Item<u64> = Item::new("global_wd_oldest");

// FIX: M-03 — nonce expiry window (7 days)
pub const NONCE_EXPIRY_WINDOW: u64 = 604_800;
