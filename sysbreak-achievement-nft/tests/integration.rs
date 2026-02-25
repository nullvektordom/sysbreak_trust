use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi, MockQuerier};
use cosmwasm_std::{from_json, Addr, MemoryStorage, OwnedDeps, Timestamp};

use sysbreak_achievement_nft::contract::*;
use sysbreak_achievement_nft::error::ContractError;
use sysbreak_achievement_nft::msg::*;
use sysbreak_achievement_nft::state::Config;

type Deps = OwnedDeps<MemoryStorage, MockApi, MockQuerier>;

fn a(deps: &Deps, name: &str) -> Addr {
    deps.api.addr_make(name)
}

fn setup() -> Deps {
    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let minter = deps.api.addr_make("minter");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        minter: minter.to_string(),
        name: "SYSBREAK Achievements".to_string(),
        symbol: "SYSACH".to_string(),
    };
    let info = message_info(&owner, &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    deps
}

fn mint_achievement(
    deps: &mut Deps,
    to: &str,
    achievement_id: &str,
    soulbound: bool,
) -> String {
    let minter = deps.api.addr_make("minter");
    let to_addr = deps.api.addr_make(to);
    let info = message_info(&minter, &[]);
    let res = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        to_addr.to_string(),
        achievement_id.to_string(),
        "combat".to_string(),
        Timestamp::from_seconds(1700000000),
        "Test achievement".to_string(),
        "rare".to_string(),
        None,
        soulbound,
    )
    .unwrap();
    // Return the token_id from attributes
    res.attributes
        .iter()
        .find(|a| a.key == "token_id")
        .unwrap()
        .value
        .clone()
}

// ─── Instantiation ──────────────────────────────────────────────────────────

#[test]
fn test_instantiate() {
    let deps = setup();
    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.owner, a(&deps, "owner"));
    assert_eq!(config.minter, a(&deps, "minter"));
    assert!(!config.paused);
}

// ─── Minting ────────────────────────────────────────────────────────────────

#[test]
fn test_mint_soulbound() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "first_hack", true);

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), token_id).unwrap()).unwrap();
    assert_eq!(nft.metadata.achievement_id, "first_hack");
    assert_eq!(nft.metadata.category, "combat");
    assert!(nft.soulbound);
    assert_eq!(nft.owner, a(&deps, "player1").to_string());
}

#[test]
fn test_mint_non_soulbound() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "speed_run", false);

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), token_id).unwrap()).unwrap();
    assert!(!nft.soulbound);
}

#[test]
fn test_mint_non_minter_fails() {
    let mut deps = setup();
    let player = a(&deps, "player1");
    let info = message_info(&player, &[]);

    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        player.to_string(),
        "first_hack".to_string(),
        "combat".to_string(),
        Timestamp::from_seconds(1700000000),
        "desc".to_string(),
        "rare".to_string(),
        None,
        true,
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "minter".to_string()
        }
    );
}

// ─── Deduplication ──────────────────────────────────────────────────────────

#[test]
fn test_duplicate_achievement_same_owner_fails() {
    let mut deps = setup();
    mint_achievement(&mut deps, "player1", "first_hack", true);

    // Same achievement to same player should fail
    let minter = a(&deps, "minter");
    let player = a(&deps, "player1");
    let info = message_info(&minter, &[]);
    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        player.to_string(),
        "first_hack".to_string(),
        "combat".to_string(),
        Timestamp::from_seconds(1700000001),
        "duplicate".to_string(),
        "rare".to_string(),
        None,
        true,
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::DuplicateAchievement {
            achievement_id: "first_hack".to_string(),
            owner: player.to_string(),
        }
    );
}

#[test]
fn test_same_achievement_different_owners_ok() {
    let mut deps = setup();
    mint_achievement(&mut deps, "player1", "first_hack", true);
    // Different player can earn the same achievement
    mint_achievement(&mut deps, "player2", "first_hack", true);

    let count: NumTokensResponse =
        from_json(query_num_tokens(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(count.count, 2);
}

#[test]
fn test_has_achievement_query() {
    let mut deps = setup();
    mint_achievement(&mut deps, "player1", "first_hack", true);

    let check: AchievementCheckResponse = from_json(
        query_has_achievement(
            deps.as_ref(),
            a(&deps, "player1").to_string(),
            "first_hack".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(check.has_achievement);
    assert_eq!(check.token_id, Some("1".to_string()));

    // Non-existent achievement
    let check: AchievementCheckResponse = from_json(
        query_has_achievement(
            deps.as_ref(),
            a(&deps, "player1").to_string(),
            "nonexistent".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!check.has_achievement);
    assert!(check.token_id.is_none());
}

// ─── Soulbound Enforcement ──────────────────────────────────────────────────

#[test]
fn test_soulbound_transfer_rejected() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "first_hack", true);
    let player1 = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    let info = message_info(&player1, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id,
    )
    .unwrap_err();

    assert_eq!(err, ContractError::Soulbound);
}

#[test]
fn test_soulbound_send_rejected() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "first_hack", true);
    let player1 = a(&deps, "player1");
    let contract = a(&deps, "marketplace");

    let info = message_info(&player1, &[]);
    let err = execute_send_nft(
        deps.as_mut(),
        mock_env(),
        info,
        contract.to_string(),
        token_id,
        cosmwasm_std::Binary::default(),
    )
    .unwrap_err();

    assert_eq!(err, ContractError::Soulbound);
}

#[test]
fn test_soulbound_approve_rejected() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "first_hack", true);
    let player1 = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    let info = message_info(&player1, &[]);
    let err = execute_approve(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id,
    )
    .unwrap_err();

    assert_eq!(err, ContractError::Soulbound);
}

