use cosmwasm_schema::{cw_serde, QueryResponses};
use crate::state::ItemMetadata;
use std::collections::BTreeMap;

#[cw_serde]
pub struct InstantiateMsg {
    /// Contract owner address
    pub owner: String,
    /// Authorized minter address (backend wallet)
    pub minter: String,
    /// Royalty basis points (max 10000)
    pub royalty_bps: u16,
    /// Royalty payment recipient
    pub royalty_recipient: String,
    /// Collection name
    pub name: String,
    /// Collection symbol
    pub symbol: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Mint a single item NFT (minter only)
    Mint {
        to: String,
        item_type: String,
        rarity: String,
        level: u32,
        stats: BTreeMap<String, u64>,
        origin: String,
        token_uri: Option<String>,
    },
    /// Batch mint up to 50 items (minter only)
    BatchMint {
        mints: Vec<MintRequest>,
    },
    /// Transfer an NFT to another address
    TransferNft {
        recipient: String,
        token_id: String,
    },
    /// Send an NFT to a contract with a callback message
    SendNft {
        contract: String,
        token_id: String,
        msg: cosmwasm_std::Binary,
    },
    /// Approve a spender for a specific token
    Approve {
        spender: String,
        token_id: String,
    },
    /// Revoke approval for a specific token
    Revoke {
        token_id: String,
    },
    /// Approve an operator for all tokens owned by sender
    ApproveAll {
        operator: String,
    },
    /// Revoke operator approval for all tokens
    RevokeAll {
        operator: String,
    },
    /// Step 1 of minter transfer: propose a new minter (owner only)
    ProposeMinter {
        new_minter: String,
    },
    /// Step 2 of minter transfer: new minter accepts the role
    AcceptMinter {},
    /// Cancel a pending minter transfer (owner only)
    CancelMinterTransfer {},
    /// Pause the contract — freezes minting and transfers (owner only)
    Pause {},
    /// Unpause the contract (owner only)
    Unpause {},
    /// Update royalty configuration (owner only)
    UpdateRoyalty {
        royalty_bps: u16,
        royalty_recipient: String,
    },
    // FIX: L-02 — burn function
    Burn { token_id: String },
    // FIX: H-04 — two-step owner transfer
    ProposeOwner { new_owner: String },
    AcceptOwner {},
    CancelOwnerTransfer {},
    // FIX: I-01 — emergency fund sweep
    SweepFunds { denom: String, amount: cosmwasm_std::Uint128, recipient: String },
}

#[cw_serde]
pub struct MintRequest {
    pub to: String,
    pub item_type: String,
    pub rarity: String,
    pub level: u32,
    pub stats: BTreeMap<String, u64>,
    pub origin: String,
    pub token_uri: Option<String>,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get contract configuration
    #[returns(crate::state::Config)]
    Config {},
    /// Get token metadata and owner info
    #[returns(NftInfoResponse)]
    NftInfo { token_id: String },
    /// Get the owner of a token
    #[returns(OwnerOfResponse)]
    OwnerOf { token_id: String },
    /// Get all tokens owned by an address
    #[returns(TokensResponse)]
    Tokens {
        owner: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Get all token IDs in the contract
    #[returns(TokensResponse)]
    AllTokens {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Get the total number of minted tokens
    #[returns(NumTokensResponse)]
    NumTokens {},
    /// Get royalty info for marketplace integration
    #[returns(RoyaltyInfoResponse)]
    RoyaltyInfo {},
    /// Check if a spender is approved for a token
    #[returns(ApprovalResponse)]
    Approval {
        token_id: String,
        spender: String,
    },
    /// Check if an operator is approved for all of an owner's tokens
    #[returns(OperatorResponse)]
    Operator {
        owner: String,
        operator: String,
    },
    /// Get pending minter transfer info
    #[returns(Option<crate::state::PendingMinterTransfer>)]
    PendingMinter {},

    // FIX: H-04
    #[returns(Option<crate::state::PendingOwnerTransfer>)]
    PendingOwner {},

    // FIX: M-05 — collection info query
    #[returns(CollectionInfoResponse)]
    CollectionInfo {},
}

#[cw_serde]
pub struct NftInfoResponse {
    pub token_id: String,
    pub owner: String,
    pub metadata: ItemMetadata,
    pub token_uri: Option<String>,
    pub approval: Option<String>,
}

#[cw_serde]
pub struct OwnerOfResponse {
    pub owner: String,
    pub approvals: Vec<String>,
}

#[cw_serde]
pub struct TokensResponse {
    pub tokens: Vec<String>,
}

#[cw_serde]
pub struct NumTokensResponse {
    pub count: u64,
}

#[cw_serde]
pub struct RoyaltyInfoResponse {
    pub royalty_bps: u16,
    pub royalty_recipient: String,
}

#[cw_serde]
pub struct ApprovalResponse {
    pub approved: bool,
}

#[cw_serde]
pub struct OperatorResponse {
    pub approved: bool,
}

// FIX: M-05
#[cw_serde]
pub struct CollectionInfoResponse {
    pub name: String,
    pub symbol: String,
}

#[cw_serde]
pub struct MigrateMsg {}
