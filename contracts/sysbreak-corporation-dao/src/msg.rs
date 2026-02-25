use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::state::{JoinPolicy, MemberRole};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub denom: String,
    pub creation_fee: Uint128,
    pub proposal_deposit: Uint128,
    pub default_max_members: u32,
    /// Default quorum in basis points (e.g. 5100 = 51%)
    pub default_quorum_bps: u16,
    /// Default voting period in seconds
    pub default_voting_period: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new corporation (requires creation fee in native tokens)
    CreateCorporation {
        name: String,
        description: String,
        join_policy: JoinPolicy,
    },

    /// Join an open corporation
    JoinCorporation { corp_id: u64 },

    /// Invite a player to an invite-only corporation (officer or founder only)
    InviteMember { corp_id: u64, invitee: String },

    /// Accept a pending invite
    AcceptInvite { corp_id: u64 },

    /// Leave a corporation voluntarily
    LeaveCorporation { corp_id: u64 },

    /// Donate native tokens to corporation treasury
    DonateTreasury { corp_id: u64 },

    /// Create a proposal (any member, requires deposit)
    CreateProposal {
        corp_id: u64,
        proposal_type: ProposalTypeMsg,
    },

    /// Vote on an active proposal
    Vote {
        proposal_id: u64,
        vote: bool,
    },

    /// Execute a passed proposal after voting period ends
    ExecuteProposal { proposal_id: u64 },

    /// Claim dissolution share (when corporation is dissolving)
    ClaimDissolution { corp_id: u64 },

    /// Founder can update description without a proposal
    UpdateDescription { corp_id: u64, description: String },

    // FIX: H-01 — withdraw surplus fees/deposits not tracked in any treasury
    WithdrawFees { amount: Uint128 },

    // FIX: H-04 — two-step owner transfer
    ProposeOwner { new_owner: String },
    AcceptOwner {},
    CancelOwnerTransfer {},
}

/// Message-level proposal type (uses String for addresses)
#[cw_serde]
pub enum ProposalTypeMsg {
    TreasurySpend { recipient: String, amount: Uint128 },
    ChangeSettings {
        name: Option<String>,
        description: Option<String>,
        join_policy: Option<JoinPolicy>,
        quorum_bps: Option<u16>,
        voting_period: Option<u64>,
    },
    KickMember { member: String },
    PromoteMember { member: String, new_role: MemberRole },
    Dissolution,
    Custom { title: String, description: String },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::state::Config)]
    Config {},

    #[returns(CorporationResponse)]
    Corporation { corp_id: u64 },

    #[returns(CorporationsListResponse)]
    ListCorporations {
        start_after: Option<u64>,
        limit: Option<u32>,
    },

    #[returns(MembersListResponse)]
    Members {
        corp_id: u64,
        start_after: Option<String>,
        limit: Option<u32>,
    },

    #[returns(MemberInfoResponse)]
    MemberInfo { corp_id: u64, address: String },

    #[returns(ProposalResponse)]
    Proposal { proposal_id: u64 },

    #[returns(ProposalsListResponse)]
    Proposals {
        corp_id: u64,
        start_after: Option<u64>,
        limit: Option<u32>,
    },

    #[returns(VoteStatusResponse)]
    VoteStatus { proposal_id: u64 },

    // FIX: H-04 — query pending owner transfer
    #[returns(Option<crate::state::PendingOwnerTransfer>)]
    PendingOwner {},
}

#[cw_serde]
pub struct CorporationResponse {
    pub corporation: crate::state::Corporation,
}

#[cw_serde]
pub struct CorporationsListResponse {
    pub corporations: Vec<crate::state::Corporation>,
}

#[cw_serde]
pub struct MembersListResponse {
    pub members: Vec<MemberEntry>,
}

#[cw_serde]
pub struct MemberEntry {
    pub address: String,
    pub role: MemberRole,
    pub joined_at: cosmwasm_std::Timestamp,
}

#[cw_serde]
pub struct MemberInfoResponse {
    pub is_member: bool,
    pub info: Option<crate::state::MemberInfo>,
}

#[cw_serde]
pub struct ProposalResponse {
    pub proposal: crate::state::Proposal,
}

#[cw_serde]
pub struct ProposalsListResponse {
    pub proposals: Vec<crate::state::Proposal>,
}

#[cw_serde]
pub struct VoteStatusResponse {
    pub yes_votes: u32,
    pub no_votes: u32,
    pub total_members: u32,
    pub quorum_bps: u16,
    pub quorum_reached: bool,
    pub passed: bool,
    pub voting_ended: bool,
}

#[cw_serde]
pub struct MigrateMsg {}
