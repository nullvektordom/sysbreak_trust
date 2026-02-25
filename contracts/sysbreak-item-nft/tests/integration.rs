use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
use cosmwasm_std::{from_json, Addr};
use std::collections::BTreeMap;

use sysbreak_item_nft::contract::*;
use sysbreak_item_nft::error::ContractError;
use sysbreak_item_nft::msg::*;
use sysbreak_item_nft::state::Config;

fn addr(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, name: &str) -> Addr {
    deps.api.addr_make(name)
}

fn setup_contract() -> cosmwasm_std::OwnedDeps<
    cosmwasm_std::MemoryStorage,
    cosmwasm_std::testing::MockApi,
    cosmwasm_std::testing::MockQuerier,
> {
    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let minter = deps.api.addr_make("minter");
    let royalty_recipient = deps.api.addr_make("royalty");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        minter: minter.to_string(),
        royalty_bps: 500,
        royalty_recipient: royalty_recipient.to_string(),
        name: "SYSBREAK Items".to_string(),
        symbol: "SYSITM".to_string(),
    };
    let info = message_info(&owner, &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    deps
}

fn default_stats() -> BTreeMap<String, u64> {
    let mut stats = BTreeMap::new();
    stats.insert("damage".to_string(), 42);
    stats.insert("speed".to_string(), 10);
    stats
}

// ─── Instantiation ──────────────────────────────────────────────────────────

#[test]
fn test_instantiate() {
    let deps = setup_contract();
    let owner = addr(&deps, "owner");
    let minter = addr(&deps, "minter");

    let res: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(res.owner, owner);
    assert_eq!(res.minter, minter);
    assert!(!res.paused);
    assert_eq!(res.royalty_bps, 500);
}

#[test]
fn test_instantiate_invalid_royalty() {
    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let minter = deps.api.addr_make("minter");
    let royalty = deps.api.addr_make("royalty");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        minter: minter.to_string(),
        royalty_bps: 10001,
        royalty_recipient: royalty.to_string(),
        name: "Test".to_string(),
        symbol: "TST".to_string(),
    };
    let info = message_info(&owner, &[]);
    let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::InvalidRoyaltyBps { bps: 10001 });
}

// ─── Single Mint ────────────────────────────────────────────────────────────

#[test]
fn test_mint_by_minter() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");

    let info = message_info(&minter, &[]);
    let res = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "rare".to_string(),
        5,
        default_stats(),
        "dropped".to_string(),
        Some("ipfs://Qm123".to_string()),
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "mint");
    assert_eq!(res.attributes[1].value, "1");

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), "1".to_string()).unwrap()).unwrap();
    assert_eq!(nft.owner, user_a.to_string());
    assert_eq!(nft.metadata.item_type, "weapon");
    assert_eq!(nft.metadata.rarity, "rare");
    assert_eq!(nft.metadata.level, 5);
    assert_eq!(nft.token_uri, Some("ipfs://Qm123".to_string()));
}

#[test]
fn test_mint_by_non_minter_fails() {
    let mut deps = setup_contract();
    let user_a = addr(&deps, "user_a");

    let info = message_info(&user_a, &[]);
    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "crafted".to_string(),
        None,
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "minter".to_string()
        }
    );
}

// ─── Batch Mint ─────────────────────────────────────────────────────────────

#[test]
fn test_batch_mint() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");

    let info = message_info(&minter, &[]);
    let mints: Vec<MintRequest> = (0..5)
        .map(|i| MintRequest {
            to: user_a.to_string(),
            item_type: "implant".to_string(),
            rarity: "common".to_string(),
            level: i,
            stats: BTreeMap::new(),
            origin: "crafted".to_string(),
            token_uri: None,
        })
        .collect();

    let res = execute_batch_mint(deps.as_mut(), mock_env(), info, mints).unwrap();
    assert_eq!(res.attributes[1].value, "5");

    let count: NumTokensResponse =
        from_json(query_num_tokens(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(count.count, 5);
}

#[test]
fn test_batch_mint_empty_fails() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let info = message_info(&minter, &[]);
    let err = execute_batch_mint(deps.as_mut(), mock_env(), info, vec![]).unwrap_err();
    assert_eq!(err, ContractError::EmptyBatch);
}

#[test]
fn test_batch_mint_too_large_fails() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let info = message_info(&minter, &[]);

    let mints: Vec<MintRequest> = (0..51)
        .map(|_| MintRequest {
            to: user_a.to_string(),
            item_type: "weapon".to_string(),
            rarity: "common".to_string(),
            level: 1,
            stats: BTreeMap::new(),
            origin: "crafted".to_string(),
            token_uri: None,
        })
        .collect();

    let err = execute_batch_mint(deps.as_mut(), mock_env(), info, mints).unwrap_err();
    assert_eq!(err, ContractError::BatchTooLarge { max: 50 });
}

