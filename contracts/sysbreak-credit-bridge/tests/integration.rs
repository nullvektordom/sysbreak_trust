use cosmwasm_std::testing::{
    message_info, mock_dependencies, mock_dependencies_with_balance, mock_env, MockApi,
    MockQuerier,
};
use cosmwasm_std::{from_json, Addr, Binary, Coin, MemoryStorage, OwnedDeps, Uint128};
use k256::ecdsa::{signature::hazmat::PrehashSigner, Signature, SigningKey, VerifyingKey};
#[allow(unused_imports)]
use k256::elliptic_curve::sec1::ToEncodedPoint;
use sha2::{Digest, Sha256};

use sysbreak_credit_bridge::contract::*;
use sysbreak_credit_bridge::error::ContractError;
use sysbreak_credit_bridge::msg::*;
use sysbreak_credit_bridge::state::Config;

type TestDeps = OwnedDeps<MemoryStorage, MockApi, MockQuerier>;

fn a(deps: &TestDeps, name: &str) -> Addr {
    deps.api.addr_make(name)
}

/// Generate a secp256k1 keypair for testing
fn gen_keypair() -> (SigningKey, VerifyingKey) {
    let bytes: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
        0x1d, 0x1e, 0x1f, 0x20,
    ];
    let sk = SigningKey::from_bytes((&bytes).into()).unwrap();
    let vk = *sk.verifying_key();
    (sk, vk)
}

fn pubkey_bytes(vk: &VerifyingKey) -> Vec<u8> {
    vk.to_encoded_point(true).as_bytes().to_vec()
}

/// Sign a withdrawal message using the test signing key
fn sign_withdrawal(
    sk: &SigningKey,
    chain_id: &str,
    contract_addr: &str,
    nonce: &str,
    player: &str,
    credit_amount: Uint128,
    token_amount: Uint128,
) -> Binary {
    let msg = format!(
        "withdraw:{}:{}:{}:{}:{}:{}",
        chain_id, contract_addr, nonce, player, credit_amount, token_amount
    );
    let mut hasher = Sha256::new();
    hasher.update(msg.as_bytes());
    let hash = hasher.finalize();

    let (sig, _recid): (Signature, _) = sk.sign_prehash(&hash).unwrap();
    Binary::from(sig.to_bytes().to_vec())
}

const DENOM: &str = "ushido";
const CHAIN_ID: &str = "shido-testnet-1";

/// mock_env() uses block time 1_571_797_419. Nonces must be "{timestamp}:{random}".
fn ts_nonce(label: &str) -> String {
    format!("1571797419:{}", label)
}

// Rate: 10_000 credits = 1_000_000 ushido (i.e. 100 ushido per credit)
const RATE_CREDITS: u128 = 10_000;
const RATE_TOKENS: u128 = 1_000_000;

fn setup() -> (TestDeps, SigningKey) {
    let (sk, vk) = gen_keypair();
    let pk_bytes = pubkey_bytes(&vk);

    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let oracle = deps.api.addr_make("oracle");
    let treasury = deps.api.addr_make("treasury");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        oracle: oracle.to_string(),
        oracle_pubkey: Binary::from(pk_bytes),
        denom: DENOM.to_string(),
        rate_credits: Uint128::from(RATE_CREDITS),
        rate_tokens: Uint128::from(RATE_TOKENS),
        fee_bps: 50, // 0.5%
        treasury: treasury.to_string(),
        min_deposit: Uint128::from(100_000u128), // 0.1 SHIDO
        player_daily_limit: Uint128::from(100_000u128), // 100k credits
        global_daily_limit: Uint128::from(10_000_000u128), // 10M credits
        cooldown_seconds: 3600, // 1 hour
        min_reserve: Uint128::from(1_000_000u128), // 1 SHIDO
        chain_id: CHAIN_ID.to_string(),
    };

    let info = message_info(&owner, &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    (deps, sk)
}

