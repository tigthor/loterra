use cosmwasm_std::{to_binary, Context, Api, Binary, Env, Extern, HandleResponse, InitResponse, MessageInfo, Querier, StdResult, Storage, BankMsg, Coin, StakingMsg, StakingQuery, StdError, AllDelegationsResponse, HumanAddr};

use crate::error::ContractError;
use crate::msg::{CountResponse, HandleMsg, InitMsg, QueryMsg};
use crate::state::{config, config_read, State};
use cosmwasm_std::testing::StakingQuerier;
use crate::error::ContractError::Std;
use std::fs::canonicalize;
use rand::Rng;

// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let state = State {
        owner: deps.api.canonical_address(&_env.contract.address)?,
        players: vec![],
        everyBlockHeight: 0,
        denom: "ulot".to_string()
    };
    config(&mut deps.storage).save(&state)?;
    Ok(InitResponse::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo,
    msg: HandleMsg,
) -> Result<HandleResponse, ContractError> {
    match msg {
        HandleMsg::Register {} => handle_register(deps, _env, info),
        HandleMsg::Play {} => handle_play(deps, _env, info),
    }
}

pub fn handle_register<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Ensure message sender is the owner
    let mut state = config(&mut Storage).load()?;
    if _env.contract.address != state.owner {
        return Err(ContractError::Unauthorized {});
    }
    // Ensure message sender is delegating some funds to the lottery validator
    //let delegations = deps.querier.query_delegation(delegator: info.sender, validator: _env.contract.address).clone()?;
    let delegations = StakingQuery::Delegation { delegator: info.sender, validator: _env.contract.address };

    deps.querier.query_delegation();
    if delegations.amount <= 0 {
        return Err(ContractError::Std(StdError::GenericErr { msg: "you need to delegate some funds to the validator first".to_string(), backtrace: None }));
    }
    // Ensure message sender is sending some funds to buy the lottery
    if info.sent_funds.is_empty() {
        return Err(ContractError::Std(StdError::GenericErr { msg: format!("send some {} to buy tickets lottery", state.denom), backtrace: None }));
    }
    // Ensure message sender is sending the rights denom tickets and add register
    for coin in info.sent_funds {
        if coin.denom != state.denom{
            return Err(ContractError::Std(StdError::GenericErr { msg: format!("send only {} to buy tickets lottery", state.denom), backtrace: None }));
        }
        /*
            TODO: Ensure the tickets are Integers and not Decimals
        */
        for _ in 0..coin.amount.to_string().parse::<u64>().unwrap() {
            state.players.push(deps.api.canonical_address(&info.sender.clone())?);
        }
    }
    // Save the state
    config(&mut Storage).save(&state);

    Ok(HandleResponse::default())
}

pub fn handle_play<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    let mut state = config(&mut Storage).load()?;
    // Ensure message sender is the owner of the contract
    if  info.sender != _env.contract.address {
        return Err(ContractError::Unauthorized {});
    }
    // Play the lottery
    let players = state.players;
    let mut range = rand::thread_rng();
    let maxRange = players.len() - 1;
    range.gen_range(0..maxRange);
    /*
        TODO: Compare the winner weight to others players
     */
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

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    _env: Env,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_config<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<ConfigResponse> {
    let state = config_read(&deps.storage).load()?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, BankQuerier, MOCK_CONTRACT_ADDR, MockStorage};
    use cosmwasm_std::{coins, from_binary, Validator, Decimal, FullDelegation, HumanAddr, Uint128, MessageInfo, StdError, Storage};
    use std::collections::HashMap;
    use std::borrow::Borrow;
    use cosmwasm_storage::{bucket_read, bucket, singleton, singleton_read, };
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use crate::msg::{CountResponse, HandleMsg, InitMsg, QueryMsg};
    #[test]
    fn test_participate_lottery() {
        let mut deps = mock_dependencies(&[]);
        let env = mock_env();
        let info = mock_info("rico", &[Coin{ denom: "ux".to_string(), amount: Uint128(2)}]);
        //Init the data
        #[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, PartialEq)]
        struct Data {
            players: Vec<HumanAddr>,
            denom: String
        }
        let mut store = MockStorage::new();
        let mut singleton = singleton::<_, Data>(&mut store, b"data");

        let mut obj = Data{
            players: vec![HumanAddr::from("hello1")],
            denom: "uchimere".to_string()
        };

        //let info = MessageInfo{ sender: HumanAddr::from("sender"), sent_funds: vec![Coin{ denom: "uchimere".to_string(), amount: Uint128(3)}]};

        //try_participate_lottery(&mut deps, env, info, HandleMsg::);

        //Check if sender is a delegator
        /*
            Todo: Check if the sender is a Delegator
         */
/*
        if info.sent_funds.is_empty(){
            StdError::GenericErr { msg: "you need at least one ticket to play the lottery".to_string(), backtrace: None };
        }
        // Add participation
        for x in info.sent_funds {
            if x.denom != "uchimere".to_string(){
                StdError::GenericErr { msg: format!("only is send {}", "uchimere".to_string()), backtrace: None };
            }
            for y in 0..x.amount.to_string().parse::<i32>().unwrap() {
                obj.players.push(info.sender.clone());
            }

            println!("{:?}", x);
        }

        singleton.save(&obj);
        let reader2 = singleton_read::<_, Data>(&mut store, b"data");
        let state2 = reader2.load();
        println!("{:?}", state2);*/
    }
    #[test]
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
    }
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
