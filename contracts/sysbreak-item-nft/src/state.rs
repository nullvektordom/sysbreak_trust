use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use std::collections::BTreeMap;

/// Contract-level configuration
#[cw_serde]
pub struct Config {
    /// Contract owner — can pause, update royalties, propose minter changes
    pub owner: Addr,
    /// Authorized minter (backend wallet)
    pub minter: Addr,
    /// Whether the contract is paused (freezes minting + transfers)
    pub paused: bool,
    /// Royalty basis points (e.g., 500 = 5%)
    pub royalty_bps: u16,
    /// Royalty payment recipient
    pub royalty_recipient: Addr,
    // FIX: M-05 — store collection name and symbol
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

/// On-chain metadata for an item NFT
#[cw_serde]
pub struct ItemMetadata {
    pub item_type: String,
    pub rarity: String,
    pub level: u32,
    /// Flexible stat block — BTreeMap for deterministic serialization
    pub stats: BTreeMap<String, u64>,
    /// How this item was obtained
    pub origin: String,
}

/// Full on-chain token data (metadata + optional URI)
#[cw_serde]
pub struct TokenData {
    pub metadata: ItemMetadata,
    pub token_uri: Option<String>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const TOKEN_COUNT: Item<u64> = Item::new("token_count");
pub const PENDING_MINTER: Item<PendingMinterTransfer> = Item::new("pending_minter");

/// token_id (string of u64) -> TokenData
pub const TOKENS: Map<&str, TokenData> = Map::new("item_tokens");

/// token_id (string of u64) -> owner Addr
pub const TOKEN_OWNERS: Map<&str, Addr> = Map::new("item_owners");

/// token_id -> spender Addr (single approval per token)
pub const TOKEN_APPROVALS: Map<&str, Addr> = Map::new("item_approvals");

/// (owner, operator) -> bool (operator approvals)
pub const OPERATOR_APPROVALS: Map<(&Addr, &Addr), bool> = Map::new("item_operators");

// FIX: H-04 — pending owner transfer storage
pub const PENDING_OWNER: Item<PendingOwnerTransfer> = Item::new("pending_owner");

// FIX: M-06 — secondary index for efficient owner-based token queries
/// (owner_addr, token_id) -> bool
pub const OWNER_TOKENS: Map<(&Addr, &str), bool> = Map::new("owner_tokens");
