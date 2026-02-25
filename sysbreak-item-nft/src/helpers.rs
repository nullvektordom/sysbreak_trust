use cosmwasm_std::{Addr, Deps, MessageInfo, StdResult};

use crate::error::ContractError;
use crate::state::{CONFIG, TOKEN_APPROVALS, TOKEN_OWNERS, OPERATOR_APPROVALS};

/// Verify the caller is the contract owner.
pub fn assert_owner(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if *sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
    Ok(())
}

/// Verify the caller is the authorized minter.
pub fn assert_minter(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if *sender != config.minter {
        return Err(ContractError::Unauthorized {
            role: "minter".to_string(),
        });
    }
    Ok(())
}

/// Verify the contract is not paused.
pub fn assert_not_paused(deps: Deps) -> Result<(), ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.paused {
        return Err(ContractError::Paused);
    }
    Ok(())
}

/// Check if `spender` is authorized to transfer `token_id` on behalf of the owner.
/// Returns true if spender is the owner, has token-level approval, or has operator approval.
pub fn is_authorized(
    deps: Deps,
    token_id: &str,
    spender: &Addr,
) -> StdResult<bool> {
    let owner = TOKEN_OWNERS.load(deps.storage, token_id)?;
    if *spender == owner {
        return Ok(true);
    }
    // Check token-level approval
    if let Some(approved) = TOKEN_APPROVALS.may_load(deps.storage, token_id)? {
        if approved == *spender {
            return Ok(true);
        }
    }
    // Check operator approval
    if let Some(true) = OPERATOR_APPROVALS.may_load(deps.storage, (&owner, spender))? {
        return Ok(true);
    }
    Ok(false)
}

/// Validate royalty basis points (max 10000 = 100%).
pub fn validate_royalty_bps(bps: u16) -> Result<(), ContractError> {
    if bps > 10_000 {
        return Err(ContractError::InvalidRoyaltyBps { bps });
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
