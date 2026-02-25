pub mod contract;
pub mod error;
pub mod helpers;
pub mod msg;
pub mod state;

#[cfg(not(feature = "library"))]
mod entry {
    use super::*;
    use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response};
    use msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};

    #[entry_point]
    pub fn instantiate(
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: InstantiateMsg,
    ) -> Result<Response, error::ContractError> {
        contract::instantiate(deps, env, info, msg)
    }

    #[entry_point]
    pub fn execute(
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: ExecuteMsg,
    ) -> Result<Response, error::ContractError> {
        match msg {
            ExecuteMsg::Mint {
                to,
                achievement_id,
                category,
                earned_at,
                description,
                rarity,
                token_uri,
                soulbound,
            } => contract::execute_mint(
                deps,
                env,
                info,
                to,
                achievement_id,
                category,
                earned_at,
                description,
                rarity,
                token_uri,
                soulbound,
            ),
            ExecuteMsg::BatchMint { mints } => {
                contract::execute_batch_mint(deps, env, info, mints)
            }
            ExecuteMsg::TransferNft {
                recipient,
                token_id,
            } => contract::execute_transfer_nft(deps, env, info, recipient, token_id),
            ExecuteMsg::SendNft {
                contract,
                token_id,
                msg,
            } => contract::execute_send_nft(deps, env, info, contract, token_id, msg),
            ExecuteMsg::Approve { spender, token_id } => {
                contract::execute_approve(deps, env, info, spender, token_id)
            }
            ExecuteMsg::Revoke { token_id } => contract::execute_revoke(deps, env, info, token_id),
            ExecuteMsg::ApproveAll { operator } => {
                contract::execute_approve_all(deps, env, info, operator)
            }
            ExecuteMsg::RevokeAll { operator } => {
                contract::execute_revoke_all(deps, env, info, operator)
            }
            ExecuteMsg::ProposeMinter { new_minter } => {
                contract::execute_propose_minter(deps, env, info, new_minter)
            }
            ExecuteMsg::AcceptMinter {} => contract::execute_accept_minter(deps, env, info),
            ExecuteMsg::CancelMinterTransfer {} => {
                contract::execute_cancel_minter_transfer(deps, env, info)
            }
            ExecuteMsg::Pause {} => contract::execute_pause(deps, env, info),
            ExecuteMsg::Unpause {} => contract::execute_unpause(deps, env, info),
            // FIX: L-02
            ExecuteMsg::Burn { token_id } => contract::execute_burn(deps, env, info, token_id),
            // FIX: H-04
            ExecuteMsg::ProposeOwner { new_owner } => {
                contract::execute_propose_owner(deps, env, info, new_owner)
            }
            ExecuteMsg::AcceptOwner {} => contract::execute_accept_owner(deps, env, info),
            ExecuteMsg::CancelOwnerTransfer {} => {
                contract::execute_cancel_owner_transfer(deps, env, info)
            }
            // FIX: I-01
            ExecuteMsg::SweepFunds { denom, amount, recipient } => {
                contract::execute_sweep_funds(deps, env, info, denom, amount, recipient)
            }
        }
    }

    #[entry_point]
    pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> cosmwasm_std::StdResult<Binary> {
        match msg {
            QueryMsg::Config {} => contract::query_config(deps),
            QueryMsg::NftInfo { token_id } => contract::query_nft_info(deps, token_id),
            QueryMsg::OwnerOf { token_id } => contract::query_owner_of(deps, token_id),
            QueryMsg::Tokens {
                owner,
                start_after,
                limit,
            } => contract::query_tokens(deps, owner, start_after, limit),
            QueryMsg::AllTokens {
                start_after,
                limit,
            } => contract::query_all_tokens(deps, start_after, limit),
            QueryMsg::NumTokens {} => contract::query_num_tokens(deps),
            QueryMsg::HasAchievement {
                owner,
                achievement_id,
            } => contract::query_has_achievement(deps, owner, achievement_id),
            QueryMsg::AchievementsByOwner {
                owner,
                start_after,
                limit,
            } => contract::query_achievements_by_owner(deps, owner, start_after, limit),
            QueryMsg::Approval { token_id, spender } => {
                contract::query_approval(deps, token_id, spender)
            }
            QueryMsg::Operator { owner, operator } => {
                contract::query_operator(deps, owner, operator)
            }
            QueryMsg::PendingMinter {} => contract::query_pending_minter(deps),
            // FIX: H-04
            QueryMsg::PendingOwner {} => contract::query_pending_owner(deps),
        }
    }

    #[entry_point]
    pub fn migrate(
        deps: DepsMut,
        env: Env,
        msg: MigrateMsg,
    ) -> Result<Response, error::ContractError> {
        contract::migrate(deps, env, msg)
    }
}
