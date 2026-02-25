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
            ExecuteMsg::Deposit {} => contract::execute_deposit(deps, env, info),
            ExecuteMsg::Withdraw {
                nonce,
                credit_amount,
                token_amount,
                signature,
            } => contract::execute_withdraw(deps, env, info, nonce, credit_amount, token_amount, signature),
            ExecuteMsg::FundTreasury {} => contract::execute_fund_treasury(deps, env, info),
            ExecuteMsg::WithdrawTreasury { amount } => {
                contract::execute_withdraw_treasury(deps, env, info, amount)
            }
            ExecuteMsg::ProposeOracle {
                new_oracle,
                new_pubkey,
            } => contract::execute_propose_oracle(deps, env, info, new_oracle, new_pubkey),
            ExecuteMsg::AcceptOracle {} => contract::execute_accept_oracle(deps, env, info),
            ExecuteMsg::CancelOracleTransfer {} => {
                contract::execute_cancel_oracle_transfer(deps, env, info)
            }
            ExecuteMsg::UpdateRate {
                rate_credits,
                rate_tokens,
            } => contract::execute_update_rate(deps, env, info, rate_credits, rate_tokens),
            ExecuteMsg::UpdateFee { fee_bps } => {
                contract::execute_update_fee(deps, env, info, fee_bps)
            }
            ExecuteMsg::UpdateLimits {
                player_daily_limit,
                global_daily_limit,
                cooldown_seconds,
                min_deposit,
                min_reserve,
            } => contract::execute_update_limits(
                deps,
                env,
                info,
                player_daily_limit,
                global_daily_limit,
                cooldown_seconds,
                min_deposit,
                min_reserve,
            ),
            ExecuteMsg::Pause {} => contract::execute_pause(deps, env, info),
            ExecuteMsg::Unpause {} => contract::execute_unpause(deps, env, info),
            // FIX: H-04
            ExecuteMsg::ProposeOwner { new_owner } => {
                contract::execute_propose_owner(deps, env, info, new_owner)
            }
            ExecuteMsg::AcceptOwner {} => contract::execute_accept_owner(deps, env, info),
            ExecuteMsg::CancelOwnerTransfer {} => {
                contract::execute_cancel_owner_transfer(deps, env, info)
            }
        }
    }

    #[entry_point]
    pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> cosmwasm_std::StdResult<Binary> {
        match msg {
            QueryMsg::Config {} => contract::query_config(deps),
            QueryMsg::TreasuryInfo {} => contract::query_treasury_info(deps, env),
            QueryMsg::PlayerInfo { address } => contract::query_player_info(deps, env, address),
            QueryMsg::NonceUsed { nonce } => contract::query_nonce_used(deps, nonce),
            QueryMsg::ConvertCreditsToTokens { credit_amount } => {
                contract::query_convert_credits_to_tokens(deps, credit_amount)
            }
            QueryMsg::ConvertTokensToCredits { token_amount } => {
                contract::query_convert_tokens_to_credits(deps, token_amount)
            }
            QueryMsg::PendingOracle {} => contract::query_pending_oracle(deps),
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
