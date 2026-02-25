use cosmwasm_std::{Addr, Binary, Deps, Env, MessageInfo, Timestamp, Uint128};
use sha2::{Digest, Sha256};

use crate::error::ContractError;
use crate::state::{
    Config, WithdrawalRecord, CONFIG, GLOBAL_WITHDRAWAL_RECORDS, GLOBAL_WD_COUNTER,
    GLOBAL_WD_OLDEST, NONCE_EXPIRY_WINDOW, PLAYER_LAST_WITHDRAWAL, PLAYER_WITHDRAWALS,
};

pub fn assert_owner(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if *sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
    Ok(())
}

pub fn assert_not_paused(deps: Deps) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.paused {
        return Err(ContractError::Paused);
    }
    Ok(())
}

/// Convert credit amount to gross token amount (before fees) using the stored rate.
/// credits / rate_credits * rate_tokens = tokens
/// We use: tokens = credits * rate_tokens / rate_credits (checked math)
pub fn credits_to_tokens(credits: Uint128, config: &Config) -> Result<Uint128, ContractError> {
    credits
        .checked_mul(config.rate_tokens)
        .map_err(|_| ContractError::Overflow)?
        .checked_div(config.rate_credits)
        .map_err(|_| ContractError::Overflow)
}

/// Convert token amount to credit amount using the stored rate.
/// tokens / rate_tokens * rate_credits = credits
pub fn tokens_to_credits(tokens: Uint128, config: &Config) -> Result<Uint128, ContractError> {
    tokens
        .checked_mul(config.rate_credits)
        .map_err(|_| ContractError::Overflow)?
        .checked_div(config.rate_tokens)
        .map_err(|_| ContractError::Overflow)
}

/// Calculate fee amount in tokens from a gross token amount.
/// fee = amount * fee_bps / 10_000
pub fn calculate_fee(amount: Uint128, fee_bps: u16) -> Result<Uint128, ContractError> {
    amount
        .checked_mul(Uint128::from(fee_bps as u128))
        .map_err(|_| ContractError::Overflow)?
        .checked_div(Uint128::from(10_000u128))
        .map_err(|_| ContractError::Overflow)
}

/// Build the canonical message that the oracle must sign for a withdrawal.
/// Format: "withdraw:{chain_id}:{contract_addr}:{nonce}:{player}:{credit_amount}:{token_amount}"
/// This prevents replay across chains, contracts, and nonces.
pub fn build_withdrawal_message(
    chain_id: &str,
    contract_addr: &str,
    nonce: &str,
    player: &str,
    credit_amount: Uint128,
    token_amount: Uint128,
) -> Vec<u8> {
    let msg = format!(
        "withdraw:{}:{}:{}:{}:{}:{}",
        chain_id, contract_addr, nonce, player, credit_amount, token_amount
    );
    // SHA-256 hash — secp256k1_verify expects a 32-byte message hash
    let mut hasher = Sha256::new();
    hasher.update(msg.as_bytes());
    hasher.finalize().to_vec()
}

/// Sum withdrawal amounts within a rolling 24h window, pruning expired entries.
/// Returns (pruned_records, total_in_window).
pub fn sum_rolling_window(
    records: Vec<WithdrawalRecord>,
    now: Timestamp,
    window_seconds: u64,
) -> (Vec<WithdrawalRecord>, Uint128) {
    let cutoff = now.minus_seconds(window_seconds);
    let mut total = Uint128::zero();
    let mut active: Vec<WithdrawalRecord> = Vec::new();

    for record in records {
        if record.timestamp >= cutoff {
            // Safe: individual amounts are validated Uint128, sum bounded by global limit
            total = total.saturating_add(record.amount_credits);
            active.push(record);
        }
    }

    (active, total)
}

/// Check player daily limit and cooldown. Returns the current 24h usage.
pub fn check_player_limits(
    deps: Deps,
    env: &Env,
    player: &Addr,
    credit_amount: Uint128,
    config: &Config,
) -> Result<Uint128, ContractError> {
    let now = env.block.time;

    // Cooldown check
    if let Some(last) = PLAYER_LAST_WITHDRAWAL.may_load(deps.storage, player)? {
        let cooldown_until = last.plus_seconds(config.cooldown_seconds);
        if now < cooldown_until {
            return Err(ContractError::CooldownActive {
                available_at: cooldown_until.seconds().to_string(),
            });
        }
    }

    // Rolling 24h window
    let records = PLAYER_WITHDRAWALS
        .may_load(deps.storage, player)?
        .unwrap_or_default();
    let (_active, used) = sum_rolling_window(records, now, 86_400);

    let new_total = used.checked_add(credit_amount).map_err(|_| ContractError::Overflow)?;
    if new_total > config.player_daily_limit {
        return Err(ContractError::PlayerDailyLimitExceeded {
            used: used.to_string(),
            requested: credit_amount.to_string(),
            limit: config.player_daily_limit.to_string(),
        });
    }

    Ok(used)
}

// FIX: M-04 — Map-based global limit check with pruning
/// Check global daily limit using the Map-based storage. Returns the current 24h usage.
pub fn check_global_limit(
    deps: Deps,
    env: &Env,
    credit_amount: Uint128,
    config: &Config,
) -> Result<Uint128, ContractError> {
    let now = env.block.time;
    let cutoff = now.minus_seconds(86_400);
    let oldest = GLOBAL_WD_OLDEST.may_load(deps.storage)?.unwrap_or(0);
    let counter = GLOBAL_WD_COUNTER.may_load(deps.storage)?.unwrap_or(0);

    let mut used = Uint128::zero();
    for idx in oldest..=counter {
        if let Some(record) = GLOBAL_WITHDRAWAL_RECORDS.may_load(deps.storage, idx)? {
            if record.timestamp >= cutoff {
                used = used.saturating_add(record.amount_credits);
            }
        }
    }

    let new_total = used.checked_add(credit_amount).map_err(|_| ContractError::Overflow)?;
    if new_total > config.global_daily_limit {
        return Err(ContractError::GlobalDailyLimitExceeded {
            used: used.to_string(),
            requested: credit_amount.to_string(),
            limit: config.global_daily_limit.to_string(),
        });
    }

    Ok(used)
}

// FIX: M-08 — reject unexpected funds
pub fn reject_funds(info: &MessageInfo) -> Result<(), ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::UnexpectedFunds);
    }
    Ok(())
}

// FIX: L-03 — validate oracle public key length
pub fn validate_pubkey(pubkey: &Binary) -> Result<(), ContractError> {
    let len = pubkey.len();
    if len != 33 && len != 65 {
        return Err(ContractError::InvalidPubkeyLength { length: len });
    }
    Ok(())
}

// FIX: M-03 — parse and validate timestamp-based nonce
/// Nonce format: "{unix_timestamp}:{random}"
/// Rejects nonces older than NONCE_EXPIRY_WINDOW.
pub fn validate_nonce_timestamp(nonce: &str, now: Timestamp) -> Result<(), ContractError> {
    let parts: Vec<&str> = nonce.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(ContractError::InvalidNonceFormat);
    }
    let nonce_ts: u64 = parts[0]
        .parse()
        .map_err(|_| ContractError::InvalidNonceFormat)?;
    let now_secs = now.seconds();
    if nonce_ts < now_secs.saturating_sub(NONCE_EXPIRY_WINDOW) {
        return Err(ContractError::NonceExpired {
            window: NONCE_EXPIRY_WINDOW,
        });
    }
    Ok(())
}
