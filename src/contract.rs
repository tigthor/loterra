use cosmwasm_std::{to_binary, attr, Context, Api, Binary, Env, Deps, DepsMut, HandleResponse, InitResponse, MessageInfo, Querier, StdResult, Storage, BankMsg, Coin, StakingMsg, StakingQuery, StdError, AllDelegationsResponse, HumanAddr, Uint128, Delegation, Decimal, BankQuery};

use crate::error::ContractError;
use crate::msg::{HandleMsg, InitMsg, QueryMsg, ConfigResponse};
use crate::state::{config, config_read, State};
use cosmwasm_std::testing::StakingQuerier;
use crate::error::ContractError::Std;
use std::fs::canonicalize;
use rand::Rng;
use schemars::_serde_json::map::Entry::Vacant;
use std::io::Stderr;
use std::ops::{Mul, Sub};

// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
// #[serde(rename_all = "snake_case")]
pub fn init(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> StdResult<InitResponse> {

    let state = State {
        owner: deps.api.canonical_address(&info.sender)?,
        players: msg.players,
        blockPlay: msg.blockPlay,
        blockClaim: msg.blockClaim,
        blockIcoTimeframe: msg.blockIcoTimeframe,
        everyBlockHeight: msg.everyBlockHeight,
        denom: msg.denom,
        denomDelegation: msg.denomDelegation,
        denomShare: msg.denomShare,
        claimTicket: msg.claimTicket,
        claimReward: msg.claimReward,
        holdersRewards: msg.holdersRewards,
        tokenHolderSupply: msg.tokenHolderSupply
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
        HandleMsg::Claim {} => handle_claim(deps, _env, info),
        HandleMsg::Ico {} => handle_ico(deps, _env, info),
        HandleMsg::Buy {} => handle_buy(deps, _env, info),
        HandleMsg::Reward {} => handle_reward(deps, _env, info)
    }
}

pub fn handle_register(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // Check if some funds are sent
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denom {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denom.clone().to_string()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denom.clone().to_string())),
    }?;
    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    }

    let ticketNumber = sent.u128();
    for d in 0..ticketNumber {
        // Update the state
        state.players.push(deps.api.canonical_address(&info.sender.clone())?);
    }

    // Save the new state
    config(deps.storage).save(&state);

    Ok(HandleResponse::default())
}

pub fn handle_claim(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // convert the sender to canonical address
    let sender = deps.api.canonical_address(&info.sender).unwrap();
    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Claim".to_string()));
    }

    if _env.block.height > state.blockClaim {
        state.claimTicket = vec![];
        state.blockClaim = _env.block.height + state.everyBlockHeight;
    }
    // Ensure sender is a delegator
    let isDelegator = deps.querier.query_all_delegations(&info.sender)?;
    if isDelegator.is_empty() {
        return Err(ContractError::NoDelegations {})
    }
    // Ensure sender only can claim one time every x blocks
    if state.claimTicket.iter().any(|address| deps.api.human_address(address).unwrap() == info.sender){
        return Err(ContractError::AlreadyClaimed {});
    }
    // Add the sender to claimed state
    state.claimTicket.push(sender.clone());

    // Get the contract balance
    let balance = deps.querier.query_balance(_env.contract.address.clone(), &state.denom)?;
    // Cancel if no amount in the contract
    if balance.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }

    // Save the new state
    config(deps.storage).save(&state);

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin{ denom: state.denom.clone(), amount: Uint128(1)}]
    };
    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "claim"), attr("to", &sender)],
        data: None,
    })
}


