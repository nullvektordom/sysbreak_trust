use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("unauthorized: only {role} can perform this action")]
    Unauthorized { role: String },

    #[error("corporation not found: {id}")]
    CorporationNotFound { id: u64 },

    #[error("proposal not found: {id}")]
    ProposalNotFound { id: u64 },

    #[error("already a member of corporation {corp_id}")]
    AlreadyMember { corp_id: u64 },

    #[error("not a member of corporation {corp_id}")]
    NotMember { corp_id: u64 },

    #[error("corporation is invite-only")]
    InviteOnly,

    #[error("no pending invite for this address")]
    NoPendingInvite,

    #[error("corporation is full (max {max} members)")]
    CorporationFull { max: u32 },

    #[error("voting period has not ended for proposal {id}")]
    VotingNotEnded { id: u64 },

    #[error("voting period has ended for proposal {id}")]
    VotingEnded { id: u64 },

    #[error("already voted on proposal {id}")]
    AlreadyVoted { id: u64 },

    #[error("proposal {id} did not pass quorum")]
    QuorumNotReached { id: u64 },

    #[error("proposal {id} has already been executed")]
    AlreadyExecuted { id: u64 },

    #[error("proposal {id} has already been rejected or cancelled")]
    ProposalNotPending { id: u64 },

    #[error("member joined after proposal was created (flash-join protection)")]
    JoinedAfterProposal,

    #[error("treasury spend exceeds 25% of corporation treasury")]
    SpendExceedsLimit,

    #[error("dissolution requires 75% supermajority, only got {pct}%")]
    DissolutionSupermajorityNotReached { pct: u64 },

    #[error("corporation is dissolving, no new proposals allowed")]
    Dissolving,

    #[error("corporation has already been dissolved")]
    Dissolved,

    #[error("no funds sent")]
    NoFundsSent,

    #[error("must send exactly one coin denomination")]
    MultipleDenomsSent,

    #[error("wrong denomination: expected {expected}, got {got}")]
    WrongDenom { expected: String, got: String },

    #[error("insufficient funds for creation fee")]
    InsufficientCreationFee,

    #[error("insufficient proposal deposit")]
    InsufficientProposalDeposit,

    #[error("zero amount not allowed")]
    ZeroAmount,

    #[error("cannot kick the last member — use dissolution instead")]
    CannotKickLastMember,

    #[error("founder cannot leave while corporation has other members — use dissolution")]
    FounderCannotLeave,

    #[error("nothing to claim")]
    NothingToClaim,

    #[error("overflow in arithmetic operation")]
    Overflow,

    // FIX: H-01 — surplus withdrawal error
    #[error("insufficient surplus: requested {requested}, available {available}")]
    InsufficientSurplus { requested: String, available: String },

    // FIX: H-03 — prevent promotion to Founder role
    #[error("cannot promote a member to Founder role")]
    CannotPromoteToFounder,

    // FIX: H-04 — two-step owner transfer errors
    #[error("no owner transfer pending")]
    NoOwnerTransferPending,

    #[error("caller is not the pending owner")]
    NotPendingOwner,

    #[error("owner transfer already pending")]
    OwnerTransferAlreadyPending,

    // FIX: M-01 — exact payment required
    #[error("overpayment not allowed: expected {expected}, got {got}")]
    OverpaymentNotAllowed { expected: String, got: String },

    // FIX: M-02 — governance parameter validation
    #[error("invalid quorum_bps: {value} (must be 1..=10000)")]
    InvalidQuorumBps { value: u16 },

    #[error("invalid voting_period: {value} seconds (must be 3600..=2592000)")]
    InvalidVotingPeriod { value: u64 },

    // FIX: M-08 — reject unexpected funds
    #[error("unexpected funds sent with this message")]
    UnexpectedFunds,
}
