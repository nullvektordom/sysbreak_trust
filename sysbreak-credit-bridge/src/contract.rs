use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::helpers::*;
use crate::msg::*;
use crate::state::*;

const CONTRACT_NAME: &str = "crates.io:sysbreak-credit-bridge";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Instantiate ────────────────────────────────────────────────────────────

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    if msg.rate_credits.is_zero() || msg.rate_tokens.is_zero() {
        return Err(ContractError::ZeroAmount);
    }
    if msg.fee_bps > 10_000 {
        return Err(ContractError::Overflow);
    }

    // FIX: L-03 — validate oracle public key on instantiation
    validate_pubkey(&msg.oracle_pubkey)?;

    let owner = deps.api.addr_validate(&msg.owner)?;
    let oracle = deps.api.addr_validate(&msg.oracle)?;
    let treasury = deps.api.addr_validate(&msg.treasury)?;

    let config = Config {
        owner,
        oracle,
        paused: false,
        denom: msg.denom,
        rate_credits: msg.rate_credits,
        rate_tokens: msg.rate_tokens,
        fee_bps: msg.fee_bps,
        treasury,
        min_deposit: msg.min_deposit,
        player_daily_limit: msg.player_daily_limit,
        global_daily_limit: msg.global_daily_limit,
        cooldown_seconds: msg.cooldown_seconds,
        min_reserve: msg.min_reserve,
        oracle_pubkey: msg.oracle_pubkey,
        chain_id: msg.chain_id,
    };

    CONFIG.save(deps.storage, &config)?;
    PEAK_BALANCE.save(deps.storage, &Uint128::zero())?;
    // FIX: M-04 — initialize Map-based global withdrawal counters
    GLOBAL_WD_COUNTER.save(deps.storage, &0u64)?;
    GLOBAL_WD_OLDEST.save(deps.storage, &0u64)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract", CONTRACT_NAME))
}

// ─── Execute: Deposit ───────────────────────────────────────────────────────

pub fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    assert_not_paused(deps.as_ref())?;

    let config = CONFIG.load(deps.storage)?;

    if info.funds.is_empty() {
        return Err(ContractError::NoFundsSent);
    }
    if info.funds.len() > 1 {
        return Err(ContractError::MultipleDenomsSent);
    }

    let sent = &info.funds[0];
    if sent.denom != config.denom {
        return Err(ContractError::WrongDenom {
            expected: config.denom,
            got: sent.denom.clone(),
        });
    }
    if sent.amount < config.min_deposit {
        return Err(ContractError::DepositBelowMinimum {
            min: config.min_deposit.to_string(),
        });
    }

    // Calculate credit amount (before fee — fee is on withdrawal, not deposit)
    let credit_amount = tokens_to_credits(sent.amount, &config)?;

    // Update peak balance tracking
    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;
    let mut peak = PEAK_BALANCE.load(deps.storage)?;
    if contract_balance > peak {
        peak = contract_balance;
        PEAK_BALANCE.save(deps.storage, &peak)?;
    }

    // Backend observes this event and credits the player's in-game account
    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender.as_str())
        .add_attribute("token_amount", sent.amount.to_string())
        .add_attribute("credit_amount", credit_amount.to_string()))
}

