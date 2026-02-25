use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
use cosmwasm_std::{coin, from_json, Addr, BankMsg, Timestamp, Uint128};

use sysbreak_corporation_dao::contract::{execute, instantiate, query};
use sysbreak_corporation_dao::error::ContractError;
use sysbreak_corporation_dao::msg::*;
use sysbreak_corporation_dao::state::*;

const DENOM: &str = "ushido";

fn setup_deps() -> cosmwasm_std::OwnedDeps<
    cosmwasm_std::MemoryStorage,
    cosmwasm_std::testing::MockApi,
    cosmwasm_std::testing::MockQuerier,
> {
    mock_dependencies()
}

fn addr(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, name: &str) -> Addr {
    deps.api.addr_make(name)
}

fn default_instantiate_msg(owner: &Addr) -> InstantiateMsg {
    InstantiateMsg {
        owner: owner.to_string(),
        denom: DENOM.to_string(),
        creation_fee: Uint128::new(1000),
        proposal_deposit: Uint128::new(500),
        default_max_members: 50,
        default_quorum_bps: 5100, // 51%
        default_voting_period: 259200, // 3 days
    }
}

fn do_instantiate(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
) -> Addr {
    let owner = deps.api.addr_make("owner");
    let msg = default_instantiate_msg(&owner);
    let info = message_info(&owner, &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    owner
}

fn create_corporation(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    sender: &Addr,
    name: &str,
    join_policy: JoinPolicy,
) -> u64 {
    let info = message_info(sender, &[coin(1000, DENOM)]);
    let msg = ExecuteMsg::CreateCorporation {
        name: name.to_string(),
        description: format!("{} description", name),
        join_policy,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    // Extract corp_id from attributes
    res.attributes
        .iter()
        .find(|a| a.key == "corp_id")
        .unwrap()
        .value
        .parse()
        .unwrap()
}

fn join_corporation(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    sender: &Addr,
    corp_id: u64,
) {
    let info = message_info(sender, &[]);
    let msg = ExecuteMsg::JoinCorporation { corp_id };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();
}

fn create_proposal(
    deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
    env: &cosmwasm_std::Env,
    sender: &Addr,
    corp_id: u64,
    proposal_type: ProposalTypeMsg,
) -> u64 {
    let info = message_info(sender, &[coin(500, DENOM)]);
    let msg = ExecuteMsg::CreateProposal {
        corp_id,
        proposal_type,
    };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    res.attributes
        .iter()
        .find(|a| a.key == "proposal_id")
        .unwrap()
        .value
        .parse()
        .unwrap()
}

// ─── Tests ────────────────────────────────────────────────────────────

#[test]
fn test_instantiate() {
    let mut deps = setup_deps();
    let owner = do_instantiate(&mut deps);

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();
    let config: Config = from_json(res).unwrap();
    assert_eq!(config.owner, owner);
    assert_eq!(config.denom, DENOM);
    assert_eq!(config.creation_fee, Uint128::new(1000));
}

#[test]
fn test_create_corporation() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "TestCorp", JoinPolicy::Open);
    assert_eq!(corp_id, 1);

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id: 1 }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.name, "TestCorp");
    assert_eq!(resp.corporation.founder, founder);
    assert_eq!(resp.corporation.member_count, 1);
}

#[test]
fn test_create_corporation_insufficient_fee() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let info = message_info(&founder, &[coin(999, DENOM)]);
    let msg = ExecuteMsg::CreateCorporation {
        name: "TestCorp".to_string(),
        description: "desc".to_string(),
        join_policy: JoinPolicy::Open,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::InsufficientCreationFee);
}

#[test]
fn test_join_open_corporation() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "OpenCorp", JoinPolicy::Open);

    let member = addr(&deps, "member1");
    join_corporation(&mut deps, &member, corp_id);

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.member_count, 2);

    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::MemberInfo {
            corp_id,
            address: member.to_string(),
        },
    )
    .unwrap();
    let resp: MemberInfoResponse = from_json(res).unwrap();
    assert!(resp.is_member);
    assert_eq!(resp.info.unwrap().role, MemberRole::Member);
}

#[test]
fn test_cannot_join_invite_only() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "PrivateCorp", JoinPolicy::InviteOnly);

    let member = addr(&deps, "member1");
    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::JoinCorporation { corp_id };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::InviteOnly);
}

#[test]
fn test_invite_and_accept() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "PrivateCorp", JoinPolicy::InviteOnly);

    let invitee = addr(&deps, "invitee");

    // Founder invites
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::InviteMember {
        corp_id,
        invitee: invitee.to_string(),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    // Invitee accepts
    let info = message_info(&invitee, &[]);
    let msg = ExecuteMsg::AcceptInvite { corp_id };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.member_count, 2);
}

