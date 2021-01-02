use cosmwasm_std::{to_binary, Context, Api, Binary, Env, Deps, DepsMut, HandleResponse, InitResponse,
                   MessageInfo, Querier, StdResult, Storage, BankMsg, Coin, StakingMsg,
                   StakingQuery, StdError, AllDelegationsResponse, HumanAddr, Uint128, Delegation};

use crate::error::ContractError;
use crate::msg::{HandleMsg, InitMsg, QueryMsg, ConfigResponse};
use crate::state::{config, config_read, State};
use cosmwasm_std::testing::StakingQuerier;
use crate::error::ContractError::Std;
use std::fs::canonicalize;
use rand::Rng;
use schemars::_serde_json::map::Entry::Vacant;
use std::io::Stderr;



// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
pub fn init(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let state = State {
        owner: deps.api.canonical_address(&info.sender)?,
        players: vec![],
        everyBlockHeight: 0,
        denom: "ujack".to_string()
    };
    config(deps.storage).save(&state)?;
    Ok(InitResponse::default())
}


// And declare a custom Error variant for the ones where you will want to make use of it
pub fn handle(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: HandleMsg,
) -> Result<HandleResponse, ContractError> {
    match msg {
        HandleMsg::Register {} => handle_register(deps, _env, info),
        HandleMsg::Play {} => handle_play(deps, _env, info),
    }
}

pub fn handle_register(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {

    let mut state = config(deps.storage).load()?;
    let ownerAddress = deps.api.human_address( &state.owner)?;

    // Ensure message sender is sending some funds to buy the lottery
    if info.sent_funds.clone().is_empty() {
        //return Err(ContractError::Unauthorized {});
        return Err(ContractError::EmptyBalance {});
        //return Err(ContractError::Std(StdError::GenericErr { msg: "Error send funds please".to_string(), backtrace: Backtrace::disabled()  }))
        //return Err(ContractError::Std(StdError::GenericErr { msg: format!("send some {} to buy tickets lottery", state.denom), backtrace: Backtrace }));
    }

    // Ensure sender are not sending wrong coins
    for coin in &info.sent_funds {
        if coin.denom != state.denom{
            return Err(ContractError::EmptyBalance {});
            //return Err(ContractError::Std(StdError::GenericErr { msg: "Error send funds please 2".to_string(), backtrace: Backtrace::disabled()  }))
            // return Err(ContractError::Std(StdError::GenericErr { msg: format!("send only {} to buy tickets lottery", state.denom), backtrace: None }));
        }
    }


    // Ensure message sender is delegating some funds to the lottery validator
    let allDelegations = &deps.querier.query_all_delegations(&info.sender)?;
    if allDelegations.is_empty() {
        return Err(ContractError::EmptyBalance {});
        //return Err(ContractError::Std(StdError::GenericErr { msg: "Error send funds please 3".to_string(), backtrace: Backtrace::disabled()  }))
        //return Err(ContractError::Unauthorized {});
    }
    //let delegation = allDelegations.into_iter().filter(|&delegator| delegator.validator == ownerAddress).collect::<Delegation>();
    let mut delegator = vec![];
    for delegation in allDelegations {
        if delegation.validator == ownerAddress {
            delegator.push(delegation)
        }
    };
    if delegator.is_empty(){
        return Err(ContractError::EmptyBalance {});
        //return Err(ContractError::Std(StdError::GenericErr { msg: "Error send funds please 4".to_string(), backtrace: Backtrace::disabled() }))
        //return Err(ContractError::Unauthorized {});
    }
    /*
        TODO: Probably we need to add a condition to check a minimum of staking coin to be able to participate a limit amount in the state
    */
    /*if delegator[0].amount.amount.is_zero() {
        return Err(ContractError::Unauthorized {});
        //return Err(ContractError::Std(StdError::GenericErr { msg: "you need to delegate some funds to the validator first".to_string(), backtrace: () }));
    }*/

    // Ensure message sender is sending the rights denom tickets and add register
    for coin in &info.sent_funds {
        for _ in 0..coin.amount.to_string().parse::<u64>().unwrap() {
            state.players.push(deps.api.canonical_address(&info.sender.clone())?);
        }
    }
    // Save the state
    config(deps.storage).save(&state);

    Ok(HandleResponse::default())
}

pub fn handle_play(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    /*
    let mut state = config(&mut Storage).load()?;
    // Ensure message sender is the owner of the contract
    if  info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }
    // Play the lottery
    let players = state.players;
    let mut rng = rand::thread_rng();
    let maxRange = players.len() - 1;
    let range = rng.gen_range(0..maxRange);

    let winner = players[range].clone();
    let winnerDelegation = deps.querier.query_delegation(deps.api.human_address( &winner)?, deps.api.human_address(&state.owner)?)??;

    /*
        TODO: Compare the winner weight to others players
     */
    // Ensure there is no duplicate address in the players vector
    let mut dedupPlayers = players.clone();
    dedupPlayers.sort_unstable();
    dedupPlayers.dedup();

    let mut totalDelegation: Uint128 = Uint128(0);
    for player in dedupPlayers {
        let delegation = deps.querier.query_delegation(deps.api.human_address( &player)?, deps.api.human_address(&state.owner)?)??;
        totalDelegation += delegation.amount.amount;
    }
    // Jackpot winner weight due
    //let jackpotWeight = winnerDelegation.amount.amount * Uint128(100) / totalDelegation;
    /*
        TODO: Calcule the amount to send to the winner
     */


    /*
        TODO: Withdraw rewards and commissions
     */

    /*
        TODO: Check the balance amount
     */

    /*
        TODO: Send a transaction to the winner
     */
*/
    Ok(HandleResponse::default())
}

pub fn query(
    deps: Deps,
    _env: Env,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let state = config_read(deps.storage).load()?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, BankQuerier, MOCK_CONTRACT_ADDR, MockStorage, StakingQuerier};
    use cosmwasm_std::{coins, from_binary, Validator, Decimal, FullDelegation, HumanAddr, Uint128, MessageInfo, StdError, Storage, Api, CanonicalAddr};
    use std::collections::HashMap;
    use std::borrow::Borrow;
    use cosmwasm_storage::{bucket_read, bucket, singleton, singleton_read, };
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use crate::msg::{HandleMsg, InitMsg, QueryMsg};



    #[test]
    fn proper_init (){
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        let info = mock_info(HumanAddr::from("owner"), &[]);
        let init_msg = InitMsg {
            denom: "ujack".to_string()
        };
        let res = init(deps.as_mut(), mock_env(), info, init_msg).unwrap();
        assert_eq!(0, res.messages.len());

        let res = query_config(deps.as_ref()).unwrap();

        assert_eq!( "00000000006E00000000007772000000006F650000000000", CanonicalAddr::to_string(&res.owner));

    }
    #[test]
    fn register() {

        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }]);

        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(10)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info);
        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

    }

