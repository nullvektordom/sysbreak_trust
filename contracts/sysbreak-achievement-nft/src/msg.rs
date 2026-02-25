use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Timestamp;

use crate::state::AchievementMetadata;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub minter: String,
    pub name: String,
    pub symbol: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Mint a single achievement NFT (minter only)
    Mint {
        to: String,
        achievement_id: String,
        category: String,
        earned_at: Timestamp,
        description: String,
        rarity: String,
        token_uri: Option<String>,
        soulbound: bool,
    },
    /// Batch mint up to 25 achievements (minter only)
    BatchMint {
        mints: Vec<MintRequest>,
    },
    /// Transfer an NFT — rejected if token is soulbound
    TransferNft {
        recipient: String,
        token_id: String,
    },
    /// Send an NFT to a contract — rejected if token is soulbound
    SendNft {
        contract: String,
        token_id: String,
        msg: cosmwasm_std::Binary,
    },
    /// Approve a spender for a specific token — rejected if soulbound
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
    /// Revoke operator approval
    RevokeAll {
        operator: String,
    },
    /// Step 1: propose a new minter (owner only)
    ProposeMinter {
        new_minter: String,
    },
    /// Step 2: new minter accepts the role
    AcceptMinter {},
    /// Cancel a pending minter transfer (owner only)
    CancelMinterTransfer {},
    /// Pause the contract (owner only)
    Pause {},
    /// Unpause the contract (owner only)
    Unpause {},
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
    pub achievement_id: String,
    pub category: String,
    pub earned_at: Timestamp,
    pub description: String,
    pub rarity: String,
    pub token_uri: Option<String>,
    pub soulbound: bool,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get contract configuration
    #[returns(crate::state::Config)]
    Config {},
    /// Get full token info (metadata + owner + soulbound flag)
    #[returns(NftInfoResponse)]
    NftInfo { token_id: String },
    /// Get owner of a token
    #[returns(OwnerOfResponse)]
    OwnerOf { token_id: String },
    /// Get all tokens owned by an address
    #[returns(TokensResponse)]
    Tokens {
        owner: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Get all token IDs
    #[returns(TokensResponse)]
    AllTokens {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Total minted count
    #[returns(NumTokensResponse)]
    NumTokens {},
    /// Check if a specific achievement_id has been minted to a specific address
    #[returns(AchievementCheckResponse)]
    HasAchievement {
        owner: String,
        achievement_id: String,
    },
    /// Get all achievements for a given owner
    #[returns(AchievementsResponse)]
    AchievementsByOwner {
        owner: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Check approval
    #[returns(ApprovalResponse)]
    Approval {
        token_id: String,
        spender: String,
    },
    /// Check operator approval
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
}

#[cw_serde]
pub struct NftInfoResponse {
    pub token_id: String,
    pub owner: String,
    pub metadata: AchievementMetadata,
    pub token_uri: Option<String>,
    pub soulbound: bool,
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
pub struct AchievementCheckResponse {
    pub has_achievement: bool,
    pub token_id: Option<String>,
}

#[cw_serde]
pub struct AchievementsResponse {
    pub achievements: Vec<NftInfoResponse>,
}

#[cw_serde]
pub struct ApprovalResponse {
    pub approved: bool,
}

#[cw_serde]
pub struct OperatorResponse {
    pub approved: bool,
}

#[cw_serde]
pub struct MigrateMsg {}