#[test]
fn test_accept_invite_without_invite() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::InviteOnly);

    let random = addr(&deps, "random");
    let info = message_info(&random, &[]);
    let msg = ExecuteMsg::AcceptInvite { corp_id };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::NoPendingInvite);
}

#[test]
fn test_leave_corporation() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let member = addr(&deps, "member1");
    join_corporation(&mut deps, &member, corp_id);

    // Member leaves
    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::LeaveCorporation { corp_id };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.member_count, 1);
}

#[test]
fn test_founder_cannot_leave_with_members() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let member = addr(&deps, "member1");
    join_corporation(&mut deps, &member, corp_id);

    // Founder tries to leave
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::LeaveCorporation { corp_id };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::FounderCannotLeave);
}

#[test]
fn test_donate_treasury() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let info = message_info(&founder, &[coin(5000, DENOM)]);
    let msg = ExecuteMsg::DonateTreasury { corp_id };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.treasury_balance, Uint128::new(5000));
}

#[test]
fn test_create_and_vote_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Add a member (they need to have joined BEFORE the proposal is created)
    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Advance time, then create proposal
    env.block.time = Timestamp::from_seconds(2000);

    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Test".to_string(),
            description: "A test proposal".to_string(),
        },
    );
    assert_eq!(proposal_id, 1);

    // Founder votes yes
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Member votes yes
    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Check vote status
    let res = query(deps.as_ref(), env.clone(), QueryMsg::VoteStatus { proposal_id }).unwrap();
    let status: VoteStatusResponse = from_json(res).unwrap();
    assert_eq!(status.yes_votes, 2);
    assert_eq!(status.no_votes, 0);
    assert_eq!(status.total_members, 2);
    assert!(status.quorum_reached);
    assert!(status.passed);
}

#[test]
fn test_flash_join_voting_protection() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Create proposal at time 1000
    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Test".to_string(),
            description: "desc".to_string(),
        },
    );

    // Member joins AFTER proposal created (same timestamp counts as "after")
    let member = addr(&deps, "flashjoiner");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Flash-joiner tries to vote — should fail
    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::JoinedAfterProposal);
}

#[test]
fn test_cannot_vote_twice() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Test".to_string(),
            description: "desc".to_string(),
        },
    );

    // Founder votes
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Try to vote again
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: false,
    };
    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::AlreadyVoted { id: proposal_id });
}

#[test]
fn test_execute_passed_custom_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Add member before proposal
    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Alliance".to_string(),
            description: "Form alliance with Corp2".to_string(),
        },
    );

    // Both vote yes
    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Advance past voting period (3 days = 259200s)
    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    assert!(res.attributes.iter().any(|a| a.key == "result" && a.value == "custom_passed"));

    // Check proposal status
    let res = query(deps.as_ref(), env, QueryMsg::Proposal { proposal_id }).unwrap();
    let resp: ProposalResponse = from_json(res).unwrap();
    assert_eq!(resp.proposal.status, ProposalStatus::Executed);
}

#[test]
fn test_execute_failed_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Add member
    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Bad idea".to_string(),
            description: "This will fail".to_string(),
        },
    );

    // Both vote no
    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: false,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert!(res.attributes.iter().any(|a| a.key == "result" && a.value == "failed"));
}

#[test]
fn test_treasury_spend_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Donate to treasury
    {
        let info = message_info(&founder, &[coin(10000, DENOM)]);
        let msg = ExecuteMsg::DonateTreasury { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Add member
    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    let recipient = addr(&deps, "recipient");

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::TreasurySpend {
            recipient: recipient.to_string(),
            amount: Uint128::new(2500), // exactly 25%
        },
    );

    // Both vote yes
    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Should have bank messages (deposit refund + treasury spend)
    assert_eq!(res.messages.len(), 2);

    // Check treasury decreased
    let res = query(deps.as_ref(), env, QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.treasury_balance, Uint128::new(7500));
}

#[test]
fn test_treasury_spend_exceeds_25_percent() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Donate to treasury
    {
        let info = message_info(&founder, &[coin(10000, DENOM)]);
        let msg = ExecuteMsg::DonateTreasury { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    let recipient = addr(&deps, "recipient");

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::TreasurySpend {
            recipient: recipient.to_string(),
            amount: Uint128::new(2501), // over 25%
        },
    );

    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::SpendExceedsLimit);
}

#[test]
fn test_change_settings_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::ChangeSettings {
            name: Some("NewName".to_string()),
            description: None,
            join_policy: Some(JoinPolicy::InviteOnly),
            quorum_bps: Some(6000),
            voting_period: None,
        },
    );

    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let res = query(deps.as_ref(), env, QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.name, "NewName");
    assert_eq!(resp.corporation.join_policy, JoinPolicy::InviteOnly);
    assert_eq!(resp.corporation.quorum_bps, 6000);
}

