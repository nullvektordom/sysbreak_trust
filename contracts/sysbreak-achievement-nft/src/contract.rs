use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult,
    Timestamp, WasmMsg,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::helpers::{
    assert_minter, assert_not_paused, assert_not_soulbound, assert_owner, is_authorized,
    reject_funds,
};
use crate::msg::*;
use crate::state::*;

const CONTRACT_NAME: &str = "crates.io:sysbreak-achievement-nft";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BATCH_SIZE: u32 = 25;
const DEFAULT_QUERY_LIMIT: u32 = 30;
const MAX_QUERY_LIMIT: u32 = 100;

// ─── Instantiate ────────────────────────────────────────────────────────────

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let owner = deps.api.addr_validate(&msg.owner)?;
    let minter = deps.api.addr_validate(&msg.minter)?;

    let config = Config {
        owner,
        minter,
        paused: false,
        name: msg.name,
        symbol: msg.symbol,
    };
    CONFIG.save(deps.storage, &config)?;
    TOKEN_COUNT.save(deps.storage, &0u64)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract", CONTRACT_NAME)
        .add_attribute("owner", config.owner.as_str())
        .add_attribute("minter", config.minter.as_str()))
}

// ─── Execute: Minting ───────────────────────────────────────────────────────

pub fn execute_mint(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    to: String,
    achievement_id: String,
    category: String,
    earned_at: Timestamp,
    description: String,
    rarity: String,
    token_uri: Option<String>,
    soulbound: bool,
) -> Result<Response, ContractError> {
    assert_not_paused(deps.as_ref())?;
    assert_minter(deps.as_ref(), &info.sender)?;

    let recipient = deps.api.addr_validate(&to)?;
    let token_id = mint_single(
        deps,
        &recipient,
        achievement_id.clone(),
        category,
        earned_at,
        description,
        rarity,
        token_uri,
        soulbound,
    )?;

    Ok(Response::new()
        .add_attribute("action", "mint")
        .add_attribute("token_id", &token_id)
        .add_attribute("to", recipient.as_str())
        .add_attribute("achievement_id", &achievement_id)
        .add_attribute("soulbound", soulbound.to_string()))
}

pub fn execute_batch_mint(
    mut deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    mints: Vec<MintRequest>,
) -> Result<Response, ContractError> {
    assert_not_paused(deps.as_ref())?;
    assert_minter(deps.as_ref(), &info.sender)?;

    if mints.is_empty() {
        return Err(ContractError::EmptyBatch);
    }
    if mints.len() as u32 > MAX_BATCH_SIZE {
        return Err(ContractError::BatchTooLarge {
            max: MAX_BATCH_SIZE,
        });
    }

    // Validate all recipients upfront
    let validated: Vec<(Addr, &MintRequest)> = mints
        .iter()
        .map(|m| Ok((deps.api.addr_validate(&m.to)?, m)))
        .collect::<Result<Vec<_>, ContractError>>()?;

    let mut token_ids = Vec::with_capacity(validated.len());
    for (recipient, req) in validated {
        let token_id = mint_single(
            deps.branch(),
            &recipient,
            req.achievement_id.clone(),
            req.category.clone(),
            req.earned_at,
            req.description.clone(),
            req.rarity.clone(),
            req.token_uri.clone(),
            req.soulbound,
        )?;
        token_ids.push(token_id);
    }

    Ok(Response::new()
        .add_attribute("action", "batch_mint")
        .add_attribute("count", token_ids.len().to_string())
        .add_attribute("first_token_id", &token_ids[0])
        .add_attribute("last_token_id", &token_ids[token_ids.len() - 1]))
}

/// Atomic check-and-mint: deduplication + token creation in a single call.
fn mint_single(
    deps: DepsMut,
    recipient: &Addr,
    achievement_id: String,
    category: String,
    earned_at: Timestamp,
    description: String,
    rarity: String,
    token_uri: Option<String>,
    soulbound: bool,
) -> Result<String, ContractError> {
    // Dedup check: same achievement_id cannot be minted twice to the same address
    if ACHIEVEMENT_INDEX
        .may_load(deps.storage, (recipient, &achievement_id))?
        .is_some()
    {
        return Err(ContractError::DuplicateAchievement {
            achievement_id,
            owner: recipient.to_string(),
        });
    }

    let mut count = TOKEN_COUNT.load(deps.storage)?;
    count += 1;
    let token_id = count.to_string();

    let data = TokenData {
        owner: recipient.clone(),
        metadata: AchievementMetadata {
            achievement_id: achievement_id.clone(),
            category,
            earned_at,
            description,
            rarity,
        },
        token_uri,
        soulbound,
    };

    TOKENS.save(deps.storage, &token_id, &data)?;
    ACHIEVEMENT_INDEX.save(deps.storage, (recipient, &achievement_id), &token_id)?;
    // FIX: M-06 — maintain owner index for efficient queries
    OWNER_TOKENS.save(deps.storage, (recipient, &token_id), &true)?;
    TOKEN_COUNT.save(deps.storage, &count)?;

    Ok(token_id)
}

