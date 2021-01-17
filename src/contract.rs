use cosmwasm_std::{to_binary, attr, Context, Api, Binary, Env, Deps, DepsMut, HandleResponse, InitResponse, MessageInfo, Querier, StdResult, Storage, BankMsg, Coin, StakingMsg, StakingQuery, StdError, AllDelegationsResponse, HumanAddr, Uint128, Delegation, Decimal, BankQuery, Order, CanonicalAddr};

use crate::error::ContractError;
use crate::msg::{HandleMsg, InitMsg, QueryMsg, ConfigResponse, LatestResponse, GetResponse, AllCombinationResponse, CombinationInfo, AllWinnerResponse, WinnerInfo};
use crate::state::{config, config_read, State, beacons_storage, beacons_storage_read, combination_storage_read, combination_storage, Combination, winner_storage_read, winner_storage, Winner};
use cosmwasm_std::testing::StakingQuerier;
use crate::error::ContractError::Std;
use std::fs::canonicalize;
use schemars::_serde_json::map::Entry::Vacant;
use std::io::Stderr;
use std::ops::{Mul, Sub};
use drand_verify::{verify, g1_from_fixed, g1_from_variable};
use sha2::{Digest, Sha256};
use regex::Regex;
use hex;
use regex::internal::Input;
use serde::de::Unexpected::Str;

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
        denomTicket: msg.denomTicket,
        denomDelegation: msg.denomDelegation,
        denomDelegationDecimal: msg.denomDelegationDecimal,
        denomShare: msg.denomShare,
        claimTicket: msg.claimTicket,
        claimReward: msg.claimReward,
        holdersRewards: msg.holdersRewards,
        tokenHolderSupply: msg.tokenHolderSupply,
        drandPublicKey: msg.drandPublicKey,
        drandPeriod: msg.drandPeriod,
        drandGenesisTime: msg.drandGenesisTime,
        validatorMinAmountToAllowClaim: msg.validatorMinAmountToAllowClaim,
        delegatorMinAmountInDelegation: msg.delegatorMinAmountInDelegation,
        combinationLen: msg.combinationLen,
        jackpotReward: msg.jackpotReward,
        jackpotPercentageReward: msg.jackpotPercentageReward,
        tokenHolderPercentageFeeReward: msg.tokenHolderPercentageFeeReward
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
        HandleMsg::Register {
            combination
        } => handle_register(deps, _env, info, combination),
        HandleMsg::Play {
            round,
            previous_signature,
            signature,
        } => handle_play(deps, _env, info, round, previous_signature, signature),
        HandleMsg::Claim {} => handle_claim(deps, _env, info),
        HandleMsg::Ico {} => handle_ico(deps, _env, info),
        HandleMsg::Buy {} => handle_buy(deps, _env, info),
        HandleMsg::Reward {} => handle_reward(deps, _env, info)
    }
}

