use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::helpers::{
    assert_active, assert_member, assert_not_dissolved, assert_officer_or_founder,
    assert_voting_active, assert_voting_ended, check_dissolution_supermajority,
    check_proposal_passed, load_config, load_corporation, reject_funds, validate_funds,
    validate_funds_min, validate_quorum_bps, validate_voting_period,
};
use crate::msg::{
    CorporationResponse, CorporationsListResponse, ExecuteMsg, InstantiateMsg, MemberEntry,
    MemberInfoResponse, MembersListResponse, MigrateMsg, ProposalResponse, ProposalTypeMsg,
    ProposalsListResponse, QueryMsg, VoteStatusResponse,
};
use crate::state::{
    Config, Corporation, CorporationStatus, JoinPolicy, MemberInfo, MemberRole,
    PendingOwnerTransfer, Proposal, ProposalStatus, ProposalType, CONFIG, CORPORATIONS,
    CORP_COUNT, CORP_PROPOSALS, DISSOLUTION_CLAIMS, INVITES, MEMBERS, PENDING_OWNER, PROPOSALS,
    PROPOSAL_COUNT, VOTES,
};

const CONTRACT_NAME: &str = "crates.io:sysbreak-corporation-dao";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Instantiate ──────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // FIX: M-02 — validate governance parameters on instantiation
    validate_quorum_bps(msg.default_quorum_bps)?;
    validate_voting_period(msg.default_voting_period)?;

    let owner = deps.api.addr_validate(&msg.owner)?;
    let config = Config {
        owner,
        denom: msg.denom,
        creation_fee: msg.creation_fee,
        proposal_deposit: msg.proposal_deposit,
        default_max_members: msg.default_max_members,
        default_quorum_bps: msg.default_quorum_bps,
        default_voting_period: msg.default_voting_period,
    };
    CONFIG.save(deps.storage, &config)?;
    CORP_COUNT.save(deps.storage, &0u64)?;
    PROPOSAL_COUNT.save(deps.storage, &0u64)?;

    Ok(Response::new().add_attribute("action", "instantiate"))
}

// ─── Execute ──────────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreateCorporation {
            name,
            description,
            join_policy,
        } => execute_create_corporation(deps, env, info, name, description, join_policy),
        ExecuteMsg::JoinCorporation { corp_id } => {
            execute_join_corporation(deps, env, info, corp_id)
        }
        ExecuteMsg::InviteMember { corp_id, invitee } => {
            execute_invite_member(deps, info, corp_id, invitee)
        }
        ExecuteMsg::AcceptInvite { corp_id } => execute_accept_invite(deps, env, info, corp_id),
        ExecuteMsg::LeaveCorporation { corp_id } => {
            execute_leave_corporation(deps, info, corp_id)
        }
        ExecuteMsg::DonateTreasury { corp_id } => {
            execute_donate_treasury(deps, info, corp_id)
        }
        ExecuteMsg::CreateProposal {
            corp_id,
            proposal_type,
        } => execute_create_proposal(deps, env, info, corp_id, proposal_type),
        ExecuteMsg::Vote { proposal_id, vote } => {
            execute_vote(deps, env, info, proposal_id, vote)
        }
        ExecuteMsg::ExecuteProposal { proposal_id } => {
            execute_execute_proposal(deps, env, info, proposal_id)
        }
        ExecuteMsg::ClaimDissolution { corp_id } => {
            execute_claim_dissolution(deps, info, corp_id)
        }
        ExecuteMsg::UpdateDescription {
            corp_id,
            description,
        } => execute_update_description(deps, info, corp_id, description),
        // FIX: H-01
        ExecuteMsg::WithdrawFees { amount } => execute_withdraw_fees(deps, env, info, amount),
        // FIX: H-04
        ExecuteMsg::ProposeOwner { new_owner } => execute_propose_owner(deps, info, new_owner),
        ExecuteMsg::AcceptOwner {} => execute_accept_owner(deps, info),
        ExecuteMsg::CancelOwnerTransfer {} => execute_cancel_owner_transfer(deps, info),
    }
}

// ─── Create Corporation ───────────────────────────────────────────────