#[test]
fn test_kick_member_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    let bad_member = addr(&deps, "badmember");
    {
        let info = message_info(&bad_member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::KickMember {
            member: bad_member.to_string(),
        },
    );

    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Verify kicked
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::MemberInfo {
            corp_id,
            address: bad_member.to_string(),
        },
    )
    .unwrap();
    let resp: MemberInfoResponse = from_json(res).unwrap();
    assert!(!resp.is_member);
}

#[test]
fn test_promote_member_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    let member = addr(&deps, "member1");
    {
        let info = message_info(&member, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::PromoteMember {
            member: member.to_string(),
            new_role: MemberRole::Officer,
        },
    );

    // Only founder can vote (member joined at same time as corp creation, which is before proposal)
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::MemberInfo {
            corp_id,
            address: member.to_string(),
        },
    )
    .unwrap();
    let resp: MemberInfoResponse = from_json(res).unwrap();
    assert_eq!(resp.info.unwrap().role, MemberRole::Officer);
}

#[test]
fn test_dissolution_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Donate treasury
    {
        let info = message_info(&founder, &[coin(10000, DENOM)]);
        let msg = ExecuteMsg::DonateTreasury { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Need 75% supermajority — with 1 member, founder's vote = 100%
    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Dissolution,
    );

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::Vote {
        proposal_id,
        vote: true,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Corp should be dissolving
    let res = query(deps.as_ref(), env.clone(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.status, CorporationStatus::Dissolving);

    // Claim dissolution share
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ClaimDissolution { corp_id };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Should have bank send message with share
    assert_eq!(res.messages.len(), 1);
    let bank_msg = &res.messages[0].msg;
    match bank_msg {
        cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { amount, .. }) => {
            assert_eq!(amount[0].amount, Uint128::new(10000));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Corp should be dissolved (last member claimed)
    let res = query(deps.as_ref(), env, QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.status, CorporationStatus::Dissolved);
}

#[test]
fn test_dissolution_requires_supermajority() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Add 3 more members (total 4) — need 3 yes votes for 75%
    let m1 = addr(&deps, "m1");
    let m2 = addr(&deps, "m2");
    let m3 = addr(&deps, "m3");

    for m in [&m1, &m2, &m3] {
        let info = message_info(m, &[]);
        let msg = ExecuteMsg::JoinCorporation { corp_id };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Dissolution,
    );

    // Only 2 out of 4 vote yes (50%, need 75%)
    for voter in [&founder, &m1] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }
    for voter in [&m2, &m3] {
        let info = message_info(voter, &[]);
        let msg = ExecuteMsg::Vote {
            proposal_id,
            vote: false,
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    // This should fail because even though quorum (51%) is met, dissolution needs 75% supermajority
    // But first the general pass check happens: 2 yes vs 2 no => not passed (yes must be > no)
    // So it fails as "failed" proposal
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert!(res.attributes.iter().any(|a| a.key == "result" && a.value == "failed"));
}

#[test]
fn test_voting_not_ended() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Test".to_string(),
            description: "desc".to_string(),
        },
    );

    // Try to execute before voting ends
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::ExecuteProposal { proposal_id };
    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::VotingNotEnded { id: proposal_id });
}

#[test]
fn test_update_description_founder_only() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let member = addr(&deps, "member1");
    join_corporation(&mut deps, &member, corp_id);

    // Founder updates description
    let info = message_info(&founder, &[]);
    let msg = ExecuteMsg::UpdateDescription {
        corp_id,
        description: "Updated description".to_string(),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Corporation { corp_id }).unwrap();
    let resp: CorporationResponse = from_json(res).unwrap();
    assert_eq!(resp.corporation.description, "Updated description");

    // Member cannot update
    let info = message_info(&member, &[]);
    let msg = ExecuteMsg::UpdateDescription {
        corp_id,
        description: "Hacked!".to_string(),
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Unauthorized {
            role: "founder".to_string()
        }
    );
}

#[test]
fn test_list_corporations() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    create_corporation(&mut deps, &founder, "Corp1", JoinPolicy::Open);
    create_corporation(&mut deps, &founder, "Corp2", JoinPolicy::InviteOnly);
    create_corporation(&mut deps, &founder, "Corp3", JoinPolicy::Open);

    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::ListCorporations {
            start_after: None,
            limit: Some(2),
        },
    )
    .unwrap();
    let resp: CorporationsListResponse = from_json(res).unwrap();
    assert_eq!(resp.corporations.len(), 2);
    assert_eq!(resp.corporations[0].name, "Corp1");
    assert_eq!(resp.corporations[1].name, "Corp2");

    // Pagination
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::ListCorporations {
            start_after: Some(2),
            limit: None,
        },
    )
    .unwrap();
    let resp: CorporationsListResponse = from_json(res).unwrap();
    assert_eq!(resp.corporations.len(), 1);
    assert_eq!(resp.corporations[0].name, "Corp3");
}