// ─── Transfer ───────────────────────────────────────────────────────────────

#[test]
fn test_transfer_nft() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    let info = message_info(&minter, &[]);
    execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap();

    let info = message_info(&user_a, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap();

    let owner: OwnerOfResponse =
        from_json(query_owner_of(deps.as_ref(), "1".to_string()).unwrap()).unwrap();
    assert_eq!(owner.owner, user_b.to_string());
}

#[test]
fn test_transfer_unauthorized_fails() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    let info = message_info(&minter, &[]);
    execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap();

    let info = message_info(&user_b, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "owner or approved".to_string()
        }
    );
}

// ─── Approvals ──────────────────────────────────────────────────────────────

#[test]
fn test_approve_and_transfer() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    let info = message_info(&minter, &[]);
    execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap();

    // USER_A approves USER_B
    let info = message_info(&user_a, &[]);
    execute_approve(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap();

    let approval: ApprovalResponse = from_json(
        query_approval(deps.as_ref(), "1".to_string(), user_b.to_string()).unwrap(),
    )
    .unwrap();
    assert!(approval.approved);

    // USER_B transfers on behalf of USER_A
    let info = message_info(&user_b, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap();

    let owner_resp: OwnerOfResponse =
        from_json(query_owner_of(deps.as_ref(), "1".to_string()).unwrap()).unwrap();
    assert_eq!(owner_resp.owner, user_b.to_string());

    // Approval cleared after transfer
    let approval: ApprovalResponse = from_json(
        query_approval(deps.as_ref(), "1".to_string(), user_b.to_string()).unwrap(),
    )
    .unwrap();
    assert!(!approval.approved);
}

#[test]
fn test_operator_approval() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    let info = message_info(&minter, &[]);
    for _ in 0..2 {
        execute_mint(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            user_a.to_string(),
            "weapon".to_string(),
            "common".to_string(),
            1,
            BTreeMap::new(),
            "dropped".to_string(),
            None,
        )
        .unwrap();
    }

    let info = message_info(&user_a, &[]);
    execute_approve_all(deps.as_mut(), mock_env(), info, user_b.to_string()).unwrap();

    let op: OperatorResponse = from_json(
        query_operator(deps.as_ref(), user_a.to_string(), user_b.to_string()).unwrap(),
    )
    .unwrap();
    assert!(op.approved);

    let info = message_info(&user_b, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap();
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "2".to_string(),
    )
    .unwrap();
}

// ─── Two-Step Minter Transfer ───────────────────────────────────────────────

#[test]
fn test_two_step_minter_transfer() {
    let mut deps = setup_contract();
    let owner = addr(&deps, "owner");
    let minter = addr(&deps, "minter");
    let new_minter = addr(&deps, "new_minter");
    let user_a = addr(&deps, "user_a");

    let info = message_info(&owner, &[]);
    execute_propose_minter(deps.as_mut(), mock_env(), info, new_minter.to_string()).unwrap();

    let pending: Option<sysbreak_item_nft::state::PendingMinterTransfer> =
        from_json(query_pending_minter(deps.as_ref()).unwrap()).unwrap();
    assert!(pending.is_some());

    let info = message_info(&new_minter, &[]);
    execute_accept_minter(deps.as_mut(), mock_env(), info).unwrap();

    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.minter, new_minter);

    // Old minter can no longer mint
    let info = message_info(&minter, &[]);
    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap_err();
    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "minter".to_string()
        }
    );
}

#[test]
fn test_non_owner_cannot_propose_minter() {
    let mut deps = setup_contract();
    let user_a = addr(&deps, "user_a");

    let info = message_info(&user_a, &[]);
    let err =
        execute_propose_minter(deps.as_mut(), mock_env(), info, user_a.to_string()).unwrap_err();
    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "owner".to_string()
        }
    );
}

#[test]
fn test_wrong_address_cannot_accept_minter() {
    let mut deps = setup_contract();
    let owner = addr(&deps, "owner");
    let new_minter = addr(&deps, "new_minter");
    let user_a = addr(&deps, "user_a");

    let info = message_info(&owner, &[]);
    execute_propose_minter(deps.as_mut(), mock_env(), info, new_minter.to_string()).unwrap();

    let info = message_info(&user_a, &[]);
    let err = execute_accept_minter(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::NotPendingMinter);
}