/*    #[test]
    fn run_lottery(){
        let environment= mock_env();
        let info = MessageInfo{ sender: HumanAddr::from(MOCK_CONTRACT_ADDR), sent_funds: vec![]};

        /*let data = StakingQuerier::new("uscrt", &[Validator{
            address: HumanAddr::from("secretvaloper19qq99fyzrx3wsrxj894uhahy3s3tlaqs68a34s"),
            commission: Decimal( 100_000_000 ),
            max_commission: Decimal(20_000_000_000),
            max_change_rate: Decimal(1_000_000_000)
        }], &[FullDelegation{
            delegator: HumanAddr::from("secret1sqd7j9x2dm74jx7mdwfvkx7l6jhnu0x6yxzspj"),
            validator: HumanAddr::from("secretvaloper19qq99fyzrx3wsrxj894uhahy3s3tlaqs68a34s"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0) },
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(500_000)}]
        },
        FullDelegation{
            delegator: HumanAddr::from("secret1sqd7j9x2dm74jx7mdwfvkx7l6jhnu0x6yxzspj"),
            validator: HumanAddr::from("secretvaloper19qq99fyzrx3wsrxj894uhahy3s3tlaqs68a34s"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0) },
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(500_000)}]
        }
        ]);*/


        let mut errorMessage = false;
        // Check if MessageInfo is from contract owner and prevent sent_funds to the contract
        if !info.sent_funds.is_empty() || info.sender != environment.contract.address {
            StdError::GenericErr { msg: "don't send funds with lottery".to_string(), backtrace: None };
            errorMessage = true
        }

        assert_eq!(false, errorMessage);

        // Withdraw staking funds
        let stakingWithdraw = StakingMsg::Withdraw { validator: HumanAddr::from(MOCK_CONTRACT_ADDR), recipient: None };
        //Ok(stakingWithdraw);
        println!("{:?}", stakingWithdraw);

        println!("{:?}", environment)
    }*/
    /*#[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InitMsg { count: 17 };
        let info = mock_info("creator", &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = init(&mut deps, mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(&deps, mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(17, value.count);
    }*/

    /*#[test]
    fn increment() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InitMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = init(&mut deps, mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let info = mock_info("anyone", &coins(2, "token"));
        let msg = HandleMsg::Increment {};
        let _res = handle(&mut deps, mock_env(), info, msg).unwrap();

        // should increase counter by 1
        let res = query(&deps, mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(18, value.count);
    }

    #[test]
    fn reset() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InitMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = init(&mut deps, mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let unauth_info = mock_info("anyone", &coins(2, "token"));
        let msg = HandleMsg::Reset { count: 5 };
        let res = handle(&mut deps, mock_env(), unauth_info, msg);
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Must return unauthorized error"),
        }

        // only the original creator can reset the counter
        let auth_info = mock_info("creator", &coins(2, "token"));
        let msg = HandleMsg::Reset { count: 5 };
        let _res = handle(&mut deps, mock_env(), auth_info, msg).unwrap();

        // should now be 5
        let res = query(&deps, mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(5, value.count);
    }*/
}
