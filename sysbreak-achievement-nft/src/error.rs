use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("unauthorized: only {role} can perform this action")]
    Unauthorized { role: String },

    #[error("contract is paused")]
    Paused,

    #[error("contract is not paused")]
    NotPaused,

    #[error("batch mint exceeds maximum of {max} items")]
    BatchTooLarge { max: u32 },

    #[error("batch mint list is empty")]
    EmptyBatch,

    #[error("no minter transfer pending")]
    NoMinterTransferPending,

    #[error("caller is not the pending minter")]
    NotPendingMinter,

    #[error("minter transfer already pending")]
    MinterTransferAlreadyPending,

    #[error("token is soulbound and cannot be transferred")]
    Soulbound,

    #[error("achievement {achievement_id} already minted to {owner}")]
    DuplicateAchievement {
        achievement_id: String,
        owner: String,
    },

    #[error("token not found: {token_id}")]
    TokenNotFound { token_id: String },

    // FIX: H-04 — two-step owner transfer errors
    #[error("no owner transfer pending")]
    NoOwnerTransferPending,

    #[error("caller is not the pending owner")]
    NotPendingOwner,

    #[error("owner transfer already pending")]
    OwnerTransferAlreadyPending,

    // FIX: M-08 — reject unexpected funds
    #[error("unexpected funds sent with this message")]
    UnexpectedFunds,
}