// ─── Non-Soulbound Transfers ────────────────────────────────────────────────

#[test]
fn test_non_soulbound_transfer_works() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "speed_run", false);
    let player1 = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    let info = message_info(&player1, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id.clone(),
    )
    .unwrap();

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), token_id).unwrap()).unwrap();
    assert_eq!(nft.owner, player2.to_string());

    // Achievement index updated: player2 now has it, player1 does not
    let check: AchievementCheckResponse = from_json(
        query_has_achievement(
            deps.as_ref(),
            player2.to_string(),
            "speed_run".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(check.has_achievement);

    let check: AchievementCheckResponse = from_json(
        query_has_achievement(
            deps.as_ref(),
            player1.to_string(),
            "speed_run".to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!check.has_achievement);
}

#[test]
fn test_non_soulbound_approve_and_transfer() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "speed_run", false);
    let player1 = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    // Approve player2
    let info = message_info(&player1, &[]);
    execute_approve(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id.clone(),
    )
    .unwrap();

    // player2 transfers
    let info = message_info(&player2, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id.clone(),
    )
    .unwrap();

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), token_id).unwrap()).unwrap();
    assert_eq!(nft.owner, player2.to_string());
}

#[test]
fn test_unauthorized_transfer_fails() {
    let mut deps = setup();
    let token_id = mint_achievement(&mut deps, "player1", "speed_run", false);
    let player2 = a(&deps, "player2");

    let info = message_info(&player2, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        token_id,
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "owner or approved".to_string()
        }
    );
}

// ─── Batch Mint ─────────────────────────────────────────────────────────────