pub fn handle_register(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    combination: String
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;

    // Regex to check if the combination is allowed
    let regexBuild = format!(r"\b[a-f0-9]{{{}}}\b", state.combinationLen);
    let re = Regex::new(regexBuild.as_str()).unwrap();
    if !re.is_match(&combination) {
        return Err(ContractError::CombinationNotAuthorized(state.combinationLen.to_string()));
    }

    // Check if some funds are sent
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomTicket {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denomTicket.clone().to_string()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denomTicket.clone().to_string())),
    }?;
    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    }
    /*
        TODO: Check if sent is 1 ticket
     */

    // Check if the lottery is about to play and cancel new ticket to enter until play
    if _env.block.height >= state.blockPlay {
        return Err(ContractError::LotteryAboutToStart {});
    }

    // Save combination and addresses to the bucket
    let keyExist = combination_storage(deps.storage).load(&combination.as_bytes());
    if keyExist.is_ok(){
        let mut combinationStorage = keyExist.unwrap();
        combinationStorage.addresses.push(deps.api.canonical_address(&info.sender)?);
        combination_storage(deps.storage).save(&combination.as_bytes(), &combinationStorage);
    }
    else{
        combination_storage(deps.storage).save(&combination.as_bytes(), &Combination{ addresses:vec![deps.api.canonical_address(&info.sender)?]});
    }

    /*let ticketNumber = sent.u128();
    for d in 0..ticketNumber {
        // Update the state
        state.players.push(deps.api.canonical_address(&info.sender.clone())?);
    }*/

    // Save the new state
    // config(deps.storage).save(&state);

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
    let mut delegator = deps.querier.query_all_delegations(&info.sender)?;
    if delegator.is_empty() {
        return Err(ContractError::NoDelegations {})
    }
    delegator.sort_by(|a, b| b.amount.amount.cmp(&a.amount.amount));

    // Ensure validator are owning 10000 upot min and the user stake the majority of his funds to this validator
    let validatorBalance = deps.querier.query_balance(&delegator[0].validator, &state.denomShare).unwrap();
    if validatorBalance.amount.u128() < state.validatorMinAmountToAllowClaim as u128 {
        return Err(ContractError::ValidatorNotAuthorized(state.validatorMinAmountToAllowClaim.to_string()));
    }
    // Ensure is delegating the right denom
    if delegator[0].amount.denom != state.denomDelegation {
        return Err(ContractError::NoDelegations {});
    }
    // Ensure delegating the minimum admitted
    if delegator[0].amount.amount < state.delegatorMinAmountInDelegation {
        return Err(ContractError::DelegationTooLow(state.delegatorMinAmountInDelegation.to_string()));
    }
    // Ensure sender only can claim one time every x blocks
    if state.claimTicket.iter().any(|address| deps.api.human_address(address).unwrap() == info.sender){
        return Err(ContractError::AlreadyClaimed {});
    }
    // Add the sender to claimed state
    state.claimTicket.push(sender.clone());

    // Get the contract balance
    let balance = deps.querier.query_balance(_env.contract.address.clone(), &state.denomTicket)?;
    // Cancel if no amount in the contract
    if balance.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }

    // Save the new state
    config(deps.storage).save(&state);

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin{ denom: state.denomTicket.clone(), amount: Uint128(1)}]
    };
    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "claim"), attr("to", &sender)],
        data: None,
    })
}