fn setup_with_funded_treasury() -> (TestDeps, SigningKey, String) {
    let (sk, vk) = gen_keypair();
    let pk_bytes = pubkey_bytes(&vk);

    let mut deps = mock_dependencies_with_balance(&[Coin::new(100_000_000u128, DENOM)]);

    let owner = deps.api.addr_make("owner");
    let oracle = deps.api.addr_make("oracle");
    let treasury = deps.api.addr_make("treasury");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        oracle: oracle.to_string(),
        oracle_pubkey: Binary::from(pk_bytes),
        denom: DENOM.to_string(),
        rate_credits: Uint128::from(RATE_CREDITS),
        rate_tokens: Uint128::from(RATE_TOKENS),
        fee_bps: 50,
        treasury: treasury.to_string(),
        min_deposit: Uint128::from(100_000u128),
        player_daily_limit: Uint128::from(100_000u128),
        global_daily_limit: Uint128::from(10_000_000u128),
        cooldown_seconds: 3600,
        min_reserve: Uint128::from(1_000_000u128),
        chain_id: CHAIN_ID.to_string(),
    };

    let info = message_info(&owner, &[]);
    let env = mock_env();
    let contract_addr = env.contract.address.to_string();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, sk, contract_addr)
}

// ─── Instantiation ──────────────────────────────────────────────────────────

#[test]
fn test_instantiate() {
    let (deps, _sk) = setup();
    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.owner, a(&deps, "owner"));
    assert_eq!(config.oracle, a(&deps, "oracle"));
    assert!(!config.paused);
    assert_eq!(config.denom, DENOM);
    assert_eq!(config.rate_credits, Uint128::from(RATE_CREDITS));
    assert_eq!(config.fee_bps, 50);
}

#[test]
fn test_instantiate_zero_rate_fails() {
    let (_sk, vk) = gen_keypair();
    let pk_bytes = pubkey_bytes(&vk);

    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let oracle = deps.api.addr_make("oracle");
    let treasury = deps.api.addr_make("treasury");

    let msg = InstantiateMsg {
        owner: owner.to_string(),
        oracle: oracle.to_string(),
        oracle_pubkey: Binary::from(pk_bytes),
        denom: DENOM.to_string(),
        rate_credits: Uint128::zero(),
        rate_tokens: Uint128::from(RATE_TOKENS),
        fee_bps: 50,
        treasury: treasury.to_string(),
        min_deposit: Uint128::from(100_000u128),
        player_daily_limit: Uint128::from(100_000u128),
        global_daily_limit: Uint128::from(10_000_000u128),
        cooldown_seconds: 3600,
        min_reserve: Uint128::from(1_000_000u128),
        chain_id: CHAIN_ID.to_string(),
    };

    let info = message_info(&owner, &[]);
    let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::ZeroAmount);
}

// ─── Deposit ────────────────────────────────────────────────────────────────

#[test]
fn test_deposit() {
    let (mut deps, _sk) = setup();
    let player = a(&deps, "player1");

    let info = message_info(&player, &[Coin::new(1_000_000u128, DENOM)]);
    let res = execute_deposit(deps.as_mut(), mock_env(), info).unwrap();

    assert_eq!(res.attributes[0].value, "deposit");
    // 1_000_000 ushido * 10_000 / 1_000_000 = 10_000 credits
    assert_eq!(res.attributes[2].value, "1000000"); // token_amount
    assert_eq!(res.attributes[3].value, "10000"); // credit_amount
}

#[test]
fn test_deposit_below_minimum_fails() {
    let (mut deps, _sk) = setup();
    let player = a(&deps, "player1");

    let info = message_info(&player, &[Coin::new(50_000u128, DENOM)]); // below 100k min
    let err = execute_deposit(deps.as_mut(), mock_env(), info).unwrap_err();
    assert!(matches!(err, ContractError::DepositBelowMinimum { .. }));
}