fn execute_create_corporation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    name: String,
    description: String,
    join_policy: JoinPolicy,
) -> Result<Response, ContractError> {
    let config = load_config(deps.as_ref())?;

    // Validate creation fee
    validate_funds(
        &info,
        &config.denom,
        config.creation_fee,
        ContractError::InsufficientCreationFee,
    )?;

    let corp_id = CORP_COUNT.load(deps.storage)? + 1;
    CORP_COUNT.save(deps.storage, &corp_id)?;

    let corp = Corporation {
        id: corp_id,
        name: name.clone(),
        description,
        founder: info.sender.clone(),
        join_policy,
        quorum_bps: config.default_quorum_bps,
        voting_period: config.default_voting_period,
        max_members: config.default_max_members,
        member_count: 1,
        treasury_balance: Uint128::zero(),
        created_at: env.block.time,
        status: CorporationStatus::Active,
    };
    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    // Add founder as first member
    let member_info = MemberInfo {
        role: MemberRole::Founder,
        joined_at: env.block.time,
    };
    MEMBERS.save(deps.storage, (corp_id, &info.sender), &member_info)?;

    Ok(Response::new()
        .add_attribute("action", "create_corporation")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("name", name)
        .add_attribute("founder", info.sender.to_string()))
}

// ─── Join Corporation (Open) ──────────────────────────────────────────

fn execute_join_corporation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    corp_id: u64,
) -> Result<Response, ContractError> {
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;

    if corp.join_policy != JoinPolicy::Open {
        return Err(ContractError::InviteOnly);
    }

    // Check not already a member
    if MEMBERS.has(deps.storage, (corp_id, &info.sender)) {
        return Err(ContractError::AlreadyMember { corp_id });
    }

    // Check capacity
    if corp.member_count >= corp.max_members {
        return Err(ContractError::CorporationFull {
            max: corp.max_members,
        });
    }

    corp.member_count += 1;
    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    let member_info = MemberInfo {
        role: MemberRole::Member,
        joined_at: env.block.time,
    };
    MEMBERS.save(deps.storage, (corp_id, &info.sender), &member_info)?;

    Ok(Response::new()
        .add_attribute("action", "join_corporation")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("member", info.sender.to_string()))
}

// ─── Invite Member ────────────────────────────────────────────────────

fn execute_invite_member(
    deps: DepsMut,
    info: MessageInfo,
    corp_id: u64,
    invitee: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;
    assert_officer_or_founder(deps.as_ref(), corp_id, &info.sender)?;

    let invitee_addr = deps.api.addr_validate(&invitee)?;

    // Check not already a member
    if MEMBERS.has(deps.storage, (corp_id, &invitee_addr)) {
        return Err(ContractError::AlreadyMember { corp_id });
    }

    INVITES.save(deps.storage, (corp_id, &invitee_addr), &true)?;

    Ok(Response::new()
        .add_attribute("action", "invite_member")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("invitee", invitee_addr.to_string()))
}

// ─── Accept Invite ────────────────────────────────────────────────────

fn execute_accept_invite(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    corp_id: u64,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;

    // Check invite exists
    if !INVITES.has(deps.storage, (corp_id, &info.sender)) {
        return Err(ContractError::NoPendingInvite);
    }

    // Check not already a member
    if MEMBERS.has(deps.storage, (corp_id, &info.sender)) {
        return Err(ContractError::AlreadyMember { corp_id });
    }

    // Check capacity
    if corp.member_count >= corp.max_members {
        return Err(ContractError::CorporationFull {
            max: corp.max_members,
        });
    }

    // Remove invite
    INVITES.remove(deps.storage, (corp_id, &info.sender));

    corp.member_count += 1;
    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    let member_info = MemberInfo {
        role: MemberRole::Member,
        joined_at: env.block.time,
    };
    MEMBERS.save(deps.storage, (corp_id, &info.sender), &member_info)?;

    Ok(Response::new()
        .add_attribute("action", "accept_invite")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("member", info.sender.to_string()))
}

// ─── Leave Corporation ────────────────────────────────────────────────

fn execute_leave_corporation(
    deps: DepsMut,
    info: MessageInfo,
    corp_id: u64,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_not_dissolved(&corp)?;

    let member = assert_member(deps.as_ref(), corp_id, &info.sender)?;

    // Founder cannot leave while other members exist
    if member.role == MemberRole::Founder && corp.member_count > 1 {
        return Err(ContractError::FounderCannotLeave);
    }

    MEMBERS.remove(deps.storage, (corp_id, &info.sender));
    corp.member_count -= 1;

    // If founder leaves (last member), dissolve
    if corp.member_count == 0 {
        corp.status = CorporationStatus::Dissolved;
    }

    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    Ok(Response::new()
        .add_attribute("action", "leave_corporation")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("member", info.sender.to_string()))
}