// Derives a 32 byte randomness from the beacon's signature
fn derive_randomness(signature: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(signature);
    hasher.finalize().into()
}
/*fn empty_combination_bucket(deps: DepsMut){
    // Load combinations
    let mut combination = combination_storage(deps.storage);
    let mut store = query_all_combination(deps.as_ref()).unwrap();

   /* let mut iter = combination
        .range(None, None, Order::Ascending)
        .map(|item| item.and_then(|(k, _)| Ok(k.as_ref())
        )).collect();
*/

    let x = store.combination.iter().map(|d| combination.remove(d.key.as_bytes()));
    println!("{:?}", store.combination);

}*/
pub fn handle_play(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    round: u64,
    previous_signature: Binary,
    signature: Binary
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // Load combinations
    //let store = query_all_combination(deps.as_ref()).unwrap();
    //empty_combination_bucket(deps);
    //store.combination.iter().map(|d| storage.remove(b"476ad8"));
    let store = query_all_combination(deps.as_ref()).unwrap();
    println!("{:?} o", store.combination);
    if store.combination.is_empty() {
        return Err(ContractError::NoPlayers {});
    }


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
    // Get the current round and check if it is a valid round.
    let fromGenesis = _env.block.time - state.drandGenesisTime;
    let nextRound = (fromGenesis as f64 / state.drandPeriod as f64) + 1.0;
    println!("{}", nextRound);
    if round != nextRound as u64 {
        return Err(ContractError::InvalidRound {});
    }

    let pk = g1_from_variable(&state.drandPublicKey).unwrap();

    let valid = verify(&pk, round, &previous_signature, &signature).unwrap_or(false);

    if !valid {
        return Err(ContractError::InvalidSignature {});
    }
    let randomness = derive_randomness(&signature);
    //let randomnessHash = hex::encode(derive_randomness(&signature));
    //save beacon for other users usage
    beacons_storage(deps.storage).set(&round.to_be_bytes(), &randomness);
    let randomnessHash = hex::encode(randomness);
    println!("{}", randomnessHash);
    //let winningCombination = randomnessHash.char_indices().rev().nth(state.combinationLen as usize).map(|(i, _)| i)
    //    .expect("string with more than 10 characters");

    //let m = randomnessHash.char_indices().rev().take(10).map(|(f, e)|e);
    let n = randomnessHash.char_indices().rev().nth(state.combinationLen as usize - 1).map(|(i, _)| i).unwrap();
    let winningCombination = &randomnessHash[n..];

    /* let count = store.combination.iter().map(|e| {
        e.key.chars()
    }) */
    println!("{:?}, {}, {:?}", winningCombination.chars(), winningCombination.len(), winningCombination.chars().next());

    // Set jackpot amount
    let balance = deps.querier.query_balance(&_env.contract.address, &state.denomDelegation).unwrap();
    println!("{}", balance.amount.u128());
    // Max amount winners can claim
    let balanceAdjusted = balance.amount.mul(Decimal::percent(state.jackpotPercentageReward));
    // Amount token holders can claim of the reward is a fee
    let tokenHolderFeeReward = balanceAdjusted.mul(Decimal::percent(state.tokenHolderPercentageFeeReward));


    println!("{}", balanceAdjusted.u128());
    println!("{}", tokenHolderFeeReward.u128());

    let mut count = 0;
    for combination in store.combination {
        for x in 0..winningCombination.len(){
            if combination.key.chars().nth(x).unwrap() == winningCombination.chars().nth(x).unwrap(){
                count += 1;
            }
        }

        if count == winningCombination.len() {
            // Set the reward for token holders
            state.holdersRewards = tokenHolderFeeReward;
            winner_storage(deps.storage).save("1".as_bytes(), &Winner{ addresses: combination.addresses });
        }
        else if count == winningCombination.len() - 1 {
            winner_storage(deps.storage).save("2".as_bytes(), &Winner{ addresses: combination.addresses });
        }
        else if count == winningCombination.len() - 2 {
            winner_storage(deps.storage).save("3".as_bytes(), &Winner{ addresses: combination.addresses });
        }
        else if count == winningCombination.len() - 3 {
            winner_storage(deps.storage).save("4".as_bytes(), &Winner{ addresses: combination.addresses });
        }
        else if count == winningCombination.len() - 4 {
            winner_storage(deps.storage).save("5".as_bytes(), &Winner{ addresses: combination.addresses });
        }
        // Re init the counter for the next players
        count = 0;
    }

    //state.jackpotAmount =
    // Empty combination for the next play
    let keys = combination_storage(deps.storage)
        .range(None, None, Order::Ascending)
        .flat_map(|item| item.and_then(|(key, _)|{ Ok(key)})).collect::<Vec<Vec<u8>>>();
    for x in keys {
        combination_storage(deps.storage).remove(x.as_ref())
    }
    

    // Sort the winner
    let players = match state.players.len(){
        0 => Err(ContractError::NoPlayers {}),
        _ => Ok(&state.players)
    }?;
    // let mut rng = rand::thread_rng();
    let maxRange = players.len() - 1;
    // let winnerRange = rng.gen_range(0..maxRange);
    let winnerAddress = players[0].clone();//players[winnerRange].clone();

    // Winning 5 to 99 % of the lottery
    let percentOfJackpot = 5; //rng.gen_range(5..99);

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

    let balance = deps.querier.query_balance(&_env.contract.address, &state.denomTicket).unwrap();
    if balance.amount.is_zero(){
        return Err(ContractError::EmptyBalance {});
    }

    if balance.amount.u128() < sent.u128() {
        return Err(ContractError::EmptyBalance {});
    }

    let amountToSend = sent.u128() / state.denomDelegationDecimal.u128();

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin{ denom: state.denomTicket.clone(), amount: Uint128(amountToSend)}]
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
) -> Result<Binary, ContractError> {
    let response = match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?)?,
        QueryMsg::LatestDrand {} => to_binary(&query_latest(deps)?)?,
        QueryMsg::GetRandomness {round} => to_binary(&query_get(deps, round)?)?,
        QueryMsg::Combination {} =>  to_binary(&query_all_combination(deps)?)?,
        QueryMsg::Winner {} => to_binary(&query_all_winner(deps)?)?
    };
    Ok(response)
}