// ─── Execute: Transfers (soulbound enforcement) ─────────────────────────────

pub fn execute_transfer_nft(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    recipient: String,
    token_id: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_not_paused(deps.as_ref())?;
    // Soulbound check MUST happen before any authorization check
    assert_not_soulbound(deps.as_ref(), &token_id)?;

    if !is_authorized(deps.as_ref(), &token_id, &info.sender)? {
        return Err(ContractError::Unauthorized {
            role: "owner or approved".to_string(),
        });
    }

    let new_owner = deps.api.addr_validate(&recipient)?;
    let mut token = TOKENS.load(deps.storage, &token_id)?;
    let old_owner = token.owner.clone();

    // Update achievement index: remove old owner entry, add new
    ACHIEVEMENT_INDEX.remove(
        deps.storage,
        (&old_owner, &token.metadata.achievement_id),
    );
    ACHIEVEMENT_INDEX.save(
        deps.storage,
        (&new_owner, &token.metadata.achievement_id),
        &token_id,
    )?;
    // FIX: M-06 — update owner index
    OWNER_TOKENS.remove(deps.storage, (&old_owner, &token_id));
    OWNER_TOKENS.save(deps.storage, (&new_owner, &token_id), &true)?;

    token.owner = new_owner.clone();
    TOKENS.save(deps.storage, &token_id, &token)?;
    TOKEN_APPROVALS.remove(deps.storage, &token_id);

    Ok(Response::new()
        .add_attribute("action", "transfer_nft")
        .add_attribute("token_id", &token_id)
        .add_attribute("from", old_owner.as_str())
        .add_attribute("to", new_owner.as_str()))
}

pub fn execute_send_nft(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    contract: String,
    token_id: String,
    msg: Binary,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_not_paused(deps.as_ref())?;
    assert_not_soulbound(deps.as_ref(), &token_id)?;

    if !is_authorized(deps.as_ref(), &token_id, &info.sender)? {
        return Err(ContractError::Unauthorized {
            role: "owner or approved".to_string(),
        });
    }

    let contract_addr = deps.api.addr_validate(&contract)?;
    let mut token = TOKENS.load(deps.storage, &token_id)?;
    let old_owner = token.owner.clone();

    // State mutation BEFORE sub-message dispatch
    ACHIEVEMENT_INDEX.remove(
        deps.storage,
        (&old_owner, &token.metadata.achievement_id),
    );
    ACHIEVEMENT_INDEX.save(
        deps.storage,
        (&contract_addr, &token.metadata.achievement_id),
        &token_id,
    )?;
    // FIX: M-06 — update owner index
    OWNER_TOKENS.remove(deps.storage, (&old_owner, &token_id));
    OWNER_TOKENS.save(deps.storage, (&contract_addr, &token_id), &true)?;

    token.owner = contract_addr.clone();
    TOKENS.save(deps.storage, &token_id, &token)?;
    TOKEN_APPROVALS.remove(deps.storage, &token_id);

    let callback = cw721::receiver::Cw721ReceiveMsg {
        sender: info.sender.to_string(),
        token_id: token_id.clone(),
        msg,
    };
    let callback_msg = WasmMsg::Execute {
        contract_addr: contract_addr.to_string(),
        msg: to_json_binary(&callback)?,
        funds: vec![],
    };

    Ok(Response::new()
        .add_message(callback_msg)
        .add_attribute("action", "send_nft")
        .add_attribute("token_id", &token_id)
        .add_attribute("from", old_owner.as_str())
        .add_attribute("to", contract_addr.as_str()))
}

// ─── Execute: Approvals (soulbound enforcement) ─────────────────────────────

pub fn execute_approve(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    spender: String,
    token_id: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_not_paused(deps.as_ref())?;
    // Soulbound tokens cannot be approved for transfer
    assert_not_soulbound(deps.as_ref(), &token_id)?;

    let token = TOKENS.load(deps.storage, &token_id).map_err(|_| {
        ContractError::TokenNotFound {
            token_id: token_id.clone(),
        }
    })?;
    if info.sender != token.owner {
        return Err(ContractError::Unauthorized {
            role: "token owner".to_string(),
        });
    }

    let spender_addr = deps.api.addr_validate(&spender)?;
    TOKEN_APPROVALS.save(deps.storage, &token_id, &spender_addr)?;

    Ok(Response::new()
        .add_attribute("action", "approve")
        .add_attribute("token_id", &token_id)
        .add_attribute("spender", spender_addr.as_str()))
}