// ─── Donate Treasury ──────────────────────────────────────────────────

// FIX: I-03 — DonateTreasury intentionally allows non-member donations.
// This is by design: public treasury funding enables external sponsorship of corporations.
fn execute_donate_treasury(
    deps: DepsMut,
    info: MessageInfo,
    corp_id: u64,
) -> Result<Response, ContractError> {
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;

    let config = load_config(deps.as_ref())?;
    let amount = validate_funds_min(
        &info,
        &config.denom,
        Uint128::one(),
        ContractError::ZeroAmount,
    )?;

    corp.treasury_balance = corp
        .treasury_balance
        .checked_add(amount)
        .map_err(|_| ContractError::Overflow)?;
    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    Ok(Response::new()
        .add_attribute("action", "donate_treasury")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("amount", amount.to_string()))
}

// ─── Create Proposal ──────────────────────────────────────────────────

fn execute_create_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    corp_id: u64,
    proposal_type_msg: ProposalTypeMsg,
) -> Result<Response, ContractError> {
    let corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;
    assert_member(deps.as_ref(), corp_id, &info.sender)?;

    let config = load_config(deps.as_ref())?;

    // Validate proposal deposit
    validate_funds(
        &info,
        &config.denom,
        config.proposal_deposit,
        ContractError::InsufficientProposalDeposit,
    )?;

    // Convert msg-level proposal type to state-level (validate addresses)
    let proposal_type = match proposal_type_msg {
        ProposalTypeMsg::TreasurySpend { recipient, amount } => {
            let recipient_addr = deps.api.addr_validate(&recipient)?;
            ProposalType::TreasurySpend {
                recipient: recipient_addr,
                amount,
            }
        }
        ProposalTypeMsg::ChangeSettings {
            name,
            description,
            join_policy,
            quorum_bps,
            voting_period,
        } => ProposalType::ChangeSettings {
            name,
            description,
            join_policy,
            quorum_bps,
            voting_period,
        },
        ProposalTypeMsg::KickMember { member } => {
            let member_addr = deps.api.addr_validate(&member)?;
            ProposalType::KickMember {
                member: member_addr,
            }
        }
        ProposalTypeMsg::PromoteMember { member, new_role } => {
            let member_addr = deps.api.addr_validate(&member)?;
            ProposalType::PromoteMember {
                member: member_addr,
                new_role,
            }
        }
        ProposalTypeMsg::Dissolution => ProposalType::Dissolution,
        ProposalTypeMsg::Custom { title, description } => {
            ProposalType::Custom { title, description }
        }
    };

    let proposal_id = PROPOSAL_COUNT.load(deps.storage)? + 1;
    PROPOSAL_COUNT.save(deps.storage, &proposal_id)?;

    let voting_ends_at = Timestamp::from_seconds(env.block.time.seconds() + corp.voting_period);

    let proposal = Proposal {
        id: proposal_id,
        corp_id,
        proposer: info.sender.clone(),
        proposal_type,
        status: ProposalStatus::Active,
        yes_votes: 0,
        no_votes: 0,
        created_at: env.block.time,
        voting_ends_at,
        deposit: config.proposal_deposit,
        // FIX: H-02 — snapshot member count at creation for quorum evaluation
        member_count_snapshot: corp.member_count,
    };
    PROPOSALS.save(deps.storage, proposal_id, &proposal)?;
    // FIX: M-07 — insert into secondary index for efficient corp-based queries
    CORP_PROPOSALS.save(deps.storage, (corp_id, proposal_id), &())?;

    Ok(Response::new()
        .add_attribute("action", "create_proposal")
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("proposer", info.sender.to_string()))
}

// ─── Vote ─────────────────────────────────────────────────────────────