#[test]
fn test_deposit_wrong_denom_fails() {
    let (mut deps, _sk) = setup();
    let player = a(&deps, "player1");

    let info = message_info(&player, &[Coin::new(1_000_000u128, "uatom")]);
    let err = execute_deposit(deps.as_mut(), mock_env(), info).unwrap_err();
    assert!(matches!(err, ContractError::WrongDenom { .. }));
}

#[test]
fn test_deposit_no_funds_fails() {
    let (mut deps, _sk) = setup();
    let player = a(&deps, "player1");

    let info = message_info(&player, &[]);
    let err = execute_deposit(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::NoFundsSent);
}

#[test]
fn test_deposit_paused_fails() {
    let (mut deps, _sk) = setup();
    let owner = a(&deps, "owner");
    let player = a(&deps, "player1");

    let info = message_info(&owner, &[]);
    execute_pause(deps.as_mut(), mock_env(), info).unwrap();

    let info = message_info(&player, &[Coin::new(1_000_000u128, DENOM)]);
    let err = execute_deposit(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::Paused);
}

// ─── Withdrawal ─────────────────────────────────────────────────────────────

#[test]
fn test_withdraw_valid() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    // 10_000 credits = 1_000_000 ushido gross, fee = 5_000 (0.5%), net = 995_000
    let credit_amount = Uint128::from(10_000u128);
    let token_amount = Uint128::from(995_000u128);
    let nonce = ts_nonce("001");

    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &nonce,
        player.as_str(),
        credit_amount,
        token_amount,
    );

    let info = message_info(&player, &[]);
    let res = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        nonce.clone(),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap();

    assert_eq!(res.attributes[0].value, "withdraw");
    assert_eq!(res.attributes[3].value, "10000"); // credit_amount
    assert_eq!(res.attributes[4].value, "995000"); // token_amount
    assert_eq!(res.attributes[5].value, "5000"); // fee
    assert_eq!(res.messages.len(), 2); // player payment + fee payment
}

#[test]
fn test_withdraw_nonce_replay_fails() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    let credit_amount = Uint128::from(10_000u128);
    let token_amount = Uint128::from(995_000u128);
    let nonce = ts_nonce("001");

    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &nonce,
        player.as_str(),
        credit_amount,
        token_amount,
    );

    let info = message_info(&player, &[]);
    execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        nonce.clone(),
        credit_amount,
        token_amount,
        sig.clone(),
    )
    .unwrap();

    // Replay same nonce
    let mut env2 = mock_env();
    env2.block.time = env2.block.time.plus_seconds(3601); // past cooldown
    let err = execute_withdraw(
        deps.as_mut(),
        env2,
        info,
        nonce.clone(),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::NonceAlreadyUsed { .. }));
}

#[test]
fn test_withdraw_bad_signature_fails() {
    let (mut deps, _sk, _contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    let credit_amount = Uint128::from(10_000u128);
    let token_amount = Uint128::from(995_000u128);

    // Use garbage signature
    let bad_sig = Binary::from(vec![0u8; 64]);

    let info = message_info(&player, &[]);
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("bad"),
        credit_amount,
        token_amount,
        bad_sig,
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ContractError::InvalidSignature | ContractError::SignatureVerificationFailed
    ));
}

#[test]
fn test_withdraw_amount_mismatch_fails() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    let credit_amount = Uint128::from(10_000u128);
    let wrong_token_amount = Uint128::from(999_999u128); // wrong amount

    // Sign with wrong amount — signature will be valid but contract recalculates
    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("mismatch"),
        player.as_str(),
        credit_amount,
        wrong_token_amount,
    );

    let info = message_info(&player, &[]);
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("mismatch"),
        credit_amount,
        wrong_token_amount,
        sig,
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::AmountMismatch { .. }));
}