// ─── Execute: Withdraw ──────────────────────────────────────────────────────

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    nonce: String,
    credit_amount: Uint128,
    token_amount: Uint128,
    signature: Binary,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_not_paused(deps.as_ref())?;

    if credit_amount.is_zero() || token_amount.is_zero() {
        return Err(ContractError::ZeroAmount);
    }

    let config = CONFIG.load(deps.storage)?;
    let player = info.sender.clone();

    // FIX: M-03 — validate nonce timestamp before replay check
    validate_nonce_timestamp(&nonce, env.block.time)?;

    // 1. Nonce replay check
    if USED_NONCES
        .may_load(deps.storage, &nonce)?
        .unwrap_or(false)
    {
        return Err(ContractError::NonceAlreadyUsed {
            nonce: nonce.clone(),
        });
    }

    // 2. Verify credit ↔ token conversion matches the current rate (minus fees)
    let gross_tokens = credits_to_tokens(credit_amount, &config)?;
    let fee = calculate_fee(gross_tokens, config.fee_bps)?;
    let net_tokens = gross_tokens.checked_sub(fee).map_err(|_| ContractError::Overflow)?;

    if token_amount != net_tokens {
        return Err(ContractError::AmountMismatch {
            credits: credit_amount.to_string(),
            expected_tokens: net_tokens.to_string(),
            provided_tokens: token_amount.to_string(),
        });
    }

    // 3. Verify oracle signature
    let message_hash = build_withdrawal_message(
        &config.chain_id,
        env.contract.address.as_str(),
        &nonce,
        player.as_str(),
        credit_amount,
        token_amount,
    );

    let valid = deps
        .api
        .secp256k1_verify(&message_hash, &signature, &config.oracle_pubkey)
        .map_err(|_| ContractError::SignatureVerificationFailed)?;

    if !valid {
        return Err(ContractError::InvalidSignature);
    }

    // 4. Check player daily limit and cooldown
    check_player_limits(deps.as_ref(), &env, &player, credit_amount, &config)?;

    // 5. Check global daily limit
    check_global_limit(deps.as_ref(), &env, credit_amount, &config)?;

    // 6. Check treasury has enough balance (respecting min reserve)
    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;

    // Total outgoing: token_amount (to player) + fee (to treasury, but that's internal if treasury is external)
    // If treasury is a different address, we send fee there too
    let total_outgoing = token_amount.checked_add(fee).map_err(|_| ContractError::Overflow)?;
    let remaining = contract_balance
        .checked_sub(total_outgoing)
        .map_err(|_| ContractError::InsufficientTreasury {
            needed: total_outgoing.to_string(),
            available: contract_balance.to_string(),
            reserve_min: config.min_reserve.to_string(),
        })?;

    if remaining < config.min_reserve {
        return Err(ContractError::InsufficientTreasury {
            needed: total_outgoing.to_string(),
            available: contract_balance.to_string(),
            reserve_min: config.min_reserve.to_string(),
        });
    }

    // 7. ALL CHECKS PASSED — mutate state BEFORE dispatching bank messages

    // Mark nonce as used
    USED_NONCES.save(deps.storage, &nonce, &true)?;

    // Record player withdrawal
    let now = env.block.time;
    let record = WithdrawalRecord {
        amount_credits: credit_amount,
        timestamp: now,
    };

    let player_records = PLAYER_WITHDRAWALS
        .may_load(deps.storage, &player)?
        .unwrap_or_default();
    // Prune expired entries while we're at it
    let (mut pruned, _) = sum_rolling_window(player_records, now, 86_400);
    pruned.push(record.clone());
    PLAYER_WITHDRAWALS.save(deps.storage, &player, &pruned)?;
    PLAYER_LAST_WITHDRAWAL.save(deps.storage, &player, &now)?;

    // FIX: M-04 — record global withdrawal in Map-based storage and prune expired
    let mut counter = GLOBAL_WD_COUNTER.load(deps.storage)?;
    counter += 1;
    GLOBAL_WITHDRAWAL_RECORDS.save(deps.storage, counter, &record)?;
    GLOBAL_WD_COUNTER.save(deps.storage, &counter)?;

    // Prune a batch of old entries (up to 10 per tx for gas efficiency)
    let cutoff = now.minus_seconds(86_400);
    let mut oldest = GLOBAL_WD_OLDEST.load(deps.storage)?;
    let mut pruned = 0u32;
    while oldest < counter && pruned < 10 {
        if let Some(old_record) = GLOBAL_WITHDRAWAL_RECORDS.may_load(deps.storage, oldest)? {
            if old_record.timestamp < cutoff {
                GLOBAL_WITHDRAWAL_RECORDS.remove(deps.storage, oldest);
                oldest += 1;
                pruned += 1;
            } else {
                break;
            }
        } else {
            oldest += 1;
            pruned += 1;
        }
    }
    GLOBAL_WD_OLDEST.save(deps.storage, &oldest)?;

    // 8. Build bank messages
    let mut messages = vec![BankMsg::Send {
        to_address: player.to_string(),
        amount: vec![Coin {
            denom: config.denom.clone(),
            amount: token_amount,
        }],
    }];

    // Send fee to treasury (only if fee > 0 and treasury != contract)
    if !fee.is_zero() {
        messages.push(BankMsg::Send {
            to_address: config.treasury.to_string(),
            amount: vec![Coin {
                denom: config.denom,
                amount: fee,
            }],
        });
    }

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "withdraw")
        .add_attribute("player", player.as_str())
        .add_attribute("nonce", &nonce)
        .add_attribute("credit_amount", credit_amount.to_string())
        .add_attribute("token_amount", token_amount.to_string())
        .add_attribute("fee_amount", fee.to_string()))
}