fn execute_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
    vote: bool,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let mut proposal = PROPOSALS
        .load(deps.storage, proposal_id)
        .map_err(|_| ContractError::ProposalNotFound { id: proposal_id })?;

    assert_voting_active(&proposal, &env)?;

    // Must be a member
    let member = assert_member(deps.as_ref(), proposal.corp_id, &info.sender)?;

    // Flash-join protection: member must have joined BEFORE proposal was created
    if member.joined_at >= proposal.created_at {
        return Err(ContractError::JoinedAfterProposal);
    }

    // Check not already voted
    if VOTES.has(deps.storage, (proposal_id, &info.sender)) {
        return Err(ContractError::AlreadyVoted { id: proposal_id });
    }

    // Record vote (final, no changes allowed)
    VOTES.save(deps.storage, (proposal_id, &info.sender), &vote)?;

    if vote {
        proposal.yes_votes += 1;
    } else {
        proposal.no_votes += 1;
    }
    PROPOSALS.save(deps.storage, proposal_id, &proposal)?;

    Ok(Response::new()
        .add_attribute("action", "vote")
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("voter", info.sender.to_string())
        .add_attribute("vote", vote.to_string()))
}

// ─── Execute Proposal ─────────────────────────────────────────────────

// FIX: I-04 — ExecuteProposal is intentionally callable by any address.
// This is by design: permissionless execution after quorum prevents governance deadlock
// where no member is online to finalize a passing proposal.
fn execute_execute_proposal(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    reject_funds(&_info)?; // FIX: M-08
    let mut proposal = PROPOSALS
        .load(deps.storage, proposal_id)
        .map_err(|_| ContractError::ProposalNotFound { id: proposal_id })?;

    if proposal.status == ProposalStatus::Executed {
        return Err(ContractError::AlreadyExecuted { id: proposal_id });
    }
    if proposal.status != ProposalStatus::Active {
        return Err(ContractError::ProposalNotPending { id: proposal_id });
    }

    assert_voting_ended(&proposal, &env)?;

    let mut corp = load_corporation(deps.as_ref(), proposal.corp_id)?;
    let config = load_config(deps.as_ref())?;

    // FIX: H-02 — use snapshot member count, not current, for quorum evaluation
    let passed = check_proposal_passed(&proposal, proposal.member_count_snapshot, corp.quorum_bps);

    let mut msgs: Vec<BankMsg> = vec![];
    let mut resp = Response::new()
        .add_attribute("action", "execute_proposal")
        .add_attribute("proposal_id", proposal_id.to_string());

    if !passed {
        // Failed — burn deposit (don't refund)
        proposal.status = ProposalStatus::Failed;
        PROPOSALS.save(deps.storage, proposal_id, &proposal)?;

        return Ok(resp.add_attribute("result", "failed"));
    }

    // Mark as executed BEFORE dispatching any bank messages (check-effects-interactions)
    proposal.status = ProposalStatus::Executed;
    PROPOSALS.save(deps.storage, proposal_id, &proposal)?;

    // Refund deposit to proposer
    if !proposal.deposit.is_zero() {
        msgs.push(BankMsg::Send {
            to_address: proposal.proposer.to_string(),
            amount: vec![Coin {
                denom: config.denom.clone(),
                amount: proposal.deposit,
            }],
        });
    }

    match &proposal.proposal_type {
        ProposalType::TreasurySpend { recipient, amount } => {
            // Enforce 25% max spend per proposal
            let max_spend = corp
                .treasury_balance
                .checked_mul(Uint128::new(25))
                .map_err(|_| ContractError::Overflow)?
                .checked_div(Uint128::new(100))
                .map_err(|_| ContractError::Overflow)?;

            if *amount > max_spend {
                return Err(ContractError::SpendExceedsLimit);
            }

            corp.treasury_balance = corp
                .treasury_balance
                .checked_sub(*amount)
                .map_err(|_| ContractError::Overflow)?;
            CORPORATIONS.save(deps.storage, proposal.corp_id, &corp)?;

            msgs.push(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![Coin {
                    denom: config.denom.clone(),
                    amount: *amount,
                }],
            });

            resp = resp.add_attribute("spend_amount", amount.to_string());
        }

        ProposalType::ChangeSettings {
            name,
            description,
            join_policy,
            quorum_bps,
            voting_period,
        } => {
            // FIX: M-02 — validate governance parameters before applying
            if let Some(q) = quorum_bps {
                validate_quorum_bps(*q)?;
            }
            if let Some(vp) = voting_period {
                validate_voting_period(*vp)?;
            }

            if let Some(n) = name {
                corp.name = n.clone();
            }
            if let Some(d) = description {
                corp.description = d.clone();
            }
            if let Some(jp) = join_policy {
                corp.join_policy = jp.clone();
            }
            if let Some(q) = quorum_bps {
                corp.quorum_bps = *q;
            }
            if let Some(vp) = voting_period {
                corp.voting_period = *vp;
            }
            CORPORATIONS.save(deps.storage, proposal.corp_id, &corp)?;

            resp = resp.add_attribute("result", "settings_changed");
        }

        ProposalType::KickMember { member } => {
            // FIX: L-04 — Design decision: kicked member's existing votes remain counted.
            // This follows the common DAO pattern of vote finality — once cast, votes are
            // permanent for that proposal. Retroactively removing votes would be complex
            // and could allow governance manipulation.

            // Cannot kick the last member
            if corp.member_count <= 1 {
                return Err(ContractError::CannotKickLastMember);
            }

            MEMBERS.remove(deps.storage, (proposal.corp_id, member));
            corp.member_count -= 1;
            CORPORATIONS.save(deps.storage, proposal.corp_id, &corp)?;

            resp = resp.add_attribute("kicked", member.to_string());
        }

        ProposalType::PromoteMember { member, new_role } => {
            // FIX: H-03 — prevent creating multiple Founders via proposal
            if *new_role == MemberRole::Founder {
                return Err(ContractError::CannotPromoteToFounder);
            }

            let mut member_info =
                MEMBERS
                    .load(deps.storage, (proposal.corp_id, member))
                    .map_err(|_| ContractError::NotMember {
                        corp_id: proposal.corp_id,
                    })?;

            member_info.role = new_role.clone();
            MEMBERS.save(deps.storage, (proposal.corp_id, member), &member_info)?;

            resp = resp.add_attribute("promoted", member.to_string());
        }

        ProposalType::Dissolution => {
            // FIX: H-02 — use snapshot for supermajority check
            check_dissolution_supermajority(proposal.yes_votes, proposal.member_count_snapshot)?;

            corp.status = CorporationStatus::Dissolving;

            // FIX: L-01 — distribute remainder to founder so no funds are locked
            if !corp.treasury_balance.is_zero() && corp.member_count > 0 {
                let member_count_u128 = Uint128::from(corp.member_count);
                let share = corp
                    .treasury_balance
                    .checked_div(member_count_u128)
                    .map_err(|_| ContractError::Overflow)?;
                let remainder = corp.treasury_balance.checked_rem(member_count_u128)
                    .map_err(|_| ContractError::Overflow)?;

                // Record claims for all current members
                let members: Vec<_> = MEMBERS
                    .prefix(proposal.corp_id)
                    .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                    .collect::<StdResult<Vec<_>>>()?;

                for (addr, info) in &members {
                    let member_share = if info.role == MemberRole::Founder {
                        share.checked_add(remainder).map_err(|_| ContractError::Overflow)?
                    } else {
                        share
                    };
                    DISSOLUTION_CLAIMS.save(
                        deps.storage,
                        (proposal.corp_id, addr),
                        &member_share,
                    )?;
                }
            }

            CORPORATIONS.save(deps.storage, proposal.corp_id, &corp)?;

            resp = resp.add_attribute("result", "dissolution_started");
        }

        ProposalType::Custom { title, .. } => {
            resp = resp
                .add_attribute("result", "custom_passed")
                .add_attribute("custom_title", title);
        }
    }

    Ok(resp.add_messages(msgs))
}