#[test]
fn test_withdraw_cooldown_enforced() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    let credit_amount = Uint128::from(1_000u128);
    let token_amount = Uint128::from(99_500u128);

    // First withdrawal
    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("1"),
        player.as_str(),
        credit_amount,
        token_amount,
    );
    let info = message_info(&player, &[]);
    execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        ts_nonce("1"),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap();

    // Try again immediately — should fail with cooldown
    let sig2 = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("2"),
        player.as_str(),
        credit_amount,
        token_amount,
    );
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        ts_nonce("2"),
        credit_amount,
        token_amount,
        sig2.clone(),
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::CooldownActive { .. }));

    // After cooldown period it should work
    let mut env_later = mock_env();
    env_later.block.time = env_later.block.time.plus_seconds(3601);
    execute_withdraw(
        deps.as_mut(),
        env_later,
        info,
        ts_nonce("2"),
        credit_amount,
        token_amount,
        sig2,
    )
    .unwrap();
}

#[test]
fn test_withdraw_player_daily_limit() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    // Player daily limit is 100_000 credits. Try to withdraw 100_001
    let credit_amount = Uint128::from(100_001u128);
    let gross_tokens = Uint128::from(100_001u128)
        .checked_mul(Uint128::from(RATE_TOKENS))
        .unwrap()
        .checked_div(Uint128::from(RATE_CREDITS))
        .unwrap();
    let fee = gross_tokens
        .checked_mul(Uint128::from(50u128))
        .unwrap()
        .checked_div(Uint128::from(10_000u128))
        .unwrap();
    let token_amount = gross_tokens.checked_sub(fee).unwrap();

    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("limit"),
        player.as_str(),
        credit_amount,
        token_amount,
    );

    let info = message_info(&player, &[]);
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("limit"),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::PlayerDailyLimitExceeded { .. }));
}

#[test]
fn test_withdraw_zero_amount_fails() {
    let (mut deps, _sk, _contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    let info = message_info(&player, &[]);
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("zero"),
        Uint128::zero(),
        Uint128::zero(),
        Binary::from(vec![0u8; 64]),
    )
    .unwrap_err();

    assert_eq!(err, ContractError::ZeroAmount);
}

// ─── Nonce Query ────────────────────────────────────────────────────────────

#[test]
fn test_nonce_used_query() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    // Before use
    let res: NonceUsedResponse =
        from_json(query_nonce_used(deps.as_ref(), ts_nonce("q")).unwrap()).unwrap();
    assert!(!res.used);

    // Use it
    let credit_amount = Uint128::from(1_000u128);
    let token_amount = Uint128::from(99_500u128);
    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("q"),
        player.as_str(),
        credit_amount,
        token_amount,
    );
    let info = message_info(&player, &[]);
    execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("q"),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap();

    // After use
    let res: NonceUsedResponse =
        from_json(query_nonce_used(deps.as_ref(), ts_nonce("q")).unwrap()).unwrap();
    assert!(res.used);
}

// ─── Conversion Queries ─────────────────────────────────────────────────────

#[test]
fn test_conversion_credits_to_tokens() {
    let (deps, _sk) = setup();

    let res: ConversionResponse = from_json(
        query_convert_credits_to_tokens(deps.as_ref(), Uint128::from(10_000u128)).unwrap(),
    )
    .unwrap();

    // 10_000 credits * 1_000_000 / 10_000 = 1_000_000 gross
    // fee = 1_000_000 * 50 / 10_000 = 5_000
    // net = 995_000
    assert_eq!(res.credit_amount, Uint128::from(10_000u128));
    assert_eq!(res.token_amount, Uint128::from(995_000u128));
    assert_eq!(res.fee_amount, Uint128::from(5_000u128));
}

#[test]
fn test_conversion_tokens_to_credits() {
    let (deps, _sk) = setup();

    let res: ConversionResponse = from_json(
        query_convert_tokens_to_credits(deps.as_ref(), Uint128::from(1_000_000u128)).unwrap(),
    )
    .unwrap();

    // 1_000_000 ushido * 10_000 / 1_000_000 = 10_000 credits (no fee on deposit direction)
    assert_eq!(res.credit_amount, Uint128::from(10_000u128));
    assert_eq!(res.fee_amount, Uint128::zero());
}

