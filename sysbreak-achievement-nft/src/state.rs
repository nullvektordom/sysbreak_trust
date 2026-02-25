use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp};
use cw_storage_plus::{Item, Map};

/// Contract-level configuration
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub minter: Addr,
    pub paused: bool,
    pub name: String,
    pub symbol: String,
}

/// Two-step minter transfer state
#[cw_serde]
pub struct PendingMinterTransfer {
    pub proposed_minter: Addr,
}

// FIX: H-04 — two-step owner transfer state
#[cw_serde]
pub struct PendingOwnerTransfer {
    pub proposed_owner: Addr,
}

/// On-chain metadata for an achievement NFT
#[cw_serde]
pub struct AchievementMetadata {
    /// Unique game-defined achievement identifier (e.g. "first_hack")
    pub achievement_id: String,
    pub category: String,
    /// When the player earned it in-game (from backend, not block time)
    pub earned_at: Timestamp,
    pub description: String,
    pub rarity: String,
}

/// Full on-chain token data
#[cw_serde]
pub struct TokenData {
    pub owner: Addr,
    pub metadata: AchievementMetadata,
    pub token_uri: Option<String>,
    /// Immutable after mint — soulbound tokens reject all transfers
    pub soulbound: bool,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKEN_COUNT: Item<u64> = Item::new("token_count");
pub const PENDING_MINTER: Item<PendingMinterTransfer> = Item::new("pending_minter");

/// token_id (string of u64) -> TokenData
pub const TOKENS: Map<&str, TokenData> = Map::new("ach_tokens");

/// token_id -> spender Addr (single approval per token, only for non-soulbound)
pub const TOKEN_APPROVALS: Map<&str, Addr> = Map::new("ach_approvals");

/// (owner, operator) -> bool
pub const OPERATOR_APPROVALS: Map<(&Addr, &Addr), bool> = Map::new("ach_operators");

/// Deduplication index: (owner_addr, achievement_id) -> token_id
/// Prevents the same achievement from being minted twice to the same address.
pub const ACHIEVEMENT_INDEX: Map<(&Addr, &str), String> = Map::new("ach_idx");

// FIX: H-04 — pending owner transfer storage
pub const PENDING_OWNER: Item<PendingOwnerTransfer> = Item::new("pending_owner");

// FIX: M-06 — secondary index for efficient owner-based token queries
/// (owner_addr, token_id) -> bool
pub const OWNER_TOKENS: Map<(&Addr, &str), bool> = Map::new("owner_tokens");