pub fn handle_play(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;

    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Play".to_string()));
    }
    // Make the contract callable for everyone every x blocks
    if _env.block.height > state.blockPlay {
        // Update the state
        state.claimReward = vec![];
        state.blockPlay = _env.block.height + state.everyBlockHeight;
    }else{
        return Err(ContractError::Unauthorized {});
    }
    // Sort the winner
    let players = match state.players.len(){
        0 => Err(ContractError::NoPlayers {}),
        _ => Ok(&state.players)
    }?;
    let mut rng = rand::thread_rng();
    let maxRange = players.len() - 1;
    let winnerRange = rng.gen_range(0..maxRange);
    let winnerAddress = players[winnerRange].clone();

    // Winning 5 to 99 % of the lottery
    let percentOfJackpot = rng.gen_range(5..99);

    // Get the contract balance
    let balance = deps.querier.query_balance(&_env.contract.address, &state.denomDelegation).unwrap();
    // Calculate the gross price
    let jackpotGross = balance.amount.mul( Decimal::percent(percentOfJackpot));
    // Calculate the fees for rewarding tokens holders
    let jackpotFee = jackpotGross.mul(Decimal::percent(10));
    // Jackpot winner after fees
    let jackpot = jackpotGross - jackpotFee;

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&winnerAddress).unwrap(),
        amount: vec![Coin{ denom: state.denomDelegation.clone(), amount: jackpot.unwrap()}]
    };
    // Update the state
    state.holdersRewards = jackpotFee;
    state.players = vec![];

    // Save the new state
    config(deps.storage).save(&state);

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "jackpot"), attr("to", deps.api.human_address(&winnerAddress).unwrap())],
        data: None,
    })
}

pub fn handle_ico(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // Ico expire after blocktime
    if state.blockIcoTimeframe < _env.block.height{
        return Err(ContractError::TheIcoIsEnded {});
    }
    // Check if some funds are sent
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomDelegation {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denomDelegation.clone()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denomDelegation.clone())),
    }?;

    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    };
    // Get the contract balance prepare the tx
    let balance = deps.querier.query_balance(&_env.contract.address, &state.denomShare).unwrap();
    if balance.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }

    if balance.amount.u128() < sent.u128() {
        return Err(ContractError::EmptyBalance {});
    }

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin{ denom: state.denomShare.clone(), amount: sent}]
    };

    state.tokenHolderSupply += sent;
    // Save the new state
    config(deps.storage).save(&state);

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "ico"), attr("to", &info.sender)],
        data: None,
    })
}

pub fn handle_buy(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;

    // Get the funds
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomDelegation {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denomDelegation.clone()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denomDelegation.clone())),
    }?;

    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    };

    let balance = deps.querier.query_balance(&_env.contract.address, &state.denom).unwrap();
    if balance.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }

    if balance.amount.u128() < sent.u128() {
        return Err(ContractError::EmptyBalance {});
    }

    let amountToSend = sent.u128() / 1_000_000;

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin{ denom: state.denom.clone(), amount: Uint128(amountToSend)}]
    };

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "ticket"), attr("to", &info.sender)],
        data: None,
    })
}