fn query_config(deps: Deps) -> Result<ConfigResponse, ContractError> {
    let state = config_read(deps.storage).load()?;
    Ok(state)
}
// Query beacon by round
fn query_get(deps: Deps, round: u64) -> Result<GetResponse, ContractError>{
    let beacons = beacons_storage_read(deps.storage);
    let randomness = beacons.get(&round.to_be_bytes()).unwrap_or_default();
    Ok(GetResponse {
        randomness: randomness.into(),
    })
}
// Query latest beacon
fn query_latest(deps: Deps) -> Result<LatestResponse, ContractError>{
    let store = beacons_storage_read(deps.storage);
    let mut iter = store.range(None, None, Order::Descending);
    let (key, value) =  iter.next().ok_or(ContractError::NoBeacon {})?;
    Ok(LatestResponse {
        round: u64::from_be_bytes(Binary(key).to_array()?),
        randomness: value.into(),
    })
}
fn query_all_combination(deps: Deps) -> Result<AllCombinationResponse, ContractError>{
    let combinations = combination_storage_read(deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item|{
            item.and_then(|(k, combination)|{
               Ok(CombinationInfo{
                   key: String::from_utf8(k)?,
                   addresses: combination.addresses
               })
            })
        }).collect();


    Ok(AllCombinationResponse{
        combination: combinations,
    })
}