pub fn execute_revoke(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    token_id: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let token = TOKENS.load(deps.storage, &token_id).map_err(|_| {
        ContractError::TokenNotFound {
            token_id: token_id.clone(),
        }
    })?;
    if info.sender != token.owner {
        return Err(ContractError::Unauthorized {
            role: "token owner".to_string(),
        });
    }

    TOKEN_APPROVALS.remove(deps.storage, &token_id);

    Ok(Response::new()
        .add_attribute("action", "revoke")
        .add_attribute("token_id", &token_id))
}

pub fn execute_approve_all(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    operator: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_not_paused(deps.as_ref())?;

    let operator_addr = deps.api.addr_validate(&operator)?;
    OPERATOR_APPROVALS.save(deps.storage, (&info.sender, &operator_addr), &true)?;

    Ok(Response::new()
        .add_attribute("action", "approve_all")
        .add_attribute("owner", info.sender.as_str())
        .add_attribute("operator", operator_addr.as_str()))
}

pub fn execute_revoke_all(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    operator: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let operator_addr = deps.api.addr_validate(&operator)?;
    OPERATOR_APPROVALS.remove(deps.storage, (&info.sender, &operator_addr));

    Ok(Response::new()
        .add_attribute("action", "revoke_all")
        .add_attribute("owner", info.sender.as_str())
        .add_attribute("operator", operator_addr.as_str()))
}

// ─── Execute: Admin ─────────────────────────────────────────────────────────

pub fn execute_propose_minter(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    new_minter: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if PENDING_MINTER.may_load(deps.storage)?.is_some() {
        return Err(ContractError::MinterTransferAlreadyPending);
    }

    let proposed = deps.api.addr_validate(&new_minter)?;
    PENDING_MINTER.save(
        deps.storage,
        &PendingMinterTransfer {
            proposed_minter: proposed.clone(),
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "propose_minter")
        .add_attribute("proposed_minter", proposed.as_str()))
}

pub fn execute_accept_minter(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let pending = PENDING_MINTER
        .may_load(deps.storage)?
        .ok_or(ContractError::NoMinterTransferPending)?;

    if info.sender != pending.proposed_minter {
        return Err(ContractError::NotPendingMinter);
    }

    CONFIG.update(deps.storage, |mut c| -> StdResult<_> {
        c.minter = pending.proposed_minter.clone();
        Ok(c)
    })?;
    PENDING_MINTER.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("action", "accept_minter")
        .add_attribute("new_minter", pending.proposed_minter.as_str()))
}

pub fn execute_cancel_minter_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    assert_owner(deps.as_ref(), &info.sender)?;

    if PENDING_MINTER.may_load(deps.storage)?.is_none() {
        return Err(ContractError::NoMinterTransferPending);
    }

    PENDING_MINTER.remove(deps.storage);
    Ok(Response::new().add_attribute("action", "cancel_minter_transfer"))
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

// FIX: L-02 — burn function (minter only)
pub fn execute_burn(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    token_id: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?;
    assert_minter(deps.as_ref(), &info.sender)?;

    let token = TOKENS.load(deps.storage, &token_id).map_err(|_| {
        ContractError::TokenNotFound {
            token_id: token_id.clone(),
        }
    })?;

    ACHIEVEMENT_INDEX.remove(deps.storage, (&token.owner, &token.metadata.achievement_id));
    OWNER_TOKENS.remove(deps.storage, (&token.owner, &token_id));
    TOKENS.remove(deps.storage, &token_id);
    TOKEN_APPROVALS.remove(deps.storage, &token_id);

    let mut count = TOKEN_COUNT.load(deps.storage)?;
    count = count.saturating_sub(1);
    TOKEN_COUNT.save(deps.storage, &count)?;

    Ok(Response::new()
        .add_attribute("action", "burn")
        .add_attribute("token_id", &token_id))
}

// FIX: H-04 — two-step owner transfer
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

// FIX: I-01 — emergency fund sweep
pub fn execute_sweep_funds(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    denom: String,
    amount: cosmwasm_std::Uint128,
    recipient: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?;
    assert_owner(deps.as_ref(), &info.sender)?;
    let recipient_addr = deps.api.addr_validate(&recipient)?;
    let msg = cosmwasm_std::BankMsg::Send {
        to_address: recipient_addr.to_string(),
        amount: vec![cosmwasm_std::Coin { denom, amount }],
    };
    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "sweep_funds")
        .add_attribute("amount", amount.to_string())
        .add_attribute("recipient", recipient_addr.as_str()))
}

// ─── Queries ────────────────────────────────────────────────────────────────

pub fn query_config(deps: Deps) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    to_json_binary(&config)
}