pub fn handle_reward(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // convert the sender to canonical address
    let sender = deps.api.canonical_address(&info.sender).unwrap();
    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Reward".to_string()));
    }
    if state.tokenHolderSupply.is_zero() {
        return Err(ContractError::Unauthorized {});
    }

    // Ensure sender have some reward tokens
    let balanceSender = deps.querier.query_balance(info.sender.clone(), &state.denomShare).unwrap();
    if balanceSender.amount.is_zero(){
        return Err(ContractError::Unauthorized {});
    }

    // Ensure sender only can claim one time every x blocks
    if state.claimReward.iter().any(|address| deps.api.human_address(address).unwrap() == info.sender.clone()){
        return Err(ContractError::AlreadyClaimed {});
    }
    // Add the sender to claimed state
    state.claimReward.push(sender.clone());

    // Get the contract balance
    let balanceContract = deps.querier.query_balance(_env.contract.address.clone(), &state.denomDelegation)?;
    // Cancel if no amount in the contract
    if balanceContract.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }
    // Get the percentage of shareholder
    let shareHolderPercentage = balanceSender.amount.u128() as u64 * 100 / state.tokenHolderSupply.u128() as u64;
    if shareHolderPercentage == 0 {
        return Err(ContractError::SharesTooLow {});
    }
    // Calculate the reward
    let reward = state.holdersRewards.mul(Decimal::percent(shareHolderPercentage));
    // Update the holdersReward
    state.holdersRewards =  state.holdersRewards.sub(reward).unwrap();
    // Save the new state
    config(deps.storage).save(&state);

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin{ denom: state.denomDelegation.clone(), amount: reward}]
    };
    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "reward"), attr("to", &sender)],
        data: None,
    })
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
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, BankQuerier, MOCK_CONTRACT_ADDR, MockStorage, StakingQuerier, MockApi, MockQuerier};
    use cosmwasm_std::{coins, from_binary, Validator, Decimal, FullDelegation, HumanAddr, Uint128, MessageInfo, StdError, Storage, Api, CanonicalAddr, OwnedDeps, CosmosMsg, QuerierResult};
    use std::collections::HashMap;
    use std::borrow::Borrow;
    use cosmwasm_storage::{bucket_read, bucket, singleton, singleton_read};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use crate::msg::{HandleMsg, InitMsg, QueryMsg};




    fn default_init(deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>, staking: bool) {
        let player1 = deps.api.canonical_address(&HumanAddr::from("player1")).unwrap();
        let player2 = deps.api.canonical_address(&HumanAddr::from("player2")).unwrap();
        let player3 = deps.api.canonical_address(&HumanAddr::from("player3")).unwrap();
        let player4 = deps.api.canonical_address(&HumanAddr::from("player4")).unwrap();
        let player5 = deps.api.canonical_address(&HumanAddr::from("player5")).unwrap();
        let player6 = deps.api.canonical_address(&HumanAddr::from("player6")).unwrap();

        const DENOM: &str = "ujack";
        const DENOM_DELEGATION: &str = "uscrt";
        const DENOM_SHARE: &str = "upot";
        const EVERY_BLOCK_EIGHT: u64 = 100;
        let PLAYERS: Vec<CanonicalAddr> = vec![player1, player2.clone(), player2.clone(), player3.clone(), player3.clone(), player3.clone(), player3.clone(), player4.clone(), player4.clone(), player4.clone(), player5, player6];
        const CLAIM_TICKET: Vec<CanonicalAddr> = vec![];
        const CLAIM_REWARD: Vec<CanonicalAddr> = vec![];
        const BLOCK_PLAY: u64 = 0;
        const BLOCK_CLAIM: u64 = 0;
        const BLOCK_ICO_TIME_FRAME: u64 = 1000000000;
        const HOLDERS_REWARDS: Uint128 = Uint128(5_221);
        const TOKEN_HOLDER_SUPPLY: Uint128 = Uint128(10_000_000);

        let init_msg = InitMsg{
            denom: DENOM.to_string(),
            denomDelegation: DENOM_DELEGATION.to_string(),
            denomShare: DENOM_SHARE.to_string(),
            everyBlockHeight: EVERY_BLOCK_EIGHT,
            players: PLAYERS,
            claimTicket: CLAIM_TICKET,
            claimReward: CLAIM_REWARD,
            blockPlay: BLOCK_PLAY,
            blockClaim: BLOCK_PLAY,
            blockIcoTimeframe: BLOCK_ICO_TIME_FRAME,
            holdersRewards: HOLDERS_REWARDS,
            tokenHolderSupply: TOKEN_HOLDER_SUPPLY
        };
        let info = mock_info(HumanAddr::from("owner"), &[]);
        if staking {
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
        }
        init(deps.as_mut(), mock_env(), info, init_msg).unwrap();
    }

    #[test]
    fn proper_init (){
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, false);
        //println!("{}", 1432439234 % 100);
    }
    #[test]
    fn register() {

        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, false);

        // Test if this succeed
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(3)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        assert_eq!(0, res.unwrap().messages.len());
        // Test if we have added 3 times the player in the players array
        let res = query_config(deps.as_ref()).unwrap();
        assert_eq!(15, res.players.len());

        // Test sending 0 ticket NoFunds
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(0)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::NoFunds {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test sending 0 coins NoFunds
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::NoFunds {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test sending another coin
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::MissingDenom (msg)) => {
                assert_eq!(msg, "ujack".to_string())
            },
            _ => panic!("Unexpected error")
        }
        // Test sending more tokens than admitted
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(3)}, Coin{ denom: "uscrt".to_string(), amount: Uint128(2)}]);
        let res = handle_register(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::ExtraDenom(msg)) => {
                assert_eq!(msg, "ujack".to_string())
            },
            _ => panic!("Unexpected error")
        }
    }
    #[test]
    fn claim() {

        // Test handle do not send funds
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, true);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(3)}]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::DoNotSendFunds(msg)) => {
                assert_eq!(msg, "Claim".to_string())
            },
            _ => panic!("Unexpected error")
        };

        // Test error to claim if you are not staking
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::NoDelegations {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test error no funds in the contract
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, true);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test success claim
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps, true);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone()).unwrap();
        assert_eq!(1, res.messages.len());
        assert_eq!(res.messages[0], CosmosMsg::Bank(BankMsg::Send {
            from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
            to_address: HumanAddr::from("delegator1"),
            amount: vec![Coin{ denom: "ujack".to_string(), amount: Uint128(1) }]
        }));
        // Test if state have changed correctly
        let res = query_config(deps.as_ref()).unwrap();
        let claimer = deps.api.canonical_address(&HumanAddr::from("delegator1")).unwrap();
        // Test we added the claimer to the claimed vector
        assert!(res.claimTicket.len() > 0);
        // Test we added correct claimer to the claimed vector
        assert!(res.claimTicket.contains(&claimer));
        // Test error to claim two times
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::AlreadyClaimed {}) => {},
            _ => panic!("Unexpected error")
        }
    }

    #[test]
    fn ico (){
        //Test if balance is empty
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(0)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test if sender sent more funds than the balance can send
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(1_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending some correct funds
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(1_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "other".to_string(), amount: Uint128(2_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::MissingDenom (msg)) => {
                assert_eq!(msg, "uscrt")
            },
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending more than one tokens
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(1_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2_000_000)}, Coin{ denom: "other".to_string(), amount: Uint128(2_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::ExtraDenom (msg)) => {
                assert_eq!(msg, "uscrt")
            },
            _ => panic!("Unexpected error")
        }

        // Test success
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(10_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone()).unwrap();
        assert_eq!(1, res.messages.len());
        assert_eq!(res.messages[0], CosmosMsg::Bank(BankMsg::Send {
            from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
            to_address: HumanAddr::from("delegator1"),
            amount: vec![Coin{ denom: "upot".to_string(), amount: Uint128(2_000_000) }]
        }));
        // Test if state have changed correctly
        let res = query_config(deps.as_ref()).unwrap();
        assert_ne!(0, res.tokenHolderSupply.u128());
        assert_eq!(12_000_000, res.tokenHolderSupply.u128());

        // Test if state have changed correctly
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone()).unwrap();
        let res = query_config(deps.as_ref()).unwrap();
        // Test if tokenHolderSupply incremented correctly after multiple buys
        assert_eq!(14_000_000, res.tokenHolderSupply.u128());
    }
    #[test]
    fn buy () {
        //Test if balance is empty
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(0) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(1_000_000) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test if sender sent more funds than the balance can send
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(1_000_000) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(2_000_000) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending some correct funds
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(1_000_000) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "other".to_string(), amount: Uint128(5_561_532) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::MissingDenom(msg)) => {
                assert_eq!(msg, "uscrt")
            },
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending more than one tokens
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(1_000_000) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(2_000_000) }, Coin { denom: "other".to_string(), amount: Uint128(2_000_000) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::ExtraDenom(msg)) => {
                assert_eq!(msg, "uscrt")
            },
            _ => panic!("Unexpected error")
        }

        // Test success
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(10_000_000) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(5_961_532) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone()).unwrap();
        assert_eq!(1, res.messages.len());
        assert_eq!(res.messages[0], CosmosMsg::Bank(BankMsg::Send {
            from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
            to_address: HumanAddr::from("delegator1"),
            amount: vec![Coin{ denom: "ujack".to_string(), amount: Uint128(5) }]
        }));

    }


    #[test]
    fn play(){
        // Success lottery result
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let env = mock_env();
        let res = handle_play(deps.as_mut(), env.clone(), info.clone()).unwrap();
        assert_eq!(1, res.messages.len());
        // Test if state have changed correctly
        let res = query_config(deps.as_ref()).unwrap();
        // Test fees is now superior to 0
        assert_ne!(0, res.holdersRewards.u128());
        // Test block to play superior block height
        assert!(res.blockPlay > env.block.height);
        // Test if players array is empty, we need to re init
        assert_eq!(0, res.players.len());
        // Can't replay only every x blocks
        let res = handle_play(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {},
            _ => panic!("Unexpected error")
        }

        // Do not send funds with play
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        let env = mock_env();
        let res = handle_play(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::DoNotSendFunds (msg)) => {
                assert_eq!(msg ,"Play")
            },
            _ => panic!("Unexpected error")
        }

    }
    #[test]
    fn reward(){
        // Test no funds in the contract
        let mut deps = mock_dependencies(&[]);
        deps.querier.update_balance(HumanAddr::from("validator1"), vec![Coin{ denom: "upot".to_string(), amount: Uint128(4_532_004) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let env = mock_env();
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test no token holder detected
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        deps.querier.update_balance(HumanAddr::from("validator1"), vec![Coin{ denom: "ujack".to_string(), amount: Uint128(4_532_004) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let env = mock_env();
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test do not send funds with reward
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        deps.querier.update_balance(HumanAddr::from("validator1"), vec![Coin{ denom: "upot".to_string(), amount: Uint128(4_532_004) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[Coin{ denom: "upot".to_string(), amount: Uint128(4_532_004) }]);
        let env = mock_env();
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::DoNotSendFunds (msg)) => {
                assert_eq!(msg, "Reward")
            },
            _ => panic!("Unexpected error")
        }

        // Test shares too low
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        deps.querier.update_balance(HumanAddr::from("validator1"), vec![Coin{ denom: "upot".to_string(), amount: Uint128(4_532)}]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let env = mock_env();
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::SharesTooLow {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test success lottery result
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        deps.querier.update_balance(HumanAddr::from("validator1"), vec![Coin{ denom: "upot".to_string(), amount: Uint128(4_532_004) }]);
        default_init(&mut deps, false);
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let env = mock_env();
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone()).unwrap();
        assert_eq!(1, res.messages.len());
        assert_eq!(res.messages[0], CosmosMsg::Bank(BankMsg::Send {
            from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
            to_address: HumanAddr::from("validator1"),
            amount: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(2349) }]
        }));
        // Test if state have changed correctly
        let res = query_config(deps.as_ref()).unwrap();
        // Test if claimReward is not empty and the right address was added correctly
        assert_ne!(0, res.claimReward.len());
        assert!(res.claimReward.contains(&deps.api.canonical_address(&HumanAddr::from("validator1")).unwrap()));
        // Test reward amount was updated with success
        assert_ne!(5_221, res.holdersRewards.u128());

        // Test error can't claim two times
        let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
        match res {
            Err(ContractError::AlreadyClaimed {}) => {},
            _ => panic!("Unexpected error")
        }
    }
}