#[test]
fn test_batch_mint() {
    let mut deps = setup();
    let minter = a(&deps, "minter");
    let player = a(&deps, "player1");

    let mints: Vec<MintRequest> = (0..5)
        .map(|i| MintRequest {
            to: player.to_string(),
            achievement_id: format!("ach_{}", i),
            category: "hacking".to_string(),
            earned_at: Timestamp::from_seconds(1700000000 + i as u64),
            description: format!("Achievement {}", i),
            rarity: "common".to_string(),
            token_uri: None,
            soulbound: true,
        })
        .collect();

    let info = message_info(&minter, &[]);
    let res = execute_batch_mint(deps.as_mut(), mock_env(), info, mints).unwrap();
    assert_eq!(res.attributes[1].value, "5");

    let count: NumTokensResponse =
        from_json(query_num_tokens(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(count.count, 5);
}

#[test]
fn test_batch_mint_with_duplicate_fails() {
    let mut deps = setup();
    let minter = a(&deps, "minter");
    let player = a(&deps, "player1");

    // Pre-mint one achievement
    mint_achievement(&mut deps, "player1", "ach_0", true);

    // Batch with a duplicate
    let mints = vec![
        MintRequest {
            to: player.to_string(),
            achievement_id: "ach_1".to_string(),
            category: "hacking".to_string(),
            earned_at: Timestamp::from_seconds(1700000001),
            description: "New".to_string(),
            rarity: "common".to_string(),
            token_uri: None,
            soulbound: true,
        },
        MintRequest {
            to: player.to_string(),
            achievement_id: "ach_0".to_string(), // duplicate!
            category: "hacking".to_string(),
            earned_at: Timestamp::from_seconds(1700000002),
            description: "Dup".to_string(),
            rarity: "common".to_string(),
            token_uri: None,
            soulbound: true,
        },
    ];

    let info = message_info(&minter, &[]);
    let err = execute_batch_mint(deps.as_mut(), mock_env(), info, mints).unwrap_err();
    assert!(matches!(err, ContractError::DuplicateAchievement { .. }));
}

#[test]
fn test_batch_mint_empty_fails() {
    let mut deps = setup();
    let minter = a(&deps, "minter");
    let info = message_info(&minter, &[]);
    let err = execute_batch_mint(deps.as_mut(), mock_env(), info, vec![]).unwrap_err();
    assert_eq!(err, ContractError::EmptyBatch);
}

#[test]
fn test_batch_mint_too_large_fails() {
    let mut deps = setup();
    let minter = a(&deps, "minter");
    let player = a(&deps, "player1");
    let info = message_info(&minter, &[]);

    let mints: Vec<MintRequest> = (0..26)
        .map(|i| MintRequest {
            to: player.to_string(),
            achievement_id: format!("ach_{}", i),
            category: "hacking".to_string(),
            earned_at: Timestamp::from_seconds(1700000000),
            description: "desc".to_string(),
            rarity: "common".to_string(),
            token_uri: None,
            soulbound: true,
        })
        .collect();

    let err = execute_batch_mint(deps.as_mut(), mock_env(), info, mints).unwrap_err();
    assert_eq!(err, ContractError::BatchTooLarge { max: 25 });
}

// ─── Pause ──────────────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_mint_and_transfer() {
    let mut deps = setup();
    let owner = a(&deps, "owner");
    let minter = a(&deps, "minter");
    let player = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    // Mint one non-soulbound before pausing
    mint_achievement(&mut deps, "player1", "speed_run", false);

    // Pause
    let info = message_info(&owner, &[]);
    execute_pause(deps.as_mut(), mock_env(), info).unwrap();

    // Mint fails
    let info = message_info(&minter, &[]);
    let err = execute_mint(
        deps.as_mut(),
        mock_env(),
        info,
        player.to_string(),
        "another".to_string(),
        "combat".to_string(),
        Timestamp::from_seconds(1700000000),
        "desc".to_string(),
        "rare".to_string(),
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Transfer fails
    let info = message_info(&player, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        "1".to_string(),
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Unpause
    let info = message_info(&owner, &[]);
    execute_unpause(deps.as_mut(), mock_env(), info).unwrap();

    // Transfer works again
    let info = message_info(&player, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        "1".to_string(),
    )
    .unwrap();
}

// ─── Two-Step Minter Transfer ───────────────────────────────────────────────

#[test]
fn test_minter_transfer() {
    let mut deps = setup();
    let owner = a(&deps, "owner");
    let new_minter = a(&deps, "new_minter");

    let info = message_info(&owner, &[]);
    execute_propose_minter(deps.as_mut(), mock_env(), info, new_minter.to_string()).unwrap();

    let info = message_info(&new_minter, &[]);
    execute_accept_minter(deps.as_mut(), mock_env(), info).unwrap();

    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.minter, new_minter);
}

#[test]
fn test_wrong_address_cannot_accept_minter() {
    let mut deps = setup();
    let owner = a(&deps, "owner");
    let new_minter = a(&deps, "new_minter");
    let rando = a(&deps, "rando");

    let info = message_info(&owner, &[]);
    execute_propose_minter(deps.as_mut(), mock_env(), info, new_minter.to_string()).unwrap();

    let info = message_info(&rando, &[]);
    let err = execute_accept_minter(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::NotPendingMinter);
}

// ─── Achievements By Owner Query ────────────────────────────────────────────

#[test]
fn test_achievements_by_owner() {
    let mut deps = setup();
    mint_achievement(&mut deps, "player1", "ach_a", true);
    mint_achievement(&mut deps, "player1", "ach_b", false);
    mint_achievement(&mut deps, "player2", "ach_c", true);

    let result: AchievementsResponse = from_json(
        query_achievements_by_owner(
            deps.as_ref(),
            a(&deps, "player1").to_string(),
            None,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(result.achievements.len(), 2);

    let result: AchievementsResponse = from_json(
        query_achievements_by_owner(
            deps.as_ref(),
            a(&deps, "player2").to_string(),
            None,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(result.achievements.len(), 1);
}

// ─── Operator Approval Still Works (but soulbound tokens stay put) ──────────

#[test]
fn test_operator_can_transfer_non_soulbound_only() {
    let mut deps = setup();
    mint_achievement(&mut deps, "player1", "soulbound_ach", true);
    mint_achievement(&mut deps, "player1", "tradeable_ach", false);
    let player1 = a(&deps, "player1");
    let player2 = a(&deps, "player2");

    // Grant operator
    let info = message_info(&player1, &[]);
    execute_approve_all(deps.as_mut(), mock_env(), info, player2.to_string()).unwrap();

    // Operator can't transfer soulbound
    let info = message_info(&player2, &[]);
    let err = execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        "1".to_string(),
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Soulbound);

    // Operator CAN transfer non-soulbound
    let info = message_info(&player2, &[]);
    execute_transfer_nft(
        deps.as_mut(),
        mock_env(),
        info,
        player2.to_string(),
        "2".to_string(),
    )
    .unwrap();

    let nft: NftInfoResponse =
        from_json(query_nft_info(deps.as_ref(), "2".to_string()).unwrap()).unwrap();
    assert_eq!(nft.owner, player2.to_string());
}

// ─── Sequential Token IDs ───────────────────────────────────────────────────

#[test]
fn test_sequential_token_ids() {
    let mut deps = setup();
    for i in 0..5 {
        let token_id = mint_achievement(&mut deps, "player1", &format!("ach_{}", i), true);
        assert_eq!(token_id, (i + 1).to_string());
    }
}