#[test]
fn test_cancel_minter_transfer() {
    let mut deps = setup_contract();
    let owner = addr(&deps, "owner");
    let new_minter = addr(&deps, "new_minter");

    let info = message_info(&owner, &[]);
    execute_propose_minter(deps.as_mut(), mock_env(), info, new_minter.to_string()).unwrap();

    let info = message_info(&owner, &[]);
    execute_cancel_minter_transfer(deps.as_mut(), mock_env(), info).unwrap();

    let pending: Option<sysbreak_item_nft::state::PendingMinterTransfer> =
        from_json(query_pending_minter(deps.as_ref()).unwrap()).unwrap();
    assert!(pending.is_none());
}

// ─── Pause / Unpause ────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_mint_and_transfer() {
    let mut deps = setup_contract();
    let owner = addr(&deps, "owner");
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    // Mint one before pausing
    let info = message_info(&minter, &[]);
    execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap();

    // Pause
    let info = message_info(&owner, &[]);
    execute_pause(deps.as_mut(), mock_env(), info).unwrap();

    // Mint fails
    let info = message_info(&minter, &[]);
    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        user_a.to_string(),
        "weapon".to_string(),
        "common".to_string(),
        1,
        BTreeMap::new(),
        "dropped".to_string(),
        None,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Transfer fails
    let info = message_info(&user_a, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Unpause
    let info = message_info(&owner, &[]);
    execute_unpause(deps.as_mut(), mock_env(), info).unwrap();

    // Transfer works again
    let info = message_info(&user_a, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        user_b.to_string(),
        "1".to_string(),
    )
    .unwrap();
}

#[test]
fn test_non_owner_cannot_pause() {
    let mut deps = setup_contract();
    let user_a = addr(&deps, "user_a");

    let info = message_info(&user_a, &[]);
    let err = execute_pause(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "owner".to_string()
        }
    );
}

// ─── Royalties ──────────────────────────────────────────────────────────────

#[test]
fn test_royalty_info() {
    let deps = setup_contract();
    let royalty = addr(&deps, "royalty");

    let info: RoyaltyInfoResponse =
        from_json(query_royalty_info(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(info.royalty_bps, 500);
    assert_eq!(info.royalty_recipient, royalty.to_string());
}

#[test]
fn test_update_royalty() {
    let mut deps = setup_contract();
    let owner = addr(&deps, "owner");
    let new_royalty = addr(&deps, "new_royalty");

    let info = message_info(&owner, &[]);
    execute_update_royalty(
        deps.as_mut(),
        mock_env(),
        info,
        250,
        new_royalty.to_string(),
    )
    .unwrap();

    let royalty: RoyaltyInfoResponse =
        from_json(query_royalty_info(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(royalty.royalty_bps, 250);
    assert_eq!(royalty.royalty_recipient, new_royalty.to_string());
}

// ─── Token Queries ──────────────────────────────────────────────────────────

#[test]
fn test_tokens_by_owner() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");
    let user_b = addr(&deps, "user_b");

    let info = message_info(&minter, &[]);
    for _ in 0..3 {
        execute_mint(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            user_a.to_string(),
            "weapon".to_string(),
            "common".to_string(),
            1,
            BTreeMap::new(),
            "dropped".to_string(),
            None,
        )
        .unwrap();
    }
    for _ in 0..2 {
        execute_mint(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            user_b.to_string(),
            "implant".to_string(),
            "rare".to_string(),
            3,
            BTreeMap::new(),
            "crafted".to_string(),
            None,
        )
        .unwrap();
    }

    let tokens_a: TokensResponse = from_json(
        query_tokens(deps.as_ref(), user_a.to_string(), None, None).unwrap(),
    )
    .unwrap();
    assert_eq!(tokens_a.tokens.len(), 3);

    let tokens_b: TokensResponse = from_json(
        query_tokens(deps.as_ref(), user_b.to_string(), None, None).unwrap(),
    )
    .unwrap();
    assert_eq!(tokens_b.tokens.len(), 2);
}

#[test]
fn test_sequential_token_ids() {
    let mut deps = setup_contract();
    let minter = addr(&deps, "minter");
    let user_a = addr(&deps, "user_a");

    let info = message_info(&minter, &[]);
    for i in 1..=5u64 {
        let res = execute_mint(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            user_a.to_string(),
            "weapon".to_string(),
            "common".to_string(),
            1,
            BTreeMap::new(),
            "dropped".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(res.attributes[1].value, i.to_string());
    }
}