pub fn query_nft_info(deps: Deps, token_id: String) -> StdResult<Binary> {
    let token = TOKENS.load(deps.storage, &token_id)?;
    let approval = TOKEN_APPROVALS
        .may_load(deps.storage, &token_id)?
        .map(|a| a.to_string());

    to_json_binary(&NftInfoResponse {
        token_id,
        owner: token.owner.to_string(),
        metadata: token.metadata,
        token_uri: token.token_uri,
        soulbound: token.soulbound,
        approval,
    })
}

pub fn query_owner_of(deps: Deps, token_id: String) -> StdResult<Binary> {
    let token = TOKENS.load(deps.storage, &token_id)?;
    let approval = TOKEN_APPROVALS
        .may_load(deps.storage, &token_id)?
        .map(|a| a.to_string());
    let approvals = approval.into_iter().collect();

    to_json_binary(&OwnerOfResponse {
        owner: token.owner.to_string(),
        approvals,
    })
}

// FIX: M-06 — use OWNER_TOKENS index instead of full table scan
pub fn query_tokens(
    deps: Deps,
    owner: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;
    let start = start_after
        .as_deref()
        .map(cw_storage_plus::Bound::exclusive);

    let tokens: Vec<String> = OWNER_TOKENS
        .prefix(&owner_addr)
        .keys(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .filter_map(|k| k.ok())
        .collect();

    to_json_binary(&TokensResponse { tokens })
}

pub fn query_all_tokens(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;
    let start = start_after
        .as_deref()
        .map(cw_storage_plus::Bound::exclusive);

    let tokens: Vec<String> = TOKENS
        .keys(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .filter_map(|k| k.ok())
        .collect();

    to_json_binary(&TokensResponse { tokens })
}

pub fn query_num_tokens(deps: Deps) -> StdResult<Binary> {
    let count = TOKEN_COUNT.load(deps.storage)?;
    to_json_binary(&NumTokensResponse { count })
}

pub fn query_has_achievement(
    deps: Deps,
    owner: String,
    achievement_id: String,
) -> StdResult<Binary> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let token_id = ACHIEVEMENT_INDEX.may_load(deps.storage, (&owner_addr, &achievement_id))?;

    to_json_binary(&AchievementCheckResponse {
        has_achievement: token_id.is_some(),
        token_id,
    })
}

pub fn query_achievements_by_owner(
    deps: Deps,
    owner: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;
    let start = start_after
        .as_deref()
        .map(cw_storage_plus::Bound::exclusive);

    let achievements: Vec<NftInfoResponse> = TOKENS
        .range(deps.storage, start, None, Order::Ascending)
        .filter_map(|item| {
            let (token_id, data) = item.ok()?;
            if data.owner == owner_addr {
                let approval = TOKEN_APPROVALS
                    .may_load(deps.storage, &token_id)
                    .ok()?
                    .map(|a| a.to_string());
                Some(NftInfoResponse {
                    token_id,
                    owner: data.owner.to_string(),
                    metadata: data.metadata,
                    token_uri: data.token_uri,
                    soulbound: data.soulbound,
                    approval,
                })
            } else {
                None
            }
        })
        .take(limit)
        .collect();

    to_json_binary(&AchievementsResponse { achievements })
}

pub fn query_approval(deps: Deps, token_id: String, spender: String) -> StdResult<Binary> {
    let spender_addr = deps.api.addr_validate(&spender)?;
    let approved = TOKEN_APPROVALS
        .may_load(deps.storage, &token_id)?
        .map(|a| a == spender_addr)
        .unwrap_or(false);

    to_json_binary(&ApprovalResponse { approved })
}

pub fn query_operator(deps: Deps, owner: String, operator: String) -> StdResult<Binary> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let operator_addr = deps.api.addr_validate(&operator)?;
    let approved = OPERATOR_APPROVALS
        .may_load(deps.storage, (&owner_addr, &operator_addr))?
        .unwrap_or(false);

    to_json_binary(&OperatorResponse { approved })
}

pub fn query_pending_minter(deps: Deps) -> StdResult<Binary> {
    let pending = PENDING_MINTER.may_load(deps.storage)?;
    to_json_binary(&pending)
}

// FIX: H-04
pub fn query_pending_owner(deps: Deps) -> StdResult<Binary> {
    to_json_binary(&PENDING_OWNER.may_load(deps.storage)?)
}

// ─── Migrate ────────────────────────────────────────────────────────────────

pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // FIX: M-06 — backfill OWNER_TOKENS index by scanning TOKENS
    // FIX: I-02 — migrate() should be updated for future state changes
    let all_tokens: Vec<(String, TokenData)> = TOKENS
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for (token_id, data) in &all_tokens {
        OWNER_TOKENS.save(deps.storage, (&data.owner, token_id), &true)?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("version", CONTRACT_VERSION))
}