// ─── Claim Dissolution ────────────────────────────────────────────────

fn execute_claim_dissolution(
    deps: DepsMut,
    info: MessageInfo,
    corp_id: u64,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;

    if corp.status != CorporationStatus::Dissolving {
        return Err(ContractError::NothingToClaim);
    }

    let share = DISSOLUTION_CLAIMS
        .may_load(deps.storage, (corp_id, &info.sender))?
        .unwrap_or(Uint128::zero());

    if share.is_zero() {
        return Err(ContractError::NothingToClaim);
    }

    let config = load_config(deps.as_ref())?;

    // Remove claim and member
    DISSOLUTION_CLAIMS.remove(deps.storage, (corp_id, &info.sender));
    MEMBERS.remove(deps.storage, (corp_id, &info.sender));

    corp.member_count -= 1;
    corp.treasury_balance = corp
        .treasury_balance
        .checked_sub(share)
        .map_err(|_| ContractError::Overflow)?;

    // If all members claimed, mark as dissolved
    if corp.member_count == 0 {
        corp.status = CorporationStatus::Dissolved;
    }

    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    let msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: config.denom,
            amount: share,
        }],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "claim_dissolution")
        .add_attribute("corp_id", corp_id.to_string())
        .add_attribute("claimant", info.sender.to_string())
        .add_attribute("amount", share.to_string()))
}