fn query_all_winner(deps: Deps) -> Result<AllWinnerResponse, ContractError> {
    let winners = winner_storage_read(deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item|{
            item.and_then(|(k, winner)|{
                Ok(WinnerInfo{
                    rank: String::from_utf8(k)?,
                    addresses: winner.addresses
                })
            })
        }).collect();
    Ok(AllWinnerResponse{
        winner: winners
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, BankQuerier, MOCK_CONTRACT_ADDR, MockStorage, StakingQuerier, MockApi, MockQuerier};
    use cosmwasm_std::{coins, from_binary, Validator, Decimal, FullDelegation, HumanAddr, Uint128, MessageInfo, StdError, Storage, Api, CanonicalAddr, OwnedDeps, CosmosMsg, QuerierResult, Binary};
    use std::collections::HashMap;
    use std::borrow::Borrow;
    use cosmwasm_storage::{bucket_read, bucket, singleton, singleton_read};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use crate::msg::{HandleMsg, InitMsg, QueryMsg};
    // DRAND
    use drand_verify::{verify, g1_from_fixed, g1_from_variable};

    use hex_literal::hex;
    use sha2::{Digest, Sha256};



    fn default_init(deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>) {
        let player1 = deps.api.canonical_address(&HumanAddr::from("player1")).unwrap();
        let player2 = deps.api.canonical_address(&HumanAddr::from("player2")).unwrap();
        let player3 = deps.api.canonical_address(&HumanAddr::from("player3")).unwrap();
        let player4 = deps.api.canonical_address(&HumanAddr::from("player4")).unwrap();
        let player5 = deps.api.canonical_address(&HumanAddr::from("player5")).unwrap();
        let player6 = deps.api.canonical_address(&HumanAddr::from("player6")).unwrap();

        const DENOM_TICKET: &str = "ujack";
        const DENOM_DELEGATION: &str = "uscrt";
        const DENOM_DELEGATION_DECIMAL: Uint128 = Uint128(1_000_000);
        const DENOM_SHARE: &str = "upot";
        const EVERY_BLOCK_EIGHT: u64 = 100;
        let PLAYERS: Vec<CanonicalAddr> = vec![player1, player2.clone(), player2.clone(), player3.clone(), player3.clone(), player3.clone(), player3.clone(), player4.clone(), player4.clone(), player4.clone(), player5, player6];
        const CLAIM_TICKET: Vec<CanonicalAddr> = vec![];
        const CLAIM_REWARD: Vec<CanonicalAddr> = vec![];
        const BLOCK_PLAY: u64 = 15000;
        const BLOCK_CLAIM: u64 = 0;
        const BLOCK_ICO_TIME_FRAME: u64 = 1000000000;
        const HOLDERS_REWARDS: Uint128 = Uint128(5_221);
        const TOKEN_HOLDER_SUPPLY: Uint128 = Uint128(10_000_000);
        let PUBLIC_KEY: Binary = vec![
            134, 143, 0, 94, 184, 230, 228, 202, 10, 71, 200, 167, 124, 234, 165, 48, 154, 71, 151,
            138, 124, 113, 188, 92, 206, 150, 54, 107, 93, 122, 86, 153, 55, 197, 41, 238, 218,
            102, 199, 41, 55, 132, 169, 64, 40, 1, 175, 49,
        ].into(); //Binary::from(hex!("868f005eb8e6e4ca0a47c8a77ceaa5309a47978a7c71bc5cce96366b5d7a569937c529eeda66c7293784a9402801af31"));
        const GENESIS_TIME: u64 = 1595431050;
        const PERIOD: u64 = 30;
        const VALIDATOR_MIN_AMOUNT_TO_ALLOW_CLAIM: u64 = 10_000;
        const DELEGATOR_MIN_AMOUNT_IN_DELEGATION: Uint128 = Uint128(10_000);
        const COMBINATION_LEN: u8 = 6;
        const JACKPOT_REWARD: Uint128 = Uint128(0);
        const JACKPOT_PERCENTAGE_REWARD: u64 = 80;
        const TOKEN_HOLDER_PERCENTAGE_FEE_REWARD: u64 = 10;

        fn pubkey () -> Binary {
            vec![
                134, 143, 0, 94, 184, 230, 228, 202, 10, 71, 200, 167, 124, 234, 165, 48, 154, 71, 151,
                138, 124, 113, 188, 92, 206, 150, 54, 107, 93, 122, 86, 153, 55, 197, 41, 238, 218,
                102, 199, 41, 55, 132, 169, 64, 40, 1, 175, 49,
            ].into()
        }


        let init_msg = InitMsg{
            denomTicket: DENOM_TICKET.to_string(),
            denomDelegation: DENOM_DELEGATION.to_string(),
            denomDelegationDecimal: DENOM_DELEGATION_DECIMAL,
            denomShare: DENOM_SHARE.to_string(),
            everyBlockHeight: EVERY_BLOCK_EIGHT,
            players: PLAYERS,
            claimTicket: CLAIM_TICKET,
            claimReward: CLAIM_REWARD,
            blockPlay: BLOCK_PLAY,
            blockClaim: BLOCK_PLAY,
            blockIcoTimeframe: BLOCK_ICO_TIME_FRAME,
            holdersRewards: HOLDERS_REWARDS,
            tokenHolderSupply: TOKEN_HOLDER_SUPPLY,
            drandPublicKey: PUBLIC_KEY,
            drandPeriod: PERIOD,
            drandGenesisTime: GENESIS_TIME,
            validatorMinAmountToAllowClaim: VALIDATOR_MIN_AMOUNT_TO_ALLOW_CLAIM,
            delegatorMinAmountInDelegation: DELEGATOR_MIN_AMOUNT_IN_DELEGATION,
            combinationLen: COMBINATION_LEN,
            jackpotReward: JACKPOT_REWARD,
            jackpotPercentageReward: JACKPOT_PERCENTAGE_REWARD,
            tokenHolderPercentageFeeReward: TOKEN_HOLDER_PERCENTAGE_FEE_REWARD
        };
        let info = mock_info(HumanAddr::from("owner"), &[]);
        init(deps.as_mut(), mock_env(), info, init_msg).unwrap();
    }
    /// Derives a 32 byte randomness from the beacon's signature
    fn derive_randomness(signature: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(signature);
        hasher.finalize().into()
    }
    #[test]
    fn random (){
        //println!("{}", 1432439234 % 100);
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps);

        let res = query_config(deps.as_ref()).unwrap();

        // DRAND
        let PK_LEO_MAINNET: [u8; 48] = hex!("868f005eb8e6e4ca0a47c8a77ceaa5309a47978a7c71bc5cce96366b5d7a569937c529eeda66c7293784a9402801af31");
        const GENESIS_TIME: u64 = 1595431050;
        const PERIOD: f64 = 30.0;

        //let pk = g1_from_fixed(PK_LEO_MAINNET).unwrap();
        let pk = g1_from_variable(&res.drandPublicKey).unwrap();
        let previous_signature = hex::decode("9491284489e453531c2429245dc6a9c4667f4cc2ab36295e7652de566d8ea2e16617690472d99b7d4604ecb8a8a249190e3a9c224bda3a9ea6c367c8ab6432c8f177838c20429e51fedcb8dacd5d9c7dc08b5147d6abbfc3db4b59d832290be2").unwrap();
        let signature = hex::decode("b1af60ff60d52b38ef13f8597df977c950997b562ec8bf31b765dedf3e138801a6582b53737b654d1df047c1786acd94143c9a02c173185dcea2fa2801223180d34130bf8c6566d26773296cdc9666fdbf095417bfce6ba90bb83929081abca3").unwrap();
        let round: u64 = 501672;
        let result = verify(&pk, round, &previous_signature, &signature).unwrap();
        let mut env = mock_env();
        env.block.time = 1610566920;
        let now = env.block.time;
        let fromGenesis = env.block.time - GENESIS_TIME;
        // let time = 1610566920 - 1595431050;
        let round = (fromGenesis as f64 / PERIOD) + 1.0;

        assert_eq!(result, true);
        println!("{:?}", pk);
        println!("{:?}", hex::encode(derive_randomness(&signature)));
        println!("{:?}", env);
        // println!("{}", time);
        println!("{}", round.floor());
        println!("{}", fromGenesis);
    }
    #[test]
    fn proper_init (){
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps);
        println!("{:?}", mock_env())
    }
    #[test]
    fn register() {

        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps);

        // Test if this succeed
        let msg = HandleMsg::Register {
            combination: "1e3fab".to_string()
        };
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(1)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());
        // check if we can add multiple players
        let msg = HandleMsg::Register {
            combination: "1e3fab".to_string()
        };
        let info = mock_info(HumanAddr::from("delegator12"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(1)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();
        // Load all combination
        let res = query_all_combination(deps.as_ref()).unwrap();
        // Test if the address is saved success
        assert_eq!(2, res.combination[0].addresses.len());

        // Test sending 0 ticket NoFunds
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(0)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::NoFunds {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test sending 0 coins NoFunds
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::NoFunds {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test sending another coin
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::MissingDenom (msg)) => {
                assert_eq!(msg, "ujack".to_string())
            },
            _ => panic!("Unexpected error")
        }
        // Test sending more tokens than admitted
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(3)}, Coin{ denom: "uscrt".to_string(), amount: Uint128(2)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ExtraDenom(msg)) => {
                assert_eq!(msg, "ujack".to_string())
            },
            _ => panic!("Unexpected error")
        }
        // Test trying to play the lottery when the lottery is about to start
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(2)}]);
        let mut env = mock_env();
        // If block eight is inferior block to play the lottery is about to start and we can't admit new register until the lottery play cause the result can be leaked at every moment.
        env.block.height = 16000;
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        match res {
            Err(ContractError::LotteryAboutToStart {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test if multiple players are added to the player array
        let msg = HandleMsg::Register {
            combination: "1e3fab".to_string()
        };
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps);

        // Test if we get error if combination is not authorized
        let msg = HandleMsg::Register {
            combination: "1e3fabgvcc".to_string()
        };
        let info = mock_info(HumanAddr::from("delegator12"), &[Coin{ denom: "ujack".to_string(), amount: Uint128(3)}]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::CombinationNotAuthorized(msg)) => {
                assert_eq!("6", msg);
            },
            _ => panic!("Unexpected error")
        }
    }
    #[test]
    fn claim() {
        // Test handle do not send funds
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(15_050_030)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
                delegator: HumanAddr::from("delegator1"),
                validator: HumanAddr::from("validator3"),
                amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(2_004_000)},
                can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
                accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
            }
        ]);
        default_init(&mut deps);
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
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::NoDelegations {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test error no funds in the contract
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(15_050_030)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
                delegator: HumanAddr::from("delegator1"),
                validator: HumanAddr::from("validator3"),
                amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(2_004_000)},
                can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
                accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
            }
        ]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test validator is not holding the sufficient amount to allow his users to claim tickets
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(1_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(15_050_030)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
                delegator: HumanAddr::from("delegator1"),
                validator: HumanAddr::from("validator3"),
                amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(2_004_000)},
                can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
                accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
            }
        ]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::ValidatorNotAuthorized(msg)) =>{
                assert_eq!(msg, "10000")
            },
            _ => panic!("Unexpected error")
        }
        // Test you are staking less than 1000 scrt
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(1_050)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::DelegationTooLow(msg)) =>{
                assert_eq!(msg, "1000")
            },
            _ => panic!("Unexpected error")
        }

        // Test you are staking the right denom
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "ucoin".to_string(), amount: Uint128(10_050_000)},
            can_redelegate: Coin{ denom: "ucoin".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "ucoin".to_string(), amount: Uint128(0)},]
        }]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[]);
        let res = handle_claim(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::NoDelegations {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test success claim
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}, Coin{ denom: "ujack".to_string(), amount: Uint128(100_000_000)}]);
        deps.querier.update_balance("validator2", vec![Coin{ denom: "upot".to_string(), amount: Uint128(10_000)}]);
        deps.querier.update_staking("uscrt", &[Validator{
            address: HumanAddr::from("validator1"),
            commission: Decimal::percent(10),
            max_commission: Decimal::percent(100),
            max_change_rate: Decimal::percent(100)
        }], &[FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator1"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        }, FullDelegation{
            delegator: HumanAddr::from("delegator1"),
            validator: HumanAddr::from("validator2"),
            amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(15_050_030)},
            can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
            accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
        },
            FullDelegation{
                delegator: HumanAddr::from("delegator1"),
                validator: HumanAddr::from("validator3"),
                amount: Coin{ denom: "uscrt".to_string(), amount: Uint128(2_004_000)},
                can_redelegate: Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},
                accumulated_rewards: vec![Coin{ denom: "uscrt".to_string(), amount: Uint128(0)},]
            }
        ]);
        default_init(&mut deps);
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
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(1_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test if sender sent more funds than the balance can send
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(1_000_000)}]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(2_000_000)}]);
        let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending some correct funds
        let mut deps = mock_dependencies(&[Coin{ denom: "upot".to_string(), amount: Uint128(1_000_000)}]);
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(1_000_000) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }
        // Test if sender sent more funds than the balance can send
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(1_000_000) }]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("delegator1"), &[Coin { denom: "uscrt".to_string(), amount: Uint128(2_000_000) }]);
        let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::EmptyBalance {}) => {},
            _ => panic!("Unexpected error")
        }

        // Test the sender is sending some correct funds
        let mut deps = mock_dependencies(&[Coin { denom: "ujack".to_string(), amount: Uint128(1_000_000) }]);
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        // Expired round
        //let previous_signature = hex::decode("9491284489e453531c2429245dc6a9c4667f4cc2ab36295e7652de566d8ea2e16617690472d99b7d4604ecb8a8a249190e3a9c224bda3a9ea6c367c8ab6432c8f177838c20429e51fedcb8dacd5d9c7dc08b5147d6abbfc3db4b59d832290be2").unwrap().into();
        //let signature = hex::decode("b1af60ff60d52b38ef13f8597df977c950997b562ec8bf31b765dedf3e138801a6582b53737b654d1df047c1786acd94143c9a02c173185dcea2fa2801223180d34130bf8c6566d26773296cdc9666fdbf095417bfce6ba90bb83929081abca3").unwrap().into();
        //let round: u64 = 501672;

        // Valid round
        let signature = hex::decode("a48f7cdf76968d2a5b8d05d8a4dfc9ae823de53c4864a066c55f1a334f39314e43dec27d230f1b1222fd14bed39f0b9801b5bb2881a8dc346418ebada58dc8409f70046759f057ea1fbef449d5cd43259f9ae98d24025905887db4e6869a2fe9").unwrap().into();
        let previous_signature= hex::decode("a105715b0e253767ed03d0a5e0356bbe6a874b004d9cd0e5fe76f4acb1da345ae71e19e968afedad7c17e5c00d05ebdd0e9d1f81404c2f42934001a37fa961bff03c1a58c6e31a7db4fb417018fdca8d614d95dade2a3be461759ea86b29819f").unwrap().into();
        let round = 504530;
        let msg = HandleMsg::Play {
            round: round,
            previous_signature: previous_signature,
            signature: signature
        };

        // Success lottery result
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        default_init(&mut deps);
        // Init the bucket with players to storage
        let combination= "476ad8";
        let combination2 = "476ad3";
        let addresses1 = vec![deps.api.canonical_address(&HumanAddr("address1".to_string())).unwrap(), deps.api.canonical_address(&HumanAddr("address2".to_string())).unwrap()];
        let addresses2 = vec![deps.api.canonical_address(&HumanAddr("address2".to_string())).unwrap()];
        combination_storage(&mut deps.storage).save(&combination.as_bytes(), &Combination{ addresses: addresses1 });
        combination_storage(&mut deps.storage).save(&combination2.as_bytes(), &Combination{ addresses: addresses2 });
        let info = mock_info(HumanAddr::from("validator1"), &[]);
        let mut env = mock_env();
        env.block.height = 15005;
        env.block.time = 1610566920;
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        println!("{:?}", res);
        assert_eq!(1, res.messages.len());

        // Test if winners have been added correctly
        let res = query_all_winner(deps.as_ref()).unwrap();
        assert_eq!(1,  res.winner[0].addresses.len());

        // Test if state have changed correctly
        let res = query_config(deps.as_ref()).unwrap();
        // Test fees is now superior to 0
        assert_ne!(0, res.holdersRewards.u128());
        // Test block to play superior block height
        assert!(res.blockPlay > env.block.height);
        // Test if players array is empty, we need to re init
        assert_eq!(0, res.players.len());
        // Test if round was saved
        let res = query_latest(deps.as_ref()).unwrap();
        println!("{:?}", res);
        assert_eq!(504530, res.round);
        // Can't replay only every x blocks
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        //let res = handle_play(deps.as_mut(), mock_env(), info.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {},
            _ => panic!("Unexpected error")
        }

        // Do not send funds with play
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        default_init(&mut deps);
        // Init the bucket with players to storage
        let combination= "476ad8";
        let combination2 = "476ad3";
        let addresses1 = vec![deps.api.canonical_address(&HumanAddr("address1".to_string())).unwrap(), deps.api.canonical_address(&HumanAddr("address2".to_string())).unwrap()];
        let addresses2 = vec![deps.api.canonical_address(&HumanAddr("address2".to_string())).unwrap()];
        combination_storage(&mut deps.storage).save(&combination.as_bytes(), &Combination{ addresses: addresses1 });
        combination_storage(&mut deps.storage).save(&combination2.as_bytes(), &Combination{ addresses: addresses2 });
        let info = mock_info(HumanAddr::from("validator1"), &[Coin{ denom: "uscrt".to_string(), amount: Uint128(10_000_000)}]);
        let env = mock_env();
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        //let res = handle_play(deps.as_mut(), env.clone(), info.clone());
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
        default_init(&mut deps);
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
