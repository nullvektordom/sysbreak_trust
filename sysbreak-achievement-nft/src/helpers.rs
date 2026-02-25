use cosmwasm_std::{Addr, Deps, MessageInfo, StdResult};

use crate::error::ContractError;
use crate::state::{CONFIG, OPERATOR_APPROVALS, TOKENS, TOKEN_APPROVALS};

pub fn assert_owner(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if *sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
    Ok(())
}

pub fn assert_minter(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if *sender != config.minter {
        return Err(ContractError::Unauthorized {
            role: "minter".to_string(),
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

/// Verify the token is not soulbound. Called on every transfer/send/approve path.
pub fn assert_not_soulbound(deps: Deps, token_id: &str) -> Result<(), ContractError> {
    let token = TOKENS.load(deps.storage, token_id).map_err(|_| {
        ContractError::TokenNotFound {
            token_id: token_id.to_string(),
        }
    })?;
    if token.soulbound {
        return Err(ContractError::Soulbound);
    }
    Ok(())
}

// FIX: M-08 — reject unexpected funds
pub fn reject_funds(info: &MessageInfo) -> Result<(), ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::UnexpectedFunds);
    }
    Ok(())
}

/// Check if `spender` is authorized to act on `token_id`.
pub fn is_authorized(deps: Deps, token_id: &str, spender: &Addr) -> StdResult<bool> {
    let token = TOKENS.load(deps.storage, token_id)?;
    if *spender == token.owner {
        return Ok(true);
    }
    if let Some(approved) = TOKEN_APPROVALS.may_load(deps.storage, token_id)? {
        if approved == *spender {
            return Ok(true);
        }
    }
    if let Some(true) = OPERATOR_APPROVALS.may_load(deps.storage, (&token.owner, spender))? {
        return Ok(true);
    }
    Ok(false)
}