// ─── Arithmetic Edge Cases ──────────────────────────────────────────────────

#[test]
fn test_conversion_small_amount() {
    let (deps, _sk) = setup();

    // 1 credit = 100 ushido gross, fee = 0 (100 * 50 / 10000 = 0.5 rounds to 0)
    let res: ConversionResponse = from_json(
        query_convert_credits_to_tokens(deps.as_ref(), Uint128::from(1u128)).unwrap(),
    )
    .unwrap();

    assert_eq!(res.token_amount, Uint128::from(100u128)); // net = gross when fee rounds to 0
    assert_eq!(res.fee_amount, Uint128::zero());
}

#[test]
fn test_conversion_large_amount() {
    let (deps, _sk) = setup();

    // 1_000_000_000 credits (1B) = 100_000_000_000 ushido gross
    let res: ConversionResponse = from_json(
        query_convert_credits_to_tokens(deps.as_ref(), Uint128::from(1_000_000_000u128)).unwrap(),
    )
    .unwrap();

    let expected_gross = Uint128::from(100_000_000_000u128);
    let expected_fee = Uint128::from(500_000_000u128); // 0.5%
    let expected_net = expected_gross - expected_fee;

    assert_eq!(res.token_amount, expected_net);
    assert_eq!(res.fee_amount, expected_fee);
}

// ─── Treasury Management ────────────────────────────────────────────────────

#[test]
fn test_withdraw_treasury_respects_reserve() {
    let (mut deps, _sk, _contract_addr) = setup_with_funded_treasury();
    let owner = a(&deps, "owner");

    // Contract has 100_000_000 ushido, min_reserve is 1_000_000
    // Try to withdraw too much
    let info = message_info(&owner, &[]);
    let err = execute_withdraw_treasury(
        deps.as_mut(),
        mock_env(),
        info,
        Uint128::from(99_500_000u128), // would leave only 500k, below 1M reserve
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::ReserveBreached { .. }));

    // Withdraw an allowed amount
    let info = message_info(&owner, &[]);
    execute_withdraw_treasury(
        deps.as_mut(),
        mock_env(),
        info,
        Uint128::from(99_000_000u128), // leaves exactly 1M
    )
    .unwrap();
}

#[test]
fn test_non_owner_cannot_withdraw_treasury() {
    let (mut deps, _sk, _contract_addr) = setup_with_funded_treasury();
    let rando = a(&deps, "rando");

    let info = message_info(&rando, &[]);
    let err = execute_withdraw_treasury(
        deps.as_mut(),
        mock_env(),
        info,
        Uint128::from(1_000u128),
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "owner".to_string()
        }
    );
}

// ─── Oracle Two-Step Transfer ───────────────────────────────────────────────

#[test]
fn test_oracle_transfer() {
    let (mut deps, _sk) = setup();
    let owner = a(&deps, "owner");
    let new_oracle = a(&deps, "new_oracle");
    let new_pubkey = Binary::from(vec![0x02; 33]); // dummy compressed pubkey

    let info = message_info(&owner, &[]);
    execute_propose_oracle(
        deps.as_mut(),
        mock_env(),
        info,
        new_oracle.to_string(),
        new_pubkey.clone(),
    )
    .unwrap();

    let pending: Option<sysbreak_credit_bridge::state::PendingOracleTransfer> =
        from_json(query_pending_oracle(deps.as_ref()).unwrap()).unwrap();
    assert!(pending.is_some());

    let info = message_info(&new_oracle, &[]);
    execute_accept_oracle(deps.as_mut(), mock_env(), info).unwrap();

    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.oracle, new_oracle);
    assert_eq!(config.oracle_pubkey, new_pubkey);
}