// ─── Execute: Treasury Management ───────────────────────────────────────────

pub fn execute_fund_treasury(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    assert_owner(deps.as_ref(), &info.sender)?;

    let config = CONFIG.load(deps.storage)?;

    if info.funds.is_empty() {
        return Err(ContractError::NoFundsSent);
    }
    if info.funds.len() > 1 {
        return Err(ContractError::MultipleDenomsSent);
    }
    let sent = &info.funds[0];
    if sent.denom != config.denom {
        return Err(ContractError::WrongDenom {
            expected: config.denom,
            got: sent.denom.clone(),
        });
    }

    // Update peak balance
    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;
    let mut peak = PEAK_BALANCE.load(deps.storage)?;
    if contract_balance > peak {
        peak = contract_balance;
        PEAK_BALANCE.save(deps.storage, &peak)?;
    }

    Ok(Response::new()
        .add_attribute("action", "fund_treasury")
        .add_attribute("amount", sent.amount.to_string())
        .add_attribute("new_balance", contract_balance.to_string()))
}

pub fn execute_withdraw_treasury(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if amount.is_zero() {
        return Err(ContractError::ZeroAmount);
    }

    let config = CONFIG.load(deps.storage)?;

    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;

    let remaining = contract_balance
        .checked_sub(amount)
        .map_err(|_| ContractError::ReserveBreached {
            reserve_min: config.min_reserve.to_string(),
        })?;

    if remaining < config.min_reserve {
        return Err(ContractError::ReserveBreached {
            reserve_min: config.min_reserve.to_string(),
        });
    }

    let msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: config.denom,
            amount,
        }],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "withdraw_treasury")
        .add_attribute("amount", amount.to_string())
        .add_attribute("remaining", remaining.to_string()))
}

// ─── Execute: Oracle Transfer (two-step) ────────────────────────────────────

pub fn execute_propose_oracle(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    new_oracle: String,
    new_pubkey: Binary,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;
    // FIX: L-03 — validate public key
    validate_pubkey(&new_pubkey)?;

    if PENDING_ORACLE.may_load(deps.storage)?.is_some() {
        return Err(ContractError::OracleTransferAlreadyPending);
    }

    let proposed = deps.api.addr_validate(&new_oracle)?;
    PENDING_ORACLE.save(
        deps.storage,
        &PendingOracleTransfer {
            proposed_oracle: proposed.clone(),
            proposed_pubkey: new_pubkey,
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "propose_oracle")
        .add_attribute("proposed_oracle", proposed.as_str()))
}

pub fn execute_accept_oracle(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let pending = PENDING_ORACLE
        .may_load(deps.storage)?
        .ok_or(ContractError::NoOracleTransferPending)?;

    if info.sender != pending.proposed_oracle {
        return Err(ContractError::NotPendingOracle);
    }

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.oracle = pending.proposed_oracle.clone();
        c.oracle_pubkey = pending.proposed_pubkey.clone();
        Ok(c)
    })?;
    PENDING_ORACLE.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("action", "accept_oracle")
        .add_attribute("new_oracle", pending.proposed_oracle.as_str()))
}