// ─── Update Description (Founder only, no proposal) ──────────────────

fn execute_update_description(
    deps: DepsMut,
    info: MessageInfo,
    corp_id: u64,
    description: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let mut corp = load_corporation(deps.as_ref(), corp_id)?;
    assert_active(&corp)?;

    let member = assert_member(deps.as_ref(), corp_id, &info.sender)?;
    if member.role != MemberRole::Founder {
        return Err(ContractError::Unauthorized {
            role: "founder".to_string(),
        });
    }

    corp.description = description;
    CORPORATIONS.save(deps.storage, corp_id, &corp)?;

    Ok(Response::new()
        .add_attribute("action", "update_description")
        .add_attribute("corp_id", corp_id.to_string()))
}

// ─── Withdraw Fees (H-01) ─────────────────────────────────────────────

// FIX: H-01 — allow owner to withdraw surplus fees/deposits not tracked in any treasury
fn execute_withdraw_fees(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let config = load_config(deps.as_ref())?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
    if amount.is_zero() {
        return Err(ContractError::ZeroAmount);
    }

    // Query actual contract balance
    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.denom)?
        .amount;

    // Sum all tracked treasury balances across corporations
    let total_tracked: Uint128 = CORPORATIONS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .try_fold(Uint128::zero(), |acc, item| {
            let (_, corp) = item?;
            Ok::<_, cosmwasm_std::StdError>(acc.saturating_add(corp.treasury_balance))
        })?;

    let surplus = contract_balance.saturating_sub(total_tracked);
    if amount > surplus {
        return Err(ContractError::InsufficientSurplus {
            requested: amount.to_string(),
            available: surplus.to_string(),
        });
    }

    let msg = BankMsg::Send {
        to_address: config.owner.to_string(),
        amount: vec![Coin {
            denom: config.denom,
            amount,
        }],
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "withdraw_fees")
        .add_attribute("amount", amount.to_string())
        .add_attribute("surplus", surplus.to_string()))
}

// ─── Two-Step Owner Transfer (H-04) ──────────────────────────────────

fn execute_propose_owner(
    deps: DepsMut,
    info: MessageInfo,
    new_owner: String,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let config = load_config(deps.as_ref())?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
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

fn execute_accept_owner(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
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

fn execute_cancel_owner_transfer(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    reject_funds(&info)?; // FIX: M-08
    let config = load_config(deps.as_ref())?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {
            role: "owner".to_string(),
        });
    }
    if PENDING_OWNER.may_load(deps.storage)?.is_none() {
        return Err(ContractError::NoOwnerTransferPending);
    }

    PENDING_OWNER.remove(deps.storage);
    Ok(Response::new().add_attribute("action", "cancel_owner_transfer"))
}

// ─── Query ────────────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Corporation { corp_id } => query_corporation(deps, corp_id),
        QueryMsg::ListCorporations { start_after, limit } => {
            query_list_corporations(deps, start_after, limit)
        }
        QueryMsg::Members {
            corp_id,
            start_after,
            limit,
        } => query_members(deps, corp_id, start_after, limit),
        QueryMsg::MemberInfo { corp_id, address } => query_member_info(deps, corp_id, address),
        QueryMsg::Proposal { proposal_id } => query_proposal(deps, proposal_id),
        QueryMsg::Proposals {
            corp_id,
            start_after,
            limit,
        } => query_proposals(deps, corp_id, start_after, limit),
        QueryMsg::VoteStatus { proposal_id } => query_vote_status(deps, env, proposal_id),
        // FIX: H-04
        QueryMsg::PendingOwner {} => to_json_binary(&PENDING_OWNER.may_load(deps.storage)?),
    }
}