#[test]
fn test_wrong_address_cannot_accept_oracle() {
    let (mut deps, _sk) = setup();
    let owner = a(&deps, "owner");
    let new_oracle = a(&deps, "new_oracle");
    let rando = a(&deps, "rando");

    let info = message_info(&owner, &[]);
    execute_propose_oracle(
        deps.as_mut(),
        mock_env(),
        info,
        new_oracle.to_string(),
        Binary::from(vec![0x02; 33]),
    )
    .unwrap();

    let info = message_info(&rando, &[]);
    let err = execute_accept_oracle(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::NotPendingOracle);
}

// ─── Pause ──────────────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_deposits_and_withdrawals() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let owner = a(&deps, "owner");
    let player = a(&deps, "player1");

    // Pause
    let info = message_info(&owner, &[]);
    execute_pause(deps.as_mut(), mock_env(), info).unwrap();

    // Deposit fails
    let info = message_info(&player, &[Coin::new(1_000_000u128, DENOM)]);
    let err = execute_deposit(deps.as_mut(), mock_env(), info).unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Withdrawal fails
    let credit_amount = Uint128::from(1_000u128);
    let token_amount = Uint128::from(99_500u128);
    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("paused"),
        player.as_str(),
        credit_amount,
        token_amount,
    );
    let info = message_info(&player, &[]);
    let err = execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("paused"),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Paused);

    // Unpause
    let info = message_info(&owner, &[]);
    execute_unpause(deps.as_mut(), mock_env(), info).unwrap();
}

// ─── Admin Updates ──────────────────────────────────────────────────────────

#[test]
fn test_update_rate() {
    let (mut deps, _sk) = setup();
    let owner = a(&deps, "owner");

    let info = message_info(&owner, &[]);
    execute_update_rate(
        deps.as_mut(),
        mock_env(),
        info,
        Uint128::from(20_000u128),
        Uint128::from(1_000_000u128),
    )
    .unwrap();

    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.rate_credits, Uint128::from(20_000u128));
}

#[test]
fn test_update_limits() {
    let (mut deps, _sk) = setup();
    let owner = a(&deps, "owner");

    let info = message_info(&owner, &[]);
    execute_update_limits(
        deps.as_mut(),
        mock_env(),
        info,
        Some(Uint128::from(200_000u128)),
        None,
        Some(1800),
        None,
        None,
    )
    .unwrap();

    let config: Config = from_json(query_config(deps.as_ref()).unwrap()).unwrap();
    assert_eq!(config.player_daily_limit, Uint128::from(200_000u128));
    assert_eq!(config.cooldown_seconds, 1800);
    // Unchanged values
    assert_eq!(config.global_daily_limit, Uint128::from(10_000_000u128));
}

// ─── Player Info Query ──────────────────────────────────────────────────────

#[test]
fn test_player_info_query() {
    let (mut deps, sk, contract_addr) = setup_with_funded_treasury();
    let player = a(&deps, "player1");

    // Before any withdrawal
    let res: PlayerInfoResponse = from_json(
        query_player_info(deps.as_ref(), mock_env(), player.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(res.withdrawals_24h, Uint128::zero());
    assert_eq!(res.remaining_limit, Uint128::from(100_000u128));

    // Do a withdrawal
    let credit_amount = Uint128::from(5_000u128);
    let token_amount = Uint128::from(497_500u128);
    let sig = sign_withdrawal(
        &sk,
        CHAIN_ID,
        &contract_addr,
        &ts_nonce("info"),
        player.as_str(),
        credit_amount,
        token_amount,
    );
    let info = message_info(&player, &[]);
    execute_withdraw(
        deps.as_mut(),
        mock_env(),
        info,
        ts_nonce("info"),
        credit_amount,
        token_amount,
        sig,
    )
    .unwrap();

    let res: PlayerInfoResponse = from_json(
        query_player_info(deps.as_ref(), mock_env(), player.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(res.withdrawals_24h, Uint128::from(5_000u128));
    assert_eq!(res.remaining_limit, Uint128::from(95_000u128));
}