#[test]
fn test_list_members() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let m1 = addr(&deps, "member1");
    let m2 = addr(&deps, "member2");
    join_corporation(&mut deps, &m1, corp_id);
    join_corporation(&mut deps, &m2, corp_id);

    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Members {
            corp_id,
            start_after: None,
            limit: None,
        },
    )
    .unwrap();
    let resp: MembersListResponse = from_json(res).unwrap();
    assert_eq!(resp.members.len(), 3); // founder + 2 members
}

#[test]
fn test_corporation_full() {
    let mut deps = setup_deps();

    // Create with max_members = 2
    let owner = deps.api.addr_make("owner");
    let msg = InstantiateMsg {
        owner: owner.to_string(),
        denom: DENOM.to_string(),
        creation_fee: Uint128::new(1000),
        proposal_deposit: Uint128::new(500),
        default_max_members: 2,
        default_quorum_bps: 5100,
        default_voting_period: 259200,
    };
    let info = message_info(&owner, &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "SmallCorp", JoinPolicy::Open);

    let m1 = addr(&deps, "m1");
    join_corporation(&mut deps, &m1, corp_id);

    // 3rd member should fail
    let m2 = addr(&deps, "m2");
    let info = message_info(&m2, &[]);
    let msg = ExecuteMsg::JoinCorporation { corp_id };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::CorporationFull { max: 2 });
}

#[test]
fn test_already_member() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let m1 = addr(&deps, "m1");
    join_corporation(&mut deps, &m1, corp_id);

    // Try to join again
    let info = message_info(&m1, &[]);
    let msg = ExecuteMsg::JoinCorporation { corp_id };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::AlreadyMember { corp_id });
}

#[test]
fn test_non_member_cannot_create_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let corp_id = create_corporation(&mut deps, &founder, "Corp", JoinPolicy::Open);

    let outsider = addr(&deps, "outsider");
    let info = message_info(&outsider, &[coin(500, DENOM)]);
    let msg = ExecuteMsg::CreateProposal {
        corp_id,
        proposal_type: ProposalTypeMsg::Custom {
            title: "Hack".to_string(),
            description: "desc".to_string(),
        },
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::NotMember { corp_id });
}

#[test]
fn test_dissolving_blocks_new_proposals() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    // Create and pass dissolution
    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Dissolution,
    );

    let info = message_info(&founder, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Vote {
            proposal_id,
            vote: true,
        },
    )
    .unwrap();

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);
    let info = message_info(&founder, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ExecuteProposal { proposal_id },
    )
    .unwrap();

    // Try to create new proposal — should fail
    let info = message_info(&founder, &[coin(500, DENOM)]);
    let msg = ExecuteMsg::CreateProposal {
        corp_id,
        proposal_type: ProposalTypeMsg::Custom {
            title: "Blocked".to_string(),
            description: "desc".to_string(),
        },
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::Dissolving);
}

#[test]
fn test_already_executed_proposal() {
    let mut deps = setup_deps();
    do_instantiate(&mut deps);

    let founder = addr(&deps, "founder");
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1000);

    let corp_id = {
        let info = message_info(&founder, &[coin(1000, DENOM)]);
        let msg = ExecuteMsg::CreateCorporation {
            name: "Corp".to_string(),
            description: "desc".to_string(),
            join_policy: JoinPolicy::Open,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        res.attributes.iter().find(|a| a.key == "corp_id").unwrap().value.parse::<u64>().unwrap()
    };

    let member = addr(&deps, "m1");
    {
        let info = message_info(&member, &[]);
        execute(deps.as_mut(), env.clone(), info, ExecuteMsg::JoinCorporation { corp_id }).unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000);
    let proposal_id = create_proposal(
        &mut deps,
        &env,
        &founder,
        corp_id,
        ProposalTypeMsg::Custom {
            title: "Test".to_string(),
            description: "desc".to_string(),
        },
    );

    for voter in [&founder, &member] {
        let info = message_info(voter, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Vote {
                proposal_id,
                vote: true,
            },
        )
        .unwrap();
    }

    env.block.time = Timestamp::from_seconds(2000 + 259200 + 1);

    let info = message_info(&founder, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ExecuteProposal { proposal_id },
    )
    .unwrap();

    // Try to execute again
    let info = message_info(&founder, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::ExecuteProposal { proposal_id },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::AlreadyExecuted { id: proposal_id });
}
