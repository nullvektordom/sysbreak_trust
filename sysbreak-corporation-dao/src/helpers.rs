use cosmwasm_std::{Addr, Deps, Env, MessageInfo, Uint128};

use crate::error::ContractError;
use crate::state::{
    CorporationStatus, Config, Corporation, MemberInfo, MemberRole, Proposal,
    ProposalStatus, CONFIG, CORPORATIONS, MEMBERS,
};

/// Load config or return StdError
pub fn load_config(deps: Deps) -> Result<Config, ContractError> {
    Ok(CONFIG.load(deps.storage)?)
}

/// Load a corporation or return CorporationNotFound
pub fn load_corporation(deps: Deps, corp_id: u64) -> Result<Corporation, ContractError> {
    CORPORATIONS
        .load(deps.storage, corp_id)
        .map_err(|_| ContractError::CorporationNotFound { id: corp_id })
}

/// Assert the corporation is Active
pub fn assert_active(corp: &Corporation) -> Result<(), ContractError> {
    match corp.status {
        CorporationStatus::Active => Ok(()),
        CorporationStatus::Dissolving => Err(ContractError::Dissolving),
        CorporationStatus::Dissolved => Err(ContractError::Dissolved),
    }
}

/// Assert the corporation is not Dissolved (allows Dissolving for claims)
pub fn assert_not_dissolved(corp: &Corporation) -> Result<(), ContractError> {
    match corp.status {
        CorporationStatus::Dissolved => Err(ContractError::Dissolved),
        _ => Ok(()),
    }
}

/// Load member info or return NotMember
pub fn load_member(
    deps: Deps,
    corp_id: u64,
    addr: &Addr,
) -> Result<MemberInfo, ContractError> {
    MEMBERS
        .load(deps.storage, (corp_id, addr))
        .map_err(|_| ContractError::NotMember { corp_id })
}

/// Assert caller is a member and return their info
pub fn assert_member(
    deps: Deps,
    corp_id: u64,
    sender: &Addr,
) -> Result<MemberInfo, ContractError> {
    load_member(deps, corp_id, sender)
}

/// Assert caller is founder or officer
pub fn assert_officer_or_founder(
    deps: Deps,
    corp_id: u64,
    sender: &Addr,
) -> Result<MemberInfo, ContractError> {
    let info = load_member(deps, corp_id, sender)?;
    match info.role {
        MemberRole::Founder | MemberRole::Officer => Ok(info),
        MemberRole::Member => Err(ContractError::Unauthorized {
            role: "officer or founder".to_string(),
        }),
    }
}

/// Validate that exactly one coin of the correct denom and exact amount was sent.
// FIX: M-01 — reject overpayment (changed from >= to == check)
pub fn validate_funds(
    info: &MessageInfo,
    denom: &str,
    expected_amount: Uint128,
    err_insufficient: ContractError,
) -> Result<Uint128, ContractError> {
    if info.funds.is_empty() {
        return Err(ContractError::NoFundsSent);
    }
    if info.funds.len() > 1 {
        return Err(ContractError::MultipleDenomsSent);
    }
    let coin = &info.funds[0];
    if coin.denom != denom {
        return Err(ContractError::WrongDenom {
            expected: denom.to_string(),
            got: coin.denom.clone(),
        });
    }
    if coin.amount < expected_amount {
        return Err(err_insufficient);
    }
    if coin.amount > expected_amount {
        return Err(ContractError::OverpaymentNotAllowed {
            expected: expected_amount.to_string(),
            got: coin.amount.to_string(),
        });
    }
    Ok(coin.amount)
}

/// Validate that exactly one coin of the correct denom was sent, with at least min_amount.
/// Used for donations and other variable-amount payments.
pub fn validate_funds_min(
    info: &MessageInfo,
    denom: &str,
    min_amount: Uint128,
    err_insufficient: ContractError,
) -> Result<Uint128, ContractError> {
    if info.funds.is_empty() {
        return Err(ContractError::NoFundsSent);
    }
    if info.funds.len() > 1 {
        return Err(ContractError::MultipleDenomsSent);
    }
    let coin = &info.funds[0];
    if coin.denom != denom {
        return Err(ContractError::WrongDenom {
            expected: denom.to_string(),
            got: coin.denom.clone(),
        });
    }
    if coin.amount < min_amount {
        return Err(err_insufficient);
    }
    Ok(coin.amount)
}

// FIX: M-08 — reject unexpected funds on handlers that should not accept any
pub fn reject_funds(info: &MessageInfo) -> Result<(), ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::UnexpectedFunds);
    }
    Ok(())
}

// FIX: M-02 — validate governance parameters
pub fn validate_quorum_bps(bps: u16) -> Result<(), ContractError> {
    if bps == 0 || bps > 10_000 {
        return Err(ContractError::InvalidQuorumBps { value: bps });
    }
    Ok(())
}

pub fn validate_voting_period(seconds: u64) -> Result<(), ContractError> {
    if seconds < 3600 || seconds > 2_592_000 {
        return Err(ContractError::InvalidVotingPeriod { value: seconds });
    }
    Ok(())
}

/// Check that a proposal's voting period has ended
pub fn assert_voting_ended(proposal: &Proposal, env: &Env) -> Result<(), ContractError> {
    if env.block.time < proposal.voting_ends_at {
        return Err(ContractError::VotingNotEnded { id: proposal.id });
    }
    Ok(())
}

/// Check that a proposal's voting period has NOT ended
pub fn assert_voting_active(proposal: &Proposal, env: &Env) -> Result<(), ContractError> {
    if env.block.time >= proposal.voting_ends_at {
        return Err(ContractError::VotingEnded { id: proposal.id });
    }
    if proposal.status != ProposalStatus::Active {
        return Err(ContractError::ProposalNotPending { id: proposal.id });
    }
    Ok(())
}

/// Determine if a proposal passed based on votes and quorum
pub fn check_proposal_passed(
    proposal: &Proposal,
    total_members: u32,
    quorum_bps: u16,
) -> bool {
    if total_members == 0 {
        return false;
    }
    let total_votes = proposal.yes_votes + proposal.no_votes;
    // Quorum check: total_votes * 10000 >= total_members * quorum_bps
    let quorum_reached =
        (total_votes as u64) * 10000 >= (total_members as u64) * (quorum_bps as u64);
    // Majority check: yes > no
    quorum_reached && proposal.yes_votes > proposal.no_votes
}

/// Check dissolution supermajority (75%)
pub fn check_dissolution_supermajority(
    yes_votes: u32,
    total_members: u32,
) -> Result<(), ContractError> {
    if total_members == 0 {
        return Err(ContractError::DissolutionSupermajorityNotReached { pct: 0 });
    }
    // 75% of total members must vote yes
    let pct = (yes_votes as u64) * 100 / (total_members as u64);
    if (yes_votes as u64) * 100 < (total_members as u64) * 75 {
        return Err(ContractError::DissolutionSupermajorityNotReached { pct });
    }
    Ok(())
}