pub fn execute_cancel_oracle_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if PENDING_ORACLE.may_load(deps.storage)?.is_none() {
        return Err(ContractError::NoOracleTransferPending);
    }

    PENDING_ORACLE.remove(deps.storage);
    Ok(Response::new().add_attribute("action", "cancel_oracle_transfer"))
}

// ─── Execute: Admin Config Updates ──────────────────────────────────────────

pub fn execute_update_rate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    rate_credits: Uint128,
    rate_tokens: Uint128,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if rate_credits.is_zero() || rate_tokens.is_zero() {
        return Err(ContractError::ZeroAmount);
    }

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.rate_credits = rate_credits;
        c.rate_tokens = rate_tokens;
        Ok(c)
    })?;

    Ok(Response::new()
        .add_attribute("action", "update_rate")
        .add_attribute("rate_credits", rate_credits.to_string())
        .add_attribute("rate_tokens", rate_tokens.to_string()))
}

pub fn execute_update_fee(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    fee_bps: u16,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if fee_bps > 10_000 {
        return Err(ContractError::Overflow);
    }

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.fee_bps = fee_bps;
        Ok(c)
    })?;

    Ok(Response::new()
        .add_attribute("action", "update_fee")
        .add_attribute("fee_bps", fee_bps.to_string()))
}

pub fn execute_update_limits(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    player_daily_limit: Option<Uint128>,
    global_daily_limit: Option<Uint128>,
    cooldown_seconds: Option<u64>,
    min_deposit: Option<Uint128>,
    min_reserve: Option<Uint128>,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        if let Some(v) = player_daily_limit {
            c.player_daily_limit = v;
        }
        if let Some(v) = global_daily_limit {
            c.global_daily_limit = v;
        }
        if let Some(v) = cooldown_seconds {
            c.cooldown_seconds = v;
        }
        if let Some(v) = min_deposit {
            c.min_deposit = v;
        }
        if let Some(v) = min_reserve {
            c.min_reserve = v;
        }
        Ok(c)
    })?;

    Ok(Response::new().add_attribute("action", "update_limits"))
}

pub fn execute_pause(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.paused = true;
        Ok(c)
    })?;

    Ok(Response::new().add_attribute("action", "pause"))
}

pub fn execute_unpause(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    let config = CONFIG.load(deps.storage)?;
    if !config.paused {
        return Err(ContractError::NotPaused);
    }

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.paused = false;
        Ok(c)
    })?;

    Ok(Response::new().add_attribute("action", "unpause"))
}

// ─── Two-Step Owner Transfer (H-04) ─────────────────────────────────────────

pub fn execute_propose_owner(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    new_owner: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?;
    assert_owner(deps.as_ref(), &info.sender)?;
    if PENDING_OWNER.may_load(deps.storage)?.is_some() {
        return Err(ContractError::OwnerTransferAlreadyPending);
    }
    let proposed = deps.api.addr_validate(&new_owner)?;
    PENDING_OWNER.save(
        deps.storage,
        &PendingOwnerTransfer {
            proposed_owner: proposed.clone(),
        },
    )?;
    Ok(Response::new()
        .add_attribute("action", "propose_owner")
        .add_attribute("proposed_owner", proposed.as_str()))
}

pub fn execute_accept_owner(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?;
    let pending = PENDING_OWNER
        .may_load(deps.storage)?
        .ok_or(ContractError::NoOwnerTransferPending)?;
    if info.sender != pending.proposed_owner {
        return Err(ContractError::NotPendingOwner);
    }
    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.owner = pending.proposed_owner.clone();
        Ok(c)
    })?;
    PENDING_OWNER.remove(deps.storage);
    Ok(Response::new()
        .add_attribute("action", "accept_owner")
        .add_attribute("new_owner", pending.proposed_owner.as_str()))
}

pub fn execute_cancel_owner_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?;
    assert_owner(deps.as_ref(), &info.sender)?;
    if PENDING_OWNER.may_load(deps.storage)?.is_none() {
        return Err(ContractError::NoOwnerTransferPending);
    }
    PENDING_OWNER.remove(deps.storage);
    Ok(Response::new().add_attribute("action", "cancel_owner_transfer"))
}

