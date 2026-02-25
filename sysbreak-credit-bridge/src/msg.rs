use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub oracle: String,
    /// secp256k1 compressed public key (33 bytes, hex or base64)
    pub oracle_pubkey: Binary,
    pub denom: String,
    /// Conversion rate: rate_credits credits = rate_tokens ushido
    /// Example: 10_000 credits = 1_000_000 ushido → rate_credits=10000, rate_tokens=1000000
    pub rate_credits: Uint128,
    pub rate_tokens: Uint128,
    /// Fee in basis points (max 10000)
    pub fee_bps: u16,
    /// Fee/treasury recipient address
    pub treasury: String,
    /// Minimum deposit in token micro-units
    pub min_deposit: Uint128,
    /// Per-player daily withdrawal limit in credits
    pub player_daily_limit: Uint128,
    /// Global daily withdrawal limit in credits
    pub global_daily_limit: Uint128,
    /// Minimum seconds between withdrawals per player
    pub cooldown_seconds: u64,
    /// Minimum reserve in token micro-units
    pub min_reserve: Uint128,
    /// Chain ID for signature replay protection
    pub chain_id: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Deposit native $SHIDO to receive in-game credits.
    /// Credits are granted off-chain by the backend after observing the event.
    Deposit {},

    /// Execute a withdrawal authorized by the oracle/backend.
    /// The oracle signs: (chain_id, contract_addr, nonce, player, credit_amount, token_amount)
    Withdraw {
        /// Unique nonce to prevent replay
        nonce: String,
        /// Credit amount being withdrawn
        credit_amount: Uint128,
        /// Token amount (ushido) to receive — must match credit_amount at current rate minus fees
        token_amount: Uint128,
        /// secp256k1 signature over SHA-256 hash of the withdrawal payload
        signature: Binary,
    },

    /// Owner deposits additional $SHIDO to fund the bridge treasury
    FundTreasury {},

    /// Owner withdraws excess treasury (cannot go below min_reserve)
    WithdrawTreasury {
        amount: Uint128,
    },

    /// Step 1: propose new oracle (owner only)
    ProposeOracle {
        new_oracle: String,
        new_pubkey: Binary,
    },
    /// Step 2: new oracle accepts
    AcceptOracle {},
    /// Cancel pending oracle transfer (owner only)
    CancelOracleTransfer {},

    /// Update conversion rate (owner only)
    UpdateRate {
        rate_credits: Uint128,
        rate_tokens: Uint128,
    },
    /// Update fee (owner only)
    UpdateFee {
        fee_bps: u16,
    },
    /// Update limits (owner only)
    UpdateLimits {
        player_daily_limit: Option<Uint128>,
        global_daily_limit: Option<Uint128>,
        cooldown_seconds: Option<u64>,
        min_deposit: Option<Uint128>,
        min_reserve: Option<Uint128>,
    },

    /// Emergency pause (owner only)
    Pause {},
    /// Unpause (owner only)
    Unpause {},

    // FIX: H-04 — two-step owner transfer
    ProposeOwner { new_owner: String },
    AcceptOwner {},
    CancelOwnerTransfer {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::state::Config)]
    Config {},

    #[returns(TreasuryInfoResponse)]
    TreasuryInfo {},

    #[returns(PlayerInfoResponse)]
    PlayerInfo { address: String },

    #[returns(NonceUsedResponse)]
    NonceUsed { nonce: String },

    #[returns(ConversionResponse)]
    ConvertCreditsToTokens { credit_amount: Uint128 },

    #[returns(ConversionResponse)]
    ConvertTokensToCredits { token_amount: Uint128 },

    #[returns(Option<crate::state::PendingOracleTransfer>)]
    PendingOracle {},

    // FIX: H-04
    #[returns(Option<crate::state::PendingOwnerTransfer>)]
    PendingOwner {},
}

#[cw_serde]
pub struct TreasuryInfoResponse {
    pub balance: Uint128,
    pub min_reserve: Uint128,
    pub peak_balance: Uint128,
    pub available_for_withdrawal: Uint128,
}

#[cw_serde]
pub struct PlayerInfoResponse {
    pub withdrawals_24h: Uint128,
    pub daily_limit: Uint128,
    pub remaining_limit: Uint128,
    pub cooldown_until: Option<u64>,
}

#[cw_serde]
pub struct NonceUsedResponse {
    pub used: bool,
}

#[cw_serde]
pub struct ConversionResponse {
    pub credit_amount: Uint128,
    pub token_amount: Uint128,
    pub fee_amount: Uint128,
}

#[cw_serde]
pub struct MigrateMsg {}
