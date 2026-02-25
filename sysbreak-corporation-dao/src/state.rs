use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

// FIX: H-04 — two-step owner transfer state
#[cw_serde]
pub struct PendingOwnerTransfer {
    pub proposed_owner: Addr,
}

/// Global contract configuration
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub denom: String,
    /// Fee to create a corporation (in native tokens)
    pub creation_fee: Uint128,
    /// Deposit required to create a proposal (refunded if passed, burned if failed)
    pub proposal_deposit: Uint128,
    /// Default max members per corporation
    pub default_max_members: u32,
    /// Default quorum threshold in basis points (5100 = 51%)
    pub default_quorum_bps: u16,
    /// Default voting period in seconds (3 days = 259200)
    pub default_voting_period: u64,
}

/// A corporation (guild)
#[cw_serde]
pub struct Corporation {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub founder: Addr,
    pub join_policy: JoinPolicy,
    pub quorum_bps: u16,
    pub voting_period: u64,
    pub max_members: u32,
    pub member_count: u32,
    pub treasury_balance: Uint128,
    pub created_at: Timestamp,
    /// Once set to Dissolving, no new proposals; once Dissolved, nothing works
    pub status: CorporationStatus,
}

#[cw_serde]
pub enum JoinPolicy {
    Open,
    InviteOnly,
}

#[cw_serde]
pub enum CorporationStatus {
    Active,
    /// Dissolution vote passed — members can claim their share
    Dissolving,
    /// All funds claimed or distributed
    Dissolved,
}

#[cw_serde]
pub enum MemberRole {
    Founder,
    Officer,
    Member,
}

#[cw_serde]
pub struct MemberInfo {
    pub role: MemberRole,
    pub joined_at: Timestamp,
}

/// Proposal types
#[cw_serde]
pub enum ProposalType {
    TreasurySpend {
        recipient: Addr,
        amount: Uint128,
    },
    ChangeSettings {
        name: Option<String>,
        description: Option<String>,
        join_policy: Option<JoinPolicy>,
        quorum_bps: Option<u16>,
        voting_period: Option<u64>,
    },
    KickMember {
        member: Addr,
    },
    PromoteMember {
        member: Addr,
        new_role: MemberRole,
    },
    Dissolution,
    Custom {
        title: String,
        description: String,
    },
}

#[cw_serde]
pub enum ProposalStatus {
    /// Voting is open
    Active,
    /// Quorum reached, proposal passed
    Passed,
    /// Quorum not reached or more "no" than "yes"
    Failed,
    /// Passed and executed
    Executed,
}

#[cw_serde]
pub struct Proposal {
    pub id: u64,
    pub corp_id: u64,
    pub proposer: Addr,
    pub proposal_type: ProposalType,
    pub status: ProposalStatus,
    pub yes_votes: u32,
    pub no_votes: u32,
    pub created_at: Timestamp,
    pub voting_ends_at: Timestamp,
    /// Deposit held — refunded on pass, burned on fail
    pub deposit: Uint128,
    // FIX: H-02 — snapshot member count at proposal creation for quorum evaluation
    pub member_count_snapshot: u32,
}

pub const CONFIG: Item<Config> = Item::new("dao_config");
pub const CORP_COUNT: Item<u64> = Item::new("corp_count");
pub const PROPOSAL_COUNT: Item<u64> = Item::new("prop_count");

/// corp_id -> Corporation
pub const CORPORATIONS: Map<u64, Corporation> = Map::new("corps");

/// (corp_id, member_addr) -> MemberInfo
pub const MEMBERS: Map<(u64, &Addr), MemberInfo> = Map::new("members");

/// (corp_id, invited_addr) -> bool (pending invites for invite-only corps)
pub const INVITES: Map<(u64, &Addr), bool> = Map::new("invites");

/// proposal_id -> Proposal
pub const PROPOSALS: Map<u64, Proposal> = Map::new("proposals");

/// (proposal_id, voter_addr) -> bool (vote tracking — true=yes, false=no)
pub const VOTES: Map<(u64, &Addr), bool> = Map::new("votes");

/// (corp_id, member_addr) -> Uint128 (claimable share during dissolution)
pub const DISSOLUTION_CLAIMS: Map<(u64, &Addr), Uint128> = Map::new("diss_claims");

// FIX: H-04 — pending owner transfer storage
pub const PENDING_OWNER: Item<PendingOwnerTransfer> = Item::new("pending_owner");

// FIX: M-07 — secondary index for efficient proposal queries by corporation
/// (corp_id, proposal_id) -> () — allows prefix scan by corp_id
pub const CORP_PROPOSALS: Map<(u64, u64), ()> = Map::new("corp_props");