// ─── Queries ────────────────────────────────────────────────────────────────

pub fn query_config(deps: Deps) -> StdResult<Binary> {
    to_json_binary(&CONFIG.load(deps.storage)?)
}

pub fn query_treasury_info(deps: Deps, env: Env) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;
    let peak = PEAK_BALANCE.load(deps.storage)?;
    let available = balance.saturating_sub(config.min_reserve);

    to_json_binary(&TreasuryInfoResponse {
        balance,
        min_reserve: config.min_reserve,
        peak_balance: peak,
        available_for_withdrawal: available,
    })
}

pub fn query_player_info(deps: Deps, env: Env, address: String) -> StdResult<Binary> {
    let addr = deps.api.addr_validate(&address)?;
    let config = CONFIG.load(deps.storage)?;
    let now = env.block.time;

    let records = PLAYER_WITHDRAWALS
        .may_load(deps.storage, &addr)?
        .unwrap_or_default();
    let (_active, used) = sum_rolling_window(records, now, 86_400);
    let remaining = config.player_daily_limit.saturating_sub(used);

    let cooldown_until = PLAYER_LAST_WITHDRAWAL
        .may_load(deps.storage, &addr)?
        .map(|last| last.plus_seconds(config.cooldown_seconds).seconds());

    to_json_binary(&PlayerInfoResponse {
        withdrawals_24h: used,
        daily_limit: config.player_daily_limit,
        remaining_limit: remaining,
        cooldown_until,
    })
}

pub fn query_nonce_used(deps: Deps, nonce: String) -> StdResult<Binary> {
    let used = USED_NONCES
        .may_load(deps.storage, &nonce)?
        .unwrap_or(false);
    to_json_binary(&NonceUsedResponse { used })
}

pub fn query_convert_credits_to_tokens(deps: Deps, credit_amount: Uint128) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let gross = credits_to_tokens(credit_amount, &config)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;
    let fee = calculate_fee(gross, config.fee_bps)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;
    let net = gross.saturating_sub(fee);

    to_json_binary(&ConversionResponse {
        credit_amount,
        token_amount: net,
        fee_amount: fee,
    })
}

pub fn query_convert_tokens_to_credits(deps: Deps, token_amount: Uint128) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let credits = tokens_to_credits(token_amount, &config)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    to_json_binary(&ConversionResponse {
        credit_amount: credits,
        token_amount,
        fee_amount: Uint128::zero(),
    })
}

pub fn query_pending_oracle(deps: Deps) -> StdResult<Binary> {
    to_json_binary(&PENDING_ORACLE.may_load(deps.storage)?)
}

// FIX: H-04
pub fn query_pending_owner(deps: Deps) -> StdResult<Binary> {
    to_json_binary(&PENDING_OWNER.may_load(deps.storage)?)
}

// ─── Migrate ────────────────────────────────────────────────────────────────

pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // FIX: M-04 — migrate GLOBAL_WITHDRAWALS Vec to GLOBAL_WITHDRAWAL_RECORDS Map
    // FIX: I-02 — migrate() should be updated for future state changes
    if let Some(old_records) = GLOBAL_WITHDRAWALS.may_load(deps.storage)? {
        let mut counter = 0u64;
        for record in old_records {
            counter += 1;
            GLOBAL_WITHDRAWAL_RECORDS.save(deps.storage, counter, &record)?;
        }
        GLOBAL_WD_COUNTER.save(deps.storage, &counter)?;
        GLOBAL_WD_OLDEST.save(deps.storage, &1u64)?;
        GLOBAL_WITHDRAWALS.remove(deps.storage);
    } else {
        // Ensure counters exist
        if GLOBAL_WD_COUNTER.may_load(deps.storage)?.is_none() {
            GLOBAL_WD_COUNTER.save(deps.storage, &0u64)?;
        }
        if GLOBAL_WD_OLDEST.may_load(deps.storage)?.is_none() {
            GLOBAL_WD_OLDEST.save(deps.storage, &0u64)?;
        }
    }

    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("version", CONTRACT_VERSION))
}
