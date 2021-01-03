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
    let address1 = HumanAddr::from("delegator1");
    let address2 = HumanAddr::from("delegator3");
    let address3 = HumanAddr::from("delegator2");
    let state = State {
        owner: deps.api.canonical_address(&info.sender)?,
        players: vec![deps.api.canonical_address(&address1).unwrap(), deps.api.canonical_address(&address2).unwrap(), deps.api.canonical_address(&address3).unwrap()],
        block: _env.block.height,
        everyBlockHeight: 0,
        denom: msg.denom,
        denomDelgation: msg.denomDelegation
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

    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denom {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denom.clone()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denom.clone())),
    }?;
    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    }

    // Ensure message sender is delegating some funds to the lottery validator
    let allDelegations = &deps.querier.query_all_delegations(&info.sender)?;
    if allDelegations.is_empty() {
        return Err(ContractError::NoDelegations{});
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
    }
    /*
        TODO: Probably we need to add a condition to check a minimum of staking coin to be able to participate a limit amount in the state
    */

    // Ensure message sender is sending the rights denom tickets and add register
    for coin in &info.sent_funds {
        for _ in 0..coin.amount.to_string().parse::<u64>().unwrap() {
            state.players.push(deps.api.canonical_address(&info.sender.clone())?);
        }
    }
    // Save the new state
    config(deps.storage).save(&state);

    Ok(HandleResponse::default())
}

pub fn handle_play(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {

    let mut state = config(deps.storage).load()?;
    // Ensure message sender is the owner of the contract
    let sender = deps.api.canonical_address(&info.sender).unwrap();
    if  sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }
    // Play the lottery
    let players = match state.players.len(){
        0 => Err(ContractError::NoPlayers {}),
        _ => Ok(&state.players)
    }?;
    let mut rng = rand::thread_rng();
    let maxRange = players.len() - 1;
    let range = rng.gen_range(0..maxRange);

    let winner = players[range].clone();

    let winnerDelegations = deps.querier.query_all_delegations(deps.api.human_address(&winner).unwrap()).unwrap();
    let winnerDelegation = winnerDelegations.iter().filter(|&e| e.validator == deps.api.human_address(&state.owner).unwrap()).cloned().collect::<Vec<Delegation>>();

    let winnerDelegationAmount = match winnerDelegation.len(){
        0 => Err(ContractError::NoDelegations {}),
        1 => {
            if info.sent_funds[0].denom == state.denomDelgation {
                Ok(&winnerDelegation[0].amount.amount)
            } else {
                Err(ContractError::MissingDenom(state.denom.clone()))
            }
        },
        _ => Err(ContractError::ExtraDelegation {})
    }?;

    println!("{}",  winnerDelegationAmount);

    for delegators in players{
        let delegations = deps.querier.query_all_delegations(deps.api.human_address(&delegators).unwrap()).unwrap();
        let delegation = delegations.iter().filter(|&e| e.validator == deps.api.human_address(&state.owner).unwrap()).cloned().collect::<Vec<Delegation>>();
        if !delegation.is_empty() {
            
            delegation[0].amount.amount
        }
    }

    //let winnerDelegation = deps.querier.query_delegation(deps.api.human_address( &winner).unwrap(), deps.api.human_address(&state.owner).unwrap())?;

    /*
        TODO: Compare the winner weight to others players
     */
    // Ensure there is no duplicate address in the players vector
    /*let mut dedupPlayers = players.clone().iter().map(|player| deps.api.human_address(player)).collect();
    dedupPlayers.sort_unstable();
    dedupPlayers.dedup();

    let mut totalDelegation: Uint128 = Uint128(0);
    for player in dedupPlayers {
        let delegation = deps.querier.query_delegation(deps.api.human_address( &player).unwrap(), deps.api.human_address(&state.owner).unwrap())?;
        totalDelegation += delegation.amount.amount;
    }*/
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
    use cosmwasm_storage::{bucket_read, bucket, singleton, singleton_read};
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

        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let init_msg = InitMsg {
            denom: "ujack".to_string(),
            denomDelegation: "uscrt".to_string()
        };
        let res = init(deps.as_mut(), mock_env(), info.clone(), init_msg).unwrap();

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
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
        FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator3"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }
        ]);

        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(10)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        //println!("{:?}", res);

        /*match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(ContractError::ExtraDenom(_e)) => (),
            Err(ContractError::MissingDenom(_e)) => (),
            Err(ContractError::NoFunds {}) => {},
            Err(ContractError::EmptyBalance {}) => {},
            Err(ContractError::NoDelegations{}) => {},
            _ => panic!("Unexpected error")
        }*/
        let res = query_config(deps.as_ref()).unwrap();
        println!("{:?}", res.players);

    }
    #[test]
    fn play(){
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let init_msg = InitMsg {
            denom: "ujack".to_string(),
            denomDelegation: "uscrt".to_string()
        };

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
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
            delegator: HumanAddr::from("delegator2"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(50_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
                delegator: HumanAddr::from("delegator2"),
                validator: HumanAddr::from("validator2"),
                amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(50_000_000)},
                can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
                accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
            },
            FullDelegation{
            delegator: HumanAddr::from("delegator3"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
        FullDelegation{
            delegator: HumanAddr::from("delegator3"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }
        ]);

        let res = init(deps.as_mut(), mock_env(), info.clone(), init_msg).unwrap();
        let res = handle_play(deps.as_mut(), mock_env(), info.clone());
        println!("{:?}", res);
        /*match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(ContractError::NoPlayers {}) => {},
            _ => panic!("Unexpected error")
        }*/

    }

}
