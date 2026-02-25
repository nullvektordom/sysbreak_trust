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

    #[error("no oracle transfer pending")]
    NoOracleTransferPending,

    #[error("caller is not the pending oracle")]
    NotPendingOracle,

    #[error("oracle transfer already pending")]
    OracleTransferAlreadyPending,

    #[error("deposit amount below minimum of {min} ushido")]
    DepositBelowMinimum { min: String },

    #[error("no funds sent with deposit")]
    NoFundsSent,

    #[error("must send exactly one coin denomination")]
    MultipleDenomsSent,

    #[error("wrong denomination: expected {expected}, got {got}")]
    WrongDenom { expected: String, got: String },

    #[error("withdrawal nonce {nonce} has already been used")]
    NonceAlreadyUsed { nonce: String },

    #[error("invalid signature")]
    InvalidSignature,

    #[error("signature verification failed")]
    SignatureVerificationFailed,

    #[error("credit/token amount mismatch: expected {expected_tokens} ushido for {credits} credits, got {provided_tokens}")]
    AmountMismatch {
        credits: String,
        expected_tokens: String,
        provided_tokens: String,
    },

    #[error("withdrawal exceeds player daily limit: {used} + {requested} > {limit} credits")]
    PlayerDailyLimitExceeded {
        used: String,
        requested: String,
        limit: String,
    },

    #[error("withdrawal exceeds global daily limit: {used} + {requested} > {limit} credits")]
    GlobalDailyLimitExceeded {
        used: String,
        requested: String,
        limit: String,
    },

    #[error("withdrawal cooldown active: next withdrawal available at {available_at}")]
    CooldownActive { available_at: String },

    #[error("insufficient treasury balance: need {needed}, have {available}, reserve minimum is {reserve_min}")]
    InsufficientTreasury {
        needed: String,
        available: String,
        reserve_min: String,
    },

    #[error("treasury withdrawal would breach minimum reserve of {reserve_min}")]
    ReserveBreached { reserve_min: String },

    #[error("zero amount not allowed")]
    ZeroAmount,

    #[error("overflow in arithmetic operation")]
    Overflow,

    // FIX: H-04 — two-step owner transfer errors
    #[error("no owner transfer pending")]
    NoOwnerTransferPending,

    #[error("caller is not the pending owner")]
    NotPendingOwner,

    #[error("owner transfer already pending")]
    OwnerTransferAlreadyPending,

    // FIX: L-03 — invalid public key length
    #[error("invalid public key length: {length} bytes (expected 33 compressed or 65 uncompressed)")]
    InvalidPubkeyLength { length: usize },

    // FIX: M-03 — expired nonce
    #[error("nonce has expired (older than {window} seconds)")]
    NonceExpired { window: u64 },

    #[error("invalid nonce format: expected 'timestamp:random'")]
    InvalidNonceFormat,

    // FIX: M-08 — reject unexpected funds
    #[error("unexpected funds sent with this message")]
    UnexpectedFunds,
}