fn query_corporation(deps: Deps, corp_id: u64) -> StdResult<Binary> {
    let corp = CORPORATIONS.load(deps.storage, corp_id)?;
    to_json_binary(&CorporationResponse { corporation: corp })
}

fn query_list_corporations(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after.map(Bound::exclusive);

    let corporations: Vec<Corporation> = CORPORATIONS
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .map(|r| r.map(|(_, v)| v))
        .collect::<StdResult<_>>()?;

    to_json_binary(&CorporationsListResponse { corporations })
}

fn query_members(
    deps: Deps,
    corp_id: u64,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after
        .as_ref()
        .map(|s| deps.api.addr_validate(s))
        .transpose()?;
    let start_bound = start.as_ref().map(Bound::exclusive);

    let members: Vec<MemberEntry> = MEMBERS
        .prefix(corp_id)
        .range(deps.storage, start_bound, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .map(|r| {
            r.map(|(addr, info)| MemberEntry {
                address: addr.to_string(),
                role: info.role,
                joined_at: info.joined_at,
            })
        })
        .collect::<StdResult<_>>()?;

    to_json_binary(&MembersListResponse { members })
}

fn query_member_info(deps: Deps, corp_id: u64, address: String) -> StdResult<Binary> {
    let addr = deps.api.addr_validate(&address)?;
    let info = MEMBERS.may_load(deps.storage, (corp_id, &addr))?;

    to_json_binary(&MemberInfoResponse {
        is_member: info.is_some(),
        info,
    })
}

fn query_proposal(deps: Deps, proposal_id: u64) -> StdResult<Binary> {
    let proposal = PROPOSALS.load(deps.storage, proposal_id)?;
    to_json_binary(&ProposalResponse { proposal })
}

// FIX: M-07 — use CORP_PROPOSALS secondary index instead of full table scan
fn query_proposals(
    deps: Deps,
    corp_id: u64,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(30).min(100) as usize;
    let min_bound = start_after.map(Bound::exclusive);

    let proposals: Vec<Proposal> = CORP_PROPOSALS
        .prefix(corp_id)
        .keys(deps.storage, min_bound, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .map(|r| {
            let proposal_id = r?;
            PROPOSALS.load(deps.storage, proposal_id)
        })
        .collect::<StdResult<_>>()?;

    to_json_binary(&ProposalsListResponse { proposals })
}

fn query_vote_status(deps: Deps, env: Env, proposal_id: u64) -> StdResult<Binary> {
    let proposal = PROPOSALS.load(deps.storage, proposal_id)?;
    let corp = CORPORATIONS.load(deps.storage, proposal.corp_id)?;

    let voting_ended = env.block.time >= proposal.voting_ends_at;
    // FIX: H-02 — use snapshot member count for quorum evaluation
    let snapshot = proposal.member_count_snapshot;
    let quorum_reached = {
        let total_votes = proposal.yes_votes + proposal.no_votes;
        (total_votes as u64) * 10000 >= (snapshot as u64) * (corp.quorum_bps as u64)
    };
    let passed = check_proposal_passed(&proposal, snapshot, corp.quorum_bps);

    to_json_binary(&VoteStatusResponse {
        yes_votes: proposal.yes_votes,
        no_votes: proposal.no_votes,
        total_members: snapshot,
        quorum_bps: corp.quorum_bps,
        quorum_reached,
        passed,
        voting_ended,
    })
}

// ─── Migrate ──────────────────────────────────────────────────────────

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // FIX: H-02 + M-07 — backfill member_count_snapshot and CORP_PROPOSALS index
    // For existing proposals, use current corp member_count as best approximation.
    let all_proposals: Vec<(u64, Proposal)> = PROPOSALS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for (id, mut proposal) in all_proposals {
        // Backfill snapshot if zero (i.e., from pre-migration state)
        if proposal.member_count_snapshot == 0 {
            if let Ok(corp) = CORPORATIONS.load(deps.storage, proposal.corp_id) {
                proposal.member_count_snapshot = corp.member_count;
                PROPOSALS.save(deps.storage, id, &proposal)?;
            }
        }
        // Backfill CORP_PROPOSALS index
        CORP_PROPOSALS.save(deps.storage, (proposal.corp_id, id), &())?;
    }

    Ok(Response::new().add_attribute("action", "migrate"))
}
