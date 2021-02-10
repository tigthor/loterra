use cosmwasm_std::{
    to_binary, Api, BankMsg, Binary, CanonicalAddr, Coin, Decimal, Empty, Env, Extern,
    HandleResponse, HumanAddr, InitResponse, LogAttribute, Order, Querier, QueryRequest, StdError,
    StdResult, Storage, Uint128, WasmQuery,
};

use crate::msg::{
    AllCombinationResponse, AllWinnerResponse, CombinationInfo, ConfigResponse, GetPollResponse,
    HandleMsg, InitMsg, QueryMsg, RoundResponse, WinnerInfo,
};
use crate::query::TerrandResponse;
use crate::state::{
    combination_storage, combination_storage_read, config, config_read, poll_storage,
    poll_storage_read, winner_storage, winner_storage_read, Combination, PollInfoState, PollStatus,
    Proposal, State, Winner, WinnerInfoState,
};

use hex;
use serde::de::Unexpected::Bytes;
use std::convert::TryInto;
use std::ops::{Mul, Sub};

const MIN_DESC_LEN: u64 = 6;
const MAX_DESC_LEN: u64 = 64;
const NEXT_ROUND: u64 = 10;
const DRAND_PERIOD: u64 = 30;
const DRAND_GENESIS_TIME: u64 = 1595431050;
// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
// #[serde(rename_all = "snake_case")]

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let state = State {
        blockTimePlay: msg.blockTimePlay,
        everyBlockTimePlay: msg.everyBlockTimePlay,
        publicSaleEndBlock: msg.publicSaleEndBlock,
        denomStableDecimal: msg.denomStableDecimal,
        denomStable: msg.denomStable,
        denomShare: msg.denomShare,
        tokenHolderSupply: msg.tokenHolderSupply,
        pollEndHeight: msg.pollEndHeight,
        claimReward: vec![],
        holdersRewards: Uint128::zero(),
        combinationLen: 6,
        jackpotReward: Uint128::zero(),
        jackpotPercentageReward: 80,
        tokenHolderPercentageFeeReward: 10,
        feeForDrandWorkerInPercentage: 1,
        prizeRankWinnerPercentage: vec![84, 10, 5, 1],
        pollCount: 0,
        holdersMaxPercentageReward: 20,
        workerDrandMaxPercentageReward: 10,
        pricePerTicketToRegister: Uint128(1_000_000),
        terrandContractAddress: msg.terrandContractAddress,
    };
    config(&mut deps.storage).save(&state)?;
    Ok(InitResponse::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::Register { combination } => handle_register(deps, env, combination),
        HandleMsg::Play {} => handle_play(deps, env),
        HandleMsg::PublicSale {} => handle_public_sale(deps, env),
        HandleMsg::Reward {} => handle_reward(deps, env),
        HandleMsg::Jackpot {} => handle_jackpot(deps, env),
        HandleMsg::Proposal {
            description,
            proposal,
            amount,
            prizePerRank,
        } => handle_proposal(deps, env, description, proposal, amount, prizePerRank),
        HandleMsg::Vote { pollId, approve } => handle_vote(deps, env, pollId, approve),
        HandleMsg::PresentProposal { pollId } => handle_present_proposal(deps, env, pollId),
        HandleMsg::RejectProposal { pollId } => handle_reject_proposal(deps, env, pollId),
    }
}

// There is probably some built-in function for this, but this is a simple way to do it
fn is_lower_hex(combination: &str, len: u8) -> bool {
    if combination.len() != (len as usize) {
        return false;
    }
    if !combination
        .chars()
        .all(|c| ('a'..='f').contains(&c) || ('0'..='9').contains(&c))
    {
        return false;
    }
    true
}

pub fn handle_register<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    combination: String,
) -> StdResult<HandleResponse> {
    // Load the state
    let state = config(&mut deps.storage).load()?;

    // Regex to check if the combination is allowed
    if !is_lower_hex(&combination, state.combinationLen) {
        return Err(StdError::generic_err(format!(
            "Not authorized use combination of [a-f] and [0-9] with length {}",
            state.combinationLen
        )));
    }

    // Check if some funds are sent
    let sent = match env.message.sent_funds.len() {
        0 => Err(StdError::generic_err("Send some funds to register")),
        1 => {
            if env.message.sent_funds[0].denom == state.denomStable {
                Ok(env.message.sent_funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "To register you need to send {}{}",
                    state.pricePerTicketToRegister,
                    state.denomStable.clone()
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Only send {} to register",
            state.denomStable.clone()
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "you need to send {}{} in order to register",
            state.pricePerTicketToRegister.clone(),
            state.denomStable.clone()
        )));
    }
    // Handle the player is not sending too much or too less
    if sent.u128() != state.pricePerTicketToRegister.u128() {
        return Err(StdError::generic_err(format!(
            "send {}{}",
            state.pricePerTicketToRegister.clone(),
            state.denomStable.clone()
        )));
    }

    // Check if the lottery is about to play and cancel new ticket to enter until play
    if env.block.time >= state.blockTimePlay {
        return Err(StdError::generic_err(
            "Lottery is about to start wait until the end before register",
        ));
    }

    // Save combination and addresses to the bucket
    match combination_storage(&mut deps.storage).may_load(&combination.as_bytes())? {
        Some(c) => {
            let mut combination_in_storage = c;
            combination_in_storage
                .addresses
                .push(deps.api.canonical_address(&env.message.sender)?);
            combination_storage(&mut deps.storage)
                .save(&combination.as_bytes(), &combination_in_storage)?;
        }
        None => {
            combination_storage(&mut deps.storage).save(
                &combination.as_bytes(),
                &Combination {
                    addresses: vec![deps.api.canonical_address(&env.message.sender)?],
                },
            )?;
        }
    };

    Ok(HandleResponse {
        messages: vec![],
        log: vec![LogAttribute {
            key: "action".to_string(),
            value: "register".to_string(),
        }],
        data: None,
    })
}

fn encode_msg(msg: QueryMsg, address: HumanAddr) -> StdResult<QueryRequest<Empty>> {
    Ok(WasmQuery::Smart {
        contract_addr: address,
        msg: to_binary(&msg)?,
    }
    .into())
}
fn wrapper<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<TerrandResponse> {
    let res: TerrandResponse = deps.querier.query(&query)?;
    Ok(res)
}

pub fn handle_play<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    let from_genesis = state.blockTimePlay - DRAND_GENESIS_TIME;
    let next_round = (from_genesis / DRAND_PERIOD) + NEXT_ROUND;
    // reset holders reward
    state.holdersRewards = Uint128::zero();
    // Load combinations
    let store = query_all_combination(&deps).unwrap();

    /*
        Empty previous winner
    */
    // Get all keys in the bucket winner
    let keys = winner_storage(&mut deps.storage)
        .range(None, None, Order::Ascending)
        .flat_map(|item| item.map(|(key, _)| key))
        .collect::<Vec<Vec<u8>>>();
    // Empty winner for the next play
    for x in keys {
        winner_storage(&mut deps.storage).remove(x.as_ref())
    }

    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with play"));
    }

    // Make the contract callable for everyone every x blocks
    if env.block.time > state.blockTimePlay {
        // Update the state
        state.claimReward = vec![];
        state.blockTimePlay = env.block.time + state.everyBlockTimePlay;
    } else {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    let msg = QueryMsg::GetTerrand { round: next_round };
    let res = encode_msg(msg, state.terrandContractAddress.clone())?;
    let res = wrapper(&deps, res)?;
    let randomness = hex::encode(res.randomness.to_base64());
    /*
       Todo: create a function to query the randomness from the smart contract
    */
    // TODO: here the result of randomness

    let randomness_hash = randomness;

    let n = randomness_hash
        .char_indices()
        .rev()
        .nth(state.combinationLen as usize - 1)
        .map(|(i, _)| i)
        .unwrap();
    let winning_combination = &randomness_hash[n..];

    // Set jackpot amount
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &state.denomStable)
        .unwrap();
    // Max amount winners can claim
    let jackpot = balance
        .amount
        .mul(Decimal::percent(state.jackpotPercentageReward as u64));
    // Drand worker fee
    let fee_for_drand_worker =
        jackpot.mul(Decimal::percent(state.feeForDrandWorkerInPercentage as u64));
    // Amount token holders can claim of the reward is a fee
    let token_holder_fee_reward = jackpot.mul(Decimal::percent(
        state.tokenHolderPercentageFeeReward as u64,
    ));
    // Total fees if winner of the jackpot
    let total_fee = jackpot.mul(Decimal::percent(
        (state.feeForDrandWorkerInPercentage as u64)
            + (state.tokenHolderPercentageFeeReward as u64),
    ));
    // The jackpot after worker fee applied
    let mut jackpot_after = (jackpot - fee_for_drand_worker).unwrap();

    if !store.combination.is_empty() {
        let mut count = 0;
        for combination in store.combination {
            for x in 0..winning_combination.len() {
                if combination.key.chars().nth(x).unwrap()
                    == winning_combination.chars().nth(x).unwrap()
                {
                    count += 1;
                }
            }

            if count == winning_combination.len() {
                // Set the reward for token holders
                state.holdersRewards = token_holder_fee_reward;
                // Set the new jackpot after fee
                jackpot_after = (jackpot - total_fee).unwrap();

                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !data_winner.is_empty() {
                    winner_storage(&mut deps.storage).save(
                        &1_u8.to_be_bytes(),
                        &Winner {
                            winners: data_winner,
                        },
                    )?;
                }
            } else if count == winning_combination.len() - 1 {
                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !data_winner.is_empty() {
                    winner_storage(&mut deps.storage).save(
                        &2_u8.to_be_bytes(),
                        &Winner {
                            winners: data_winner,
                        },
                    )?;
                }
            } else if count == winning_combination.len() - 2 {
                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !data_winner.is_empty() {
                    winner_storage(&mut deps.storage).save(
                        &3_u8.to_be_bytes(),
                        &Winner {
                            winners: data_winner,
                        },
                    )?;
                }
            } else if count == winning_combination.len() - 3 {
                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !data_winner.is_empty() {
                    winner_storage(&mut deps.storage).save(
                        &4_u8.to_be_bytes(),
                        &Winner {
                            winners: data_winner,
                        },
                    )?;
                }
            } else if count == winning_combination.len() - 4 {
                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !data_winner.is_empty() {
                    winner_storage(&mut deps.storage).save(
                        &5_u8.to_be_bytes(),
                        &Winner {
                            winners: data_winner,
                        },
                    )?;
                }
            }
            // Re init the counter for the next players
            count = 0;
        }
    }

    let msg = BankMsg::Send {
        from_address: env.contract.address,
        to_address: env.message.sender.clone(),
        amount: vec![Coin {
            denom: state.denomStable.clone(),
            amount: fee_for_drand_worker,
        }],
    };
    // Update the state
    state.jackpotReward = jackpot_after;

    // Get all keys in the bucket combination
    let keys = combination_storage(&mut deps.storage)
        .range(None, None, Order::Ascending)
        .flat_map(|item| item.map(|(key, _)| key))
        .collect::<Vec<Vec<u8>>>();
    // Empty combination for the next play
    for x in keys {
        combination_storage(&mut deps.storage).remove(x.as_ref())
    }

    // Save the new state
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![msg.into()],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "reward".to_string(),
            },
            LogAttribute {
                key: "to".to_string(),
                value: env.message.sender.to_string(),
            },
        ],
        data: None,
    })
}

pub fn handle_public_sale<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    // Public sale expire after blockTime
    if state.publicSaleEndBlock < env.block.height {
        return Err(StdError::generic_err("Public sale is ended"));
    }
    // Check if some funds are sent
    let sent = match env.message.sent_funds.len() {
        0 => Err(StdError::generic_err(
            "Send some funds to buy public sale tokens",
        )),
        1 => {
            if env.message.sent_funds[0].denom == state.denomStable {
                Ok(env.message.sent_funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "Only {} is accepted",
                    state.denomStable.clone()
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Send only {}, no extra denom",
            state.denomStable.clone()
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err("Send some funds"));
    };
    // Get the contract balance prepare the tx
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &state.denomShare)
        .unwrap();
    if balance.amount.is_zero() {
        return Err(StdError::generic_err("All tokens have been sold"));
    }

    if balance.amount.u128() < sent.u128() {
        return Err(StdError::generic_err(
            format!("you want to buy {} the contract balance only remain {} token on public sale", sent.u128(), balance.amount.u128())
        ));
    }

    let msg = BankMsg::Send {
        from_address: env.contract.address,
        to_address: env.message.sender.clone(),
        amount: vec![Coin {
            denom: state.denomShare.clone(),
            amount: sent,
        }],
    };

    state.tokenHolderSupply += sent;
    // Save the new state
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![msg.into()],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "public sale".to_string(),
            },
            LogAttribute {
                key: "to".to_string(),
                value: env.message.sender.to_string(),
            },
        ],
        data: None,
    })
}

pub fn handle_reward<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    // convert the sender to canonical address
    let sender = deps.api.canonical_address(&env.message.sender).unwrap();
    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with reward"));
    }
    if state.tokenHolderSupply.is_zero() {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    // Ensure sender have some reward tokens
    let balance_sender = deps
        .querier
        .query_balance(env.message.sender.clone(), &state.denomShare)
        .unwrap();
    if balance_sender.amount.is_zero() {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    // Ensure sender only can claim one time every x blocks
    if state
        .claimReward
        .iter()
        .any(|address| deps.api.human_address(address).unwrap() == env.message.sender.clone())
    {
        return Err(StdError::generic_err("Already claimed"));
    }
    // Add the sender to claimed state
    state.claimReward.push(sender.clone());

    // Get the contract balance
    let balance_contract = deps
        .querier
        .query_balance(env.contract.address.clone(), &state.denomStable)?;
    // Cancel if no amount in the contract
    if balance_contract.amount.is_zero() {
        return Err(StdError::generic_err("Contract balance is empty"));
    }
    // Get the percentage of shareholder
    let share_holder_percentage =
        balance_sender.amount.u128() as u64 * 100 / state.tokenHolderSupply.u128() as u64;
    if share_holder_percentage == 0 {
        return Err(StdError::generic_err(
            "You need at least 1% of total shares to claim rewards",
        ));
    }

    // Calculate the reward
    let reward = state
        .holdersRewards
        .mul(Decimal::percent(share_holder_percentage));

    // Update the holdersReward
    state.holdersRewards = state.holdersRewards.sub(reward).unwrap();
    // Save the new state
    config(&mut deps.storage).save(&state)?;

    let msg = BankMsg::Send {
        from_address: env.contract.address,
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin {
            denom: state.denomStable,
            amount: reward,
        }],
    };

    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "reward".to_string(),
            },
            LogAttribute {
                key: "to".to_string(),
                value: env.message.sender.to_string(),
            },
        ],
        data: None,
    })
}

fn remove_from_storage<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    env: Env,
    winner: &WinnerInfo,
) -> Vec<WinnerInfoState> {
    // Update to claimed
    winner
        .winners
        .iter()
        .map(|win| {
            let mut winx = win.clone();
            if winx.address == deps.api.canonical_address(&env.message.sender).unwrap() {
                winx.claimed = true;
            }
            winx
        })
        .collect::<Vec<WinnerInfoState>>()
}

// Players claim the jackpot
pub fn handle_jackpot<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load state
    let state = config(&mut deps.storage).load()?;
    // Load winners
    let store = query_all_winner(&deps).unwrap();
    // Ensure the sender is not sending funds
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with jackpot"));
    }
    // Ensure there is jackpot reward to claim
    if state.jackpotReward.is_zero() {
        return Err(StdError::Unauthorized { backtrace: None });
    }
    // Ensure there is some winner
    if store.winner.is_empty() {
        return Err(StdError::generic_err("No winners"));
    }

    let mut jackpot_amount: Uint128 = Uint128(0);
    let mut ticket_winning: Uint128 = Uint128(0);
    for winner in store.winner.clone() {
        for winnerInfo in winner.winners.clone() {
            if winnerInfo.address == deps.api.canonical_address(&env.message.sender).unwrap() {
                if winnerInfo.claimed {
                    return Err(StdError::generic_err("Already claimed"));
                }

                match winner.rank {
                    1 => {
                        // Prizes first rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[0] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        // Remove the address from the array and save
                        let new_addresses = remove_from_storage(deps, env.clone(), &winner);
                        winner_storage(&mut deps.storage).save(
                            &1_u8.to_be_bytes(),
                            &Winner {
                                winners: new_addresses,
                            },
                        )?;
                    }
                    2 => {
                        // Prizes second rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[1] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        // Remove the address from the array and save
                        let new_addresses = remove_from_storage(deps, env.clone(), &winner);
                        winner_storage(&mut deps.storage).save(
                            &2_u8.to_be_bytes(),
                            &Winner {
                                winners: new_addresses,
                            },
                        )?;
                    }
                    3 => {
                        // Prizes third rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[2] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        // Remove the address from the array and save
                        let new_addresses = remove_from_storage(deps, env.clone(), &winner);
                        winner_storage(&mut deps.storage).save(
                            &3_u8.to_be_bytes(),
                            &Winner {
                                winners: new_addresses,
                            },
                        )?;
                    }
                    4 => {
                        // Prizes four rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[3] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        // Remove the address from the array and save
                        let new_addresses = remove_from_storage(deps, env.clone(), &winner);
                        winner_storage(&mut deps.storage).save(
                            &4_u8.to_be_bytes(),
                            &Winner {
                                winners: new_addresses,
                            },
                        )?;
                    }
                    5 => {
                        // Prizes five rank
                        ticket_winning += Uint128(1);
                        // Remove the address from the array and save
                        let new_addresses = remove_from_storage(deps, env.clone(), &winner);
                        winner_storage(&mut deps.storage).save(
                            &5_u8.to_be_bytes(),
                            &Winner {
                                winners: new_addresses,
                            },
                        )?;
                    }
                    _ => (),
                }
            }
        }
    }

    // Build the amount transaction
    let mut amount_to_send: Vec<Coin> = vec![];

    if !jackpot_amount.is_zero() {
        // Get the contract balance
        let balance = deps
            .querier
            .query_balance(&env.contract.address, &state.denomStable)
            .unwrap();
        // Ensure the contract have the balance
        if balance.amount.is_zero() {
            return Err(StdError::generic_err("Empty contract balance"));
        }
        // Ensure the contract have sufficient balance to handle the transaction
        if balance.amount < jackpot_amount {
            return Err(StdError::generic_err("Not enough funds in the contract"));
        }
        amount_to_send.push(Coin {
            denom: state.denomStable.clone(),
            amount: jackpot_amount,
        });
    }

    if !ticket_winning.is_zero() {
        // Get the contract ticket balance
        let ticket_balance = deps
            .querier
            .query_balance(&env.contract.address, &state.denomStable)
            .unwrap();
        // Ensure the contract have the balance
        if !ticket_balance.amount.is_zero() || ticket_balance.amount > ticket_winning {
            amount_to_send.push(Coin {
                denom: state.denomStable,
                amount: ticket_winning,
            });
        }
    }

    // Check if no amount to send return ok
    if amount_to_send.is_empty() {
        return Ok(HandleResponse {
            messages: vec![],
            log: vec![
                LogAttribute {
                    key: "action".to_string(),
                    value: "jackpot reward".to_string(),
                },
                LogAttribute {
                    key: "to".to_string(),
                    value: env.message.sender.to_string(),
                },
                LogAttribute {
                    key: "jackpot_prize".to_string(),
                    value: "no".to_string(),
                },
            ],
            data: None,
        });
    }

    let msg = BankMsg::Send {
        from_address: env.contract.address,
        to_address: deps
            .api
            .human_address(&deps.api.canonical_address(&env.message.sender).unwrap())
            .unwrap(),
        amount: amount_to_send,
    };

    // Send the jackpot
    Ok(HandleResponse {
        messages: vec![msg.into()],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "jackpot reward".to_string(),
            },
            LogAttribute {
                key: "to".to_string(),
                value: env.message.sender.to_string(),
            },
            LogAttribute {
                key: "jackpot_prize".to_string(),
                value: "yes".to_string(),
            },
        ],
        data: None,
    })
}

pub fn handle_proposal<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    description: String,
    proposal: Proposal,
    amount: Option<Uint128>,
    prize_per_rank: Option<Vec<u8>>,
) -> StdResult<HandleResponse> {
    let mut state = config(&mut deps.storage).load().unwrap();
    // Increment and get the new poll id for bucket key
    let poll_id = state.pollCount + 1;
    // Set the new counter
    state.pollCount = poll_id;

    //Handle sender is not sending funds
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with proposal"));
    }

    // Handle the description is respecting length
    if (description.len() as u64) < MIN_DESC_LEN {
        return Err(StdError::generic_err(format!(
            "Description min length {}",
            MIN_DESC_LEN.to_string()
        )));
    } else if (description.len() as u64) > MAX_DESC_LEN {
        return Err(StdError::generic_err(format!(
            "Description max length {}",
            MAX_DESC_LEN.to_string()
        )));
    }

    let mut proposal_amount: Uint128 = Uint128::zero();
    let mut proposal_prize_rank: Vec<u8> = vec![];

    let proposal_type = if let Proposal::HolderFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.holdersMaxPercentageReward {
                    return Err(StdError::generic_err("Amount between 0 to 100".to_string()));
                }
                proposal_amount = percentage;
            }
            None => {
                return Err(StdError::generic_err("Amount required".to_string()));
            }
        }

        Proposal::HolderFeePercentage
    } else if let Proposal::DrandWorkerFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.workerDrandMaxPercentageReward {
                    return Err(StdError::generic_err("Amount between 0 to 100".to_string()));
                }
                proposal_amount = percentage;
            }
            None => {
                return Err(StdError::generic_err("Amount required".to_string()));
            }
        }

        Proposal::DrandWorkerFeePercentage
    } else if let Proposal::JackpotRewardPercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > 100 {
                    return Err(StdError::generic_err("Amount between 0 to 100".to_string()));
                }
                proposal_amount = percentage;
            }
            None => {
                return Err(StdError::generic_err("Amount required".to_string()));
            }
        }

        Proposal::JackpotRewardPercentage
    } else if let Proposal::LotteryEveryBlockTime = proposal {
        match amount {
            Some(blockTime) => {
                proposal_amount = blockTime;
            }
            None => {
                return Err(StdError::generic_err(
                    "Amount block time required".to_string(),
                ));
            }
        }

        Proposal::LotteryEveryBlockTime
    } else if let Proposal::PrizePerRank = proposal {
        match prize_per_rank {
            Some(ranks) => {
                if ranks.len() != 4 {
                    return Err(StdError::generic_err(
                        "Ranks need to be in this format [0, 90, 10, 0] numbers between 0 to 100"
                            .to_string(),
                    ));
                }
                let mut total_percentage = 0;
                for rank in ranks.clone() {
                    if (rank as u8) > 100 {
                        return Err(StdError::generic_err(
                            "Numbers between 0 to 100".to_string(),
                        ));
                    }
                    total_percentage += rank;
                }
                // Ensure the repartition sum is 100%
                if total_percentage != 100 {
                    return Err(StdError::generic_err(
                        "Numbers total sum need to be equal to 100".to_string(),
                    ));
                }

                proposal_prize_rank = ranks;
            }
            None => {
                return Err(StdError::generic_err("Rank is required".to_string()));
            }
        }
        Proposal::PrizePerRank
    } else if let Proposal::ClaimEveryBlock = proposal {
        match amount {
            Some(blockTime) => {
                proposal_amount = blockTime;
            }
            None => {
                return Err(StdError::generic_err("amount is required".to_string()));
            }
        }
        Proposal::ClaimEveryBlock
    } else if let Proposal::AmountToRegister = proposal {
        match amount {
            Some(AmountToRegister) => {
                proposal_amount = AmountToRegister;
            }
            None => {
                return Err(StdError::generic_err("amount is required".to_string()));
            }
        }
        Proposal::AmountToRegister
    } else {
        return Err(StdError::generic_err(
            "Proposal type not founds".to_string(),
        ));
    };

    let sender_to_canonical = deps.api.canonical_address(&env.message.sender).unwrap();

    let new_poll = PollInfoState {
        creator: sender_to_canonical,
        status: PollStatus::InProgress,
        end_height: env.block.height + state.pollEndHeight,
        start_height: env.block.height,
        description,
        yes_voters: vec![],
        no_voters: vec![],
        amount: proposal_amount,
        prizeRank: proposal_prize_rank,
        proposal: proposal_type,
    };

    // Save poll
    poll_storage(&mut deps.storage).save(&state.pollCount.to_be_bytes(), &new_poll)?;

    // Save state
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "create a proposal".to_string(),
            },
            LogAttribute {
                key: "proposal_id".to_string(),
                value: poll_id.to_string(),
            },
            LogAttribute {
                key: "proposal_creator".to_string(),
                value: env.message.sender.to_string(),
            },
            LogAttribute {
                key: "proposal_creation_result".to_string(),
                value: "success".to_string(),
            },
        ],
        data: None,
    })
}

pub fn handle_vote<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    poll_id: u64,
    approve: bool,
) -> StdResult<HandleResponse> {
    let store = poll_storage(&mut deps.storage).load(&poll_id.to_be_bytes())?;
    let sender = deps.api.canonical_address(&env.message.sender).unwrap();

    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with vote"));
    }
    // Ensure the poll is still valid
    if env.block.height > store.end_height {
        return Err(StdError::generic_err("Proposal expired"));
    }
    // Ensure the voter can't vote more times
    if store.yes_voters.contains(&sender) || store.no_voters.contains(&sender) {
        return Err(StdError::generic_err("Already voted"));
    }

    match approve {
        true => {
            poll_storage(&mut deps.storage).update::<_>(&poll_id.to_be_bytes(), |poll| {
                let mut poll_data = poll.unwrap();
                poll_data.yes_voters.push(sender.clone());
                Ok(poll_data)
            })?;
        }
        false => {
            poll_storage(&mut deps.storage).update::<_>(&poll_id.to_be_bytes(), |poll| {
                let mut poll_data = poll.unwrap();
                poll_data.no_voters.push(sender.clone());
                Ok(poll_data)
            })?;
        }
    }

    Ok(HandleResponse {
        messages: vec![],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "vote".to_string(),
            },
            LogAttribute {
                key: "proposalId".to_string(),
                value: poll_id.to_string(),
            },
            LogAttribute {
                key: "voting_result".to_string(),
                value: "success".to_string(),
            },
        ],
        data: None,
    })
}

pub fn handle_reject_proposal<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    poll_id: u64,
) -> StdResult<HandleResponse> {
    let store = poll_storage_read(&mut deps.storage).load(&poll_id.to_be_bytes())?;
    let sender = deps.api.canonical_address(&env.message.sender).unwrap();

    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err(
            "Do not send funds with reject proposal",
        ));
    }
    // Ensure end proposal height is not expired
    if store.end_height < env.block.height {
        return Err(StdError::generic_err("Proposal expired"));
    }
    // Ensure only the creator can reject a proposal OR the status of the proposal is still in progress
    if store.creator != sender || store.status != PollStatus::InProgress {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    poll_storage(&mut deps.storage).update::<_>(&poll_id.to_be_bytes(), |poll| {
        let mut poll_data = poll.unwrap();
        // Update the status to rejected by the creator
        poll_data.status = PollStatus::RejectedByCreator;
        // Update the end eight to now
        poll_data.end_height = env.block.height;
        Ok(poll_data)
    })?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "creator reject the proposal".to_string(),
            },
            LogAttribute {
                key: "proposal_id".to_string(),
                value: poll_id.to_string(),
            },
        ],
        data: None,
    })
}

fn total_weight<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    state: &State,
    addresses: &[CanonicalAddr],
) -> Uint128 {
    let mut weight = Uint128::zero();
    for address in addresses {
        let human_address = deps.api.human_address(&address).unwrap();

        let balance = deps
            .querier
            .query_balance(human_address, &state.denomShare)
            .unwrap();

        if !balance.amount.is_zero() {
            weight += balance.amount;
        }
    }
    weight
}

pub fn handle_present_proposal<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    poll_id: u64,
) -> StdResult<HandleResponse> {
    // Load storage
    let mut state = config(&mut deps.storage).load().unwrap();
    let store = poll_storage_read(&mut deps.storage)
        .load(&poll_id.to_be_bytes())
        .unwrap();

    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err(
            "Do not send funds with present proposal",
        ));
    }
    // Ensure the proposal is still in Progress
    if store.status != PollStatus::InProgress {
        return Err(StdError::Unauthorized { backtrace: None });
    }
    // Ensure the proposal is ended
    if store.end_height > env.block.height {
        return Err(StdError::generic_err("Proposal still in progress"));
    }
    // Calculating the weight
    let yes_weight = total_weight(&deps, &state, &store.yes_voters);
    // let noWeight = total_weight(&deps, &state, &store.no_voters);

    // Get the amount
    let mut final_vote_weight_in_percentage: u128 = 0;
    if !yes_weight.is_zero() {
        let yes_weight_by_hundred = yes_weight.u128() * 100;
        final_vote_weight_in_percentage = yes_weight_by_hundred / state.tokenHolderSupply.u128();
    }

    // Reject the proposal
    if final_vote_weight_in_percentage < 60 {
        poll_storage(&mut deps.storage).update::<_>(&poll_id.to_be_bytes(), |poll| {
            let mut poll_data = poll.unwrap();
            // Update the status to rejected
            poll_data.status = PollStatus::Rejected;
            Ok(poll_data)
        })?;
        return Ok(HandleResponse {
            messages: vec![],
            log: vec![
                LogAttribute {
                    key: "action".to_string(),
                    value: "present the proposal".to_string(),
                },
                LogAttribute {
                    key: "proposal_id".to_string(),
                    value: poll_id.to_string(),
                },
                LogAttribute {
                    key: "proposal_result".to_string(),
                    value: "rejected".to_string(),
                },
            ],
            data: None,
        });
    };

    // Valid the proposal
    match store.proposal {
        Proposal::LotteryEveryBlockTime => {
            state.everyBlockTimePlay = store.amount.u128() as u64;
        }
        Proposal::DrandWorkerFeePercentage => {
            state.feeForDrandWorkerInPercentage = store.amount.u128() as u8;
        }
        Proposal::JackpotRewardPercentage => {
            state.jackpotPercentageReward = store.amount.u128() as u8;
        }
        Proposal::AmountToRegister => {
            state.pricePerTicketToRegister = store.amount;
        }
        Proposal::PrizePerRank => {
            state.prizeRankWinnerPercentage = store.prizeRank;
        }
        Proposal::HolderFeePercentage => {
            state.holdersMaxPercentageReward = store.amount.u128() as u8
        }
        _ => {
            return Err(StdError::generic_err("Proposal not funds"));
        }
    }

    // Save to storage
    poll_storage(&mut deps.storage).update::<_>(&poll_id.to_be_bytes(), |poll| {
        let mut poll_data = poll.unwrap();
        // Update the status to passed
        poll_data.status = PollStatus::Passed;
        Ok(poll_data)
    })?;

    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![
            LogAttribute {
                key: "action".to_string(),
                value: "present the proposal".to_string(),
            },
            LogAttribute {
                key: "proposal_id".to_string(),
                value: poll_id.to_string(),
            },
            LogAttribute {
                key: "proposal_result".to_string(),
                value: "approved".to_string(),
            },
        ],
        data: None,
    })
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    let response = match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?)?,
        QueryMsg::Combination {} => to_binary(&query_all_combination(deps)?)?,
        QueryMsg::Winner {} => to_binary(&query_all_winner(deps)?)?,
        QueryMsg::GetPoll { pollId } => to_binary(&query_poll(deps, pollId)?)?,
        QueryMsg::GetRound {} => to_binary(&query_round(deps)?)?,
        QueryMsg::GetTerrand { round: _ } => to_binary(&query_terrand(deps)?)?,
    };
    Ok(response)
}

fn query_config<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<ConfigResponse> {
    let state = config_read(&deps.storage).load()?;
    Ok(state)
}

fn query_terrand<S: Storage, A: Api, Q: Querier>(_deps: &Extern<S, A, Q>) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}

fn query_all_combination<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<AllCombinationResponse> {
    let combinations = combination_storage_read(&deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item| {
            item.and_then(|(k, combination)| {
                Ok(CombinationInfo {
                    key: String::from_utf8(k).unwrap(),
                    addresses: combination.addresses,
                })
            })
        })
        .collect();

    Ok(AllCombinationResponse {
        combination: combinations,
    })
}
fn vector_as_u8_1_array(vector: Vec<u8>) -> [u8; 1] {
    let mut arr = [0u8; 1];
    for (place, element) in arr.iter_mut().zip(vector.iter()) {
        *place = *element;
    }
    arr
}
fn query_all_winner<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<AllWinnerResponse> {
    let winners = winner_storage_read(&deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item| {
            item.and_then(|(k, winner)| {
                Ok(WinnerInfo {
                    rank: u8::from_be_bytes(vector_as_u8_1_array(k)),
                    winners: winner.winners,
                })
            })
        })
        .collect();
    Ok(AllWinnerResponse { winner: winners })
}

fn query_poll<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    poll_id: u64,
) -> StdResult<GetPollResponse> {
    let store = poll_storage_read(&deps.storage);

    let poll = match store.may_load(&poll_id.to_be_bytes())? {
        Some(poll) => Some(poll),
        None => {
            return Err(StdError::NotFound {
                kind: "not found".to_string(),
                backtrace: None,
            })
        }
    }
    .unwrap();

    Ok(GetPollResponse {
        creator: deps.api.human_address(&poll.creator).unwrap(),
        status: poll.status,
        end_height: poll.end_height,
        start_height: poll.start_height,
        description: poll.description,
        amount: poll.amount,
        prizePerRank: poll.prizeRank,
    })
}

fn query_round<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<RoundResponse> {
    let state = config_read(&deps.storage).load()?;
    let from_genesis = state.blockTimePlay - DRAND_GENESIS_TIME;
    let next_round = (from_genesis / DRAND_PERIOD) + NEXT_ROUND;

    Ok(RoundResponse {
        nextRound: next_round,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{HandleMsg, InitMsg, QueryMsg};
    use cosmwasm_std::testing::{
        mock_dependencies, mock_env, BankQuerier, MockApi, MockQuerier, MockStorage,
        MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Api, Binary, CanonicalAddr, CosmosMsg, Decimal, FullDelegation,
        HumanAddr, MessageInfo, QuerierResult, StdError::NotFound, Storage, Uint128, Validator,
    };
    use cosmwasm_storage::{bucket, bucket_read, singleton, singleton_read};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use std::borrow::Borrow;
    use std::collections::HashMap;
    use cosmwasm_std::StdError::GenericErr;

    fn default_init<S: Storage, A: Api, Q: Querier>(mut deps: &mut Extern<S, A, Q>) {
        const DENOM_STABLE: &str = "ust";
        const DENOM_STABLE_DECIMAL: Uint128 = Uint128(1_000_000);
        const DENOM_SHARE: &str = "lota";
        const BLOCK_TIME_PLAY: u64 = 1610566920;
        const EVERY_BLOCK_TIME_PLAY: u64 = 50000;
        const PUBLIC_SALE_END_BLOCK: u64 = 1000000000;
        const POLL_END_HEIGHT: u64 = 40_000;
        const TOKEN_HOLDER_SUPPLY: Uint128 = Uint128(300_000);

        let init_msg = InitMsg {
            denomStableDecimal: DENOM_STABLE_DECIMAL,
            denomStable: DENOM_STABLE.to_string(),
            denomShare: DENOM_SHARE.to_string(),
            blockTimePlay: BLOCK_TIME_PLAY,
            everyBlockTimePlay: EVERY_BLOCK_TIME_PLAY,
            publicSaleEndBlock: PUBLIC_SALE_END_BLOCK,
            pollEndHeight: POLL_END_HEIGHT,
            tokenHolderSupply: TOKEN_HOLDER_SUPPLY,
            terrandContractAddress: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k"),
        };

        init(
            &mut deps,
            mock_env("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k", &[]),
            init_msg,
        )
        .unwrap();
    }

    struct BeforeAll {
        default_length: usize,
        default_sender: HumanAddr,
        default_sender_two: HumanAddr,
    }
    fn before_all() -> BeforeAll {
        BeforeAll {
            default_length: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k").len(),
            default_sender: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007"),
            default_sender_two: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q008"),
        }
    }

    #[test]
    fn proper_init() {
        let before_all = before_all();
        let mut deps = mock_dependencies(before_all.default_length, &[]);

        default_init(&mut deps);
    }
    #[test]
    fn get_round_play() {
        let before_all = before_all();
        let mut deps = mock_dependencies(before_all.default_length, &[]);
        default_init(&mut deps);
        let res = query_round(&deps).unwrap();
        println!("{:?}", res.nextRound);
    }
    #[test]
    fn testing_saved_address_winner() {
        let before_all = before_all();
        let mut deps = mock_dependencies(
            before_all.default_length,
            &[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }],
        );
        default_init(&mut deps);

        let winner_address = deps
            .api
            .canonical_address(&HumanAddr::from("address".to_string()))
            .unwrap();
        let winner_address2 = deps
            .api
            .canonical_address(&HumanAddr::from("address2".to_string()))
            .unwrap();
        winner_storage(&mut deps.storage).save(
            &2_u8.to_be_bytes(),
            &Winner {
                winners: vec![
                    WinnerInfoState {
                        claimed: false,
                        address: winner_address,
                    },
                    WinnerInfoState {
                        claimed: true,
                        address: winner_address2,
                    },
                ],
            },
        );
        let res = query_all_winner(&deps).unwrap();
        println!("{:?}", res);
    }
    mod register {
        use super::*;

        #[test]
        fn register_success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender.clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();
            assert_eq!(
                res,
                HandleResponse {
                    messages: vec![],
                    log: vec![LogAttribute {
                        key: "action".to_string(),
                        value: "register".to_string()
                    }],
                    data: None
                }
            );
            // Check combination added with success
            let store = combination_storage(&mut deps.storage).load(&"1e3fab".as_bytes()).unwrap();
            assert_eq!(1, store.addresses.len());
            let player1 = deps.api.canonical_address(&before_all.default_sender).unwrap();
            assert!(store.addresses.contains(&player1));
            //New player
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender_two.clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
                .unwrap();
            let store = combination_storage(&mut deps.storage).load(&"1e3fab".as_bytes()).unwrap();
            let player2 = deps.api.canonical_address(&before_all.default_sender_two).unwrap();
            assert_eq!(2, store.addresses.len());
            assert!(store.addresses.contains(&player1));
            assert!(store.addresses.contains(&player2));
        }
        #[test]
        fn register_fail_if_sender_sent_empty_funds(){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(0),
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "you need to send 1000000ust in order to register")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn register_fail_if_sender_sent_multiple_denom() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    },Coin {
                        denom: "wrong".to_string(),
                        amount: Uint128(10),
                    }],
                ),
                msg.clone(),
            );

            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Only send ust to register")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn register_fail_if_sender_sent_wrong_denom() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "wrong".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            );

            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "To register you need to send 1000000ust")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn register_fail_wrong_combination() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3far".to_string(),
            };
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Not authorized use combination of [a-f] and [0-9] with length 6")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn register_fail_sent_too_much_or_less() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fae".to_string(),
            };
            // Fail sending less than required (1_000_000)
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender.clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_00),
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "send 1000000ust")
                },
                _ => panic!("Unexpected error")
            }
            // Fail sending more than required (1_000_000)
            let res = handle(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_001),
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "send 1000000ust")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn register_fail_lottery_about_to_start() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let msg = HandleMsg::Register {
                combination: "1e3fae".to_string(),
            };
            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(
                before_all.default_sender,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            // Block time is superior to blockTimePlay so the lottery is about to start
            env.block.time = state.blockTimePlay + 1000;
            let res = handle(
                &mut deps,
                env,
                msg.clone(),
            );
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Lottery is about to start wait until the end before register")
                },
                _ => panic!("Unexpected error")
            }
        }

    }
    mod public_sale {
        use super::*;

        #[test]
        fn contract_balance_sold_out (){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let res = handle_public_sale(&mut deps, mock_env(before_all.default_sender, &[Coin{ denom: "ust".to_string(), amount: Uint128(1_000) }]));
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "All tokens have been sold")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn contract_balance_not_enough (){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let res = handle_public_sale(&mut deps, mock_env(before_all.default_sender, &[Coin{ denom: "ust".to_string(), amount: Uint128(1_000) }]));
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "you want to buy 1000 the contract balance only remain 900 token on public sale")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn sent_wrong_denom (){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let res = handle_public_sale(&mut deps, mock_env(before_all.default_sender, &[Coin{ denom: "wrong".to_string(), amount: Uint128(1_000) }]));
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Only ust is accepted")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn sent_multiple_denom_right_included (){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let res = handle_public_sale(&mut deps, mock_env(before_all.default_sender, &[Coin{ denom: "wrong".to_string(), amount: Uint128(1_000) }, Coin{ denom: "ust".to_string(), amount: Uint128(1_000) }]));

            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Send only ust, no extra denom")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn sent_empty(){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let res = handle_public_sale(&mut deps, mock_env(before_all.default_sender, &[Coin{ denom: "ust".to_string(), amount: Uint128(0) }]));
            println!("{:?}", res);
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Send some funds")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn public_sale_ended(){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(before_all.default_sender, &[Coin{ denom: "ust".to_string(), amount: Uint128(1_000) }]);
            env.block.height = state.publicSaleEndBlock + 1000;
            let res = handle_public_sale(&mut deps, env);
            match res {
                Err(GenericErr{msg, backtrace: None, }) => {
                    assert_eq!(msg, "Public sale is ended")
                },
                _ => panic!("Unexpected error")
            }
        }
        #[test]
        fn success(){
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[Coin{ denom: "lota".to_string(), amount: Uint128(900) }]);
            default_init(&mut deps);
            let stateBefore = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(before_all.default_sender.clone(), &[Coin{ denom: "ust".to_string(), amount: Uint128(100) }]);
            let res = handle_public_sale(&mut deps, env.clone()).unwrap();
            let stateAfter = config(&mut deps.storage).load().unwrap();

            assert_eq!(res.messages[0], CosmosMsg::Bank(BankMsg::Send {
                from_address: env.contract.address.clone(),
                to_address: before_all.default_sender,
                amount: vec![Coin { denom: "lota".to_string(), amount: Uint128(100) }]
            }));
            assert!(stateAfter.tokenHolderSupply > stateBefore.tokenHolderSupply);
        }
    }
    mod play {
        use super::*;

    }
    /*

    mod play {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        fn init_combination(deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>) {
            // Init the bucket with players to storage
            let combination = "6139d0";
            let combination2 = "6139d3";
            let combination3 = "613943";
            let combination4 = "613543";
            let combination5 = "612543";
            let addresses1 = vec![
                deps.api
                    .canonical_address(&HumanAddr("address1".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap(),
            ];
            let addresses2 = vec![
                deps.api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address3".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address3".to_string()))
                    .unwrap(),
            ];
            let addresses3 = vec![deps
                .api
                .canonical_address(&HumanAddr("address2".to_string()))
                .unwrap()];
            combination_storage(&mut deps.storage).save(
                &combination.as_bytes(),
                &Combination {
                    addresses: addresses1.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination2.as_bytes(),
                &Combination {
                    addresses: addresses2.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination3.as_bytes(),
                &Combination {
                    addresses: addresses3.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination4.as_bytes(),
                &Combination {
                    addresses: addresses2.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination5.as_bytes(),
                &Combination {
                    addresses: addresses1.clone(),
                },
            );
        }
        #[test]
        fn success() {
            let signature  = hex::decode("a619b278ab6266309c66254a82bf2404381aae232f083df1abb37f9825ba17b7618e1e0fc0503215c39e855775183be50d8ecefae9ec03e0d87184728228841bb792f3d1f8bf84f2afb7e9217b2eaddcb372d0ffdb0ad730d24b2eaf3d0751e2").unwrap().into();
            let previous_signature  = hex::decode("b6b7f91ee0617a605a4f645dce7d5bdaf487483be949104d038394fa9adfc9289a2900fb9f39ab62983c7098680c495f038f6af6ed3b637594d01a9b068dc4aa3abcb9fbd150ab519260836e115c29c808f0dc40b50ddf1e34cc482b8626293a").unwrap().into();
            let round: u64 = 504539;
            let msg = HandleMsg::Play {};
            let mut deps = mock_dependencies(&[Coin {
                denom: "usdc".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            init_combination(&mut deps);
            // Get round
            let roundFromQuerier = query_round(deps.as_ref()).unwrap();
            // Test if combination have been added correctly
            let res = query_all_combination(deps.as_ref()).unwrap();
            assert_eq!(5, res.combination.len());

            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "usdc".to_string(),
                        amount: Uint128(80_000)
                    }]
                })
            );
            // Test if the five combination winners have been added correctly
            let res = query_all_winner(deps.as_ref()).unwrap();
            assert_eq!(5, res.winner.len());
            // Test if combination have been removed correctly
            let res = query_all_combination(deps.as_ref()).unwrap();
            assert_eq!(0, res.combination.len());

            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            println!("{}", res.holdersRewards.u128());
            // Test fees is now superior to 0
            assert_ne!(0, res.holdersRewards.u128());
            // Test block to play superior block height
            assert!(res.blockTimePlay > env.block.time);
            // Test if round was saved
            let res = query_latest(deps.as_ref()).unwrap();
            println!("{:?}", res);
            assert_eq!(roundFromQuerier.nextRound, res.round);

            // Test if winners have been added at rank 1
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert_eq!(2, res.winners.len());
            // Test if winners have been added at rank 2
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&2_u8.to_be_bytes())
                .unwrap();
            assert_eq!(3, res.winners.len());
            // Test if winners have been added at rank 3
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&3_u8.to_be_bytes())
                .unwrap();
            assert_eq!(1, res.winners.len());
            // Test if winners have been added at rank 4
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&4_u8.to_be_bytes())
                .unwrap();
            assert_eq!(3, res.winners.len());
            println!("{:?}", res);
            // Test if winners have been added at rank 5
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&5_u8.to_be_bytes())
                .unwrap();
            assert_eq!(2, res.winners.len());

            // PLay second time before block time end error
            init_combination(&mut deps);
            let signature  = hex::decode("a619b278ab6266309c66254a82bf2404381aae232f083df1abb37f9825ba17b7618e1e0fc0503215c39e855775183be50d8ecefae9ec03e0d87184728228841bb792f3d1f8bf84f2afb7e9217b2eaddcb372d0ffdb0ad730d24b2eaf3d0751e2").unwrap().into();
            let previous_signature  = hex::decode("b6b7f91ee0617a605a4f645dce7d5bdaf487483be949104d038394fa9adfc9289a2900fb9f39ab62983c7098680c495f038f6af6ed3b637594d01a9b068dc4aa3abcb9fbd150ab519260836e115c29c808f0dc40b50ddf1e34cc482b8626293a").unwrap().into();
            let msg = HandleMsg::Play {};
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());

            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn do_not_send_funds() {
            let signature  = hex::decode("a619b278ab6266309c66254a82bf2404381aae232f083df1abb37f9825ba17b7618e1e0fc0503215c39e855775183be50d8ecefae9ec03e0d87184728228841bb792f3d1f8bf84f2afb7e9217b2eaddcb372d0ffdb0ad730d24b2eaf3d0751e2").unwrap().into();
            let previous_signature  = hex::decode("b6b7f91ee0617a605a4f645dce7d5bdaf487483be949104d038394fa9adfc9289a2900fb9f39ab62983c7098680c495f038f6af6ed3b637594d01a9b068dc4aa3abcb9fbd150ab519260836e115c29c808f0dc40b50ddf1e34cc482b8626293a").unwrap().into();
            let round: u64 = 504539;
            let msg = HandleMsg::Play {};
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            init_combination(&mut deps);

            let info = mock_info(
                HumanAddr::from("validator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                }],
            );
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!(msg, "Play")
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn no_players_combination_empty() {
            let signature  = hex::decode("a619b278ab6266309c66254a82bf2404381aae232f083df1abb37f9825ba17b7618e1e0fc0503215c39e855775183be50d8ecefae9ec03e0d87184728228841bb792f3d1f8bf84f2afb7e9217b2eaddcb372d0ffdb0ad730d24b2eaf3d0751e2").unwrap().into();
            let previous_signature  = hex::decode("b6b7f91ee0617a605a4f645dce7d5bdaf487483be949104d038394fa9adfc9289a2900fb9f39ab62983c7098680c495f038f6af6ed3b637594d01a9b068dc4aa3abcb9fbd150ab519260836e115c29c808f0dc40b50ddf1e34cc482b8626293a").unwrap().into();
            let round: u64 = 504539;
            let msg = HandleMsg::Play {};
            let mut deps = mock_dependencies(&[Coin {
                denom: "usdc".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);

            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "usdc".to_string(),
                        amount: Uint128(80_000)
                    }]
                })
            );

            let res = winner_storage_read(deps.as_ref().storage).load(&[]);
            let winners = res.and_then(|i| Ok(i));
            match winners {
                Err(StdError::NotFound { kind, .. }) => assert_eq!(kind, "lottery::state::Winner"),
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod holder_claim_reward {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn contract_balance_empty() {
            let mut deps = mock_dependencies(&[]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            // Add rewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.tokenHolderSupply = Uint128(1_000_000);
            config(deps.as_mut().storage).save(&state);
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_to_claim() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("validator1"),
                &[Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(532_004),
                }],
            );
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!(msg, "Reward")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn shares_to_low() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "usdc".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532),
                }],
            );
            default_init(&mut deps);
            // Add rewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.tokenHolderSupply = Uint128(1_000_000);
            config(deps.as_mut().storage).save(&state);

            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::SharesTooLow {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "usdc".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(500_000),
                }],
            );
            default_init(&mut deps);
            // Add rewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.tokenHolderSupply = Uint128(1_000_000);
            state.holdersRewards = Uint128(50_000);
            config(deps.as_mut().storage).save(&state);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "usdc".to_string(),
                        amount: Uint128(25000)
                    }]
                })
            );
            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            // Test if claimReward is not empty and the right address was added correctly
            assert_ne!(0, res.claimReward.len());
            assert!(res.claimReward.contains(
                &deps
                    .api
                    .canonical_address(&HumanAddr::from("validator1"))
                    .unwrap()
            ));
            // Test reward amount was updated with success
            assert_ne!(5_221, res.holdersRewards.u128());

            // Error do not claim multiple times
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::AlreadyClaimed {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }

    mod winner_claim_jackpot {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn do_not_send_funds() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            // Test sender is not sending funds
            let info = mock_info(
                HumanAddr::from("address2"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                }],
            );
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!("Jackpot", msg);
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_reward() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            let mut state = config_read(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(0);
            config(deps.as_mut().storage).save(&state);
            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_winner() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);

            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::NoWinners {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn some_winners_with_contract_ticket_prize_balance_empty() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);
            // Test with some winners and empty balance ticket
            deps.querier.update_balance(
                MOCK_CONTRACT_ADDR,
                vec![Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                }],
            );
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );
            // Test transaction succeed if not ticket ujack to send
            // in this case the winner only have won tickets but since the balance is empty he will get nothing
            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(0, res.messages.len());
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "jackpot reward"),
                    attr("to", "address2"),
                    attr("jackpot_prize", "no")
                ]
            )
        }
        #[test]
        fn prioritize_coin_prize_rather_than_ticket_prize() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(0),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);

            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address1"),
                    amount: vec![Coin {
                        denom: "usdc".to_string(),
                        amount: Uint128(84_000)
                    }]
                })
            );
        }
        #[test]
        fn handle_contract_balance_is_empty_ticket() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(0),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(0),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            // Test error since the winner won uscrt and is more important prize than ujack ticket
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_is_empty() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);

            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success_claim_only_ticket() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(0),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(10),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);

            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address2"),
                    amount: vec![Coin {
                        denom: "ujack".to_string(),
                        amount: Uint128(1)
                    }]
                })
            );
        }

        #[test]
        fn success_claim() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(10),
                },
            ]);
            default_init(&mut deps);
            // Add some jackpotRewards
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(100_000);
            config(deps.as_mut().storage).save(&state);
            // Init winner storage for test
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            // Test success address3 claim jackpot and get 3 ticket since he won 3 times
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address1"),
                    amount: vec![
                        Coin {
                            denom: "usdc".to_string(),
                            amount: Uint128(84_000)
                        },
                        Coin {
                            denom: "ujack".to_string(),
                            amount: Uint128(1)
                        }
                    ]
                })
            );
            // test if sender is claimed true
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert!(res.winners[0].claimed);

            // Test sender can't claim jackpot anymore since claimed is true
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::AlreadyClaimed {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }

    #[test]
    fn proposal() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        // Test success proposal HolderFeePercentage
        let info = mock_info(HumanAddr::from("address2"), &[]);
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to a more expensive".to_string(),
            proposal: Proposal::HolderFeePercentage,
            amount: Option::from(Uint128(15)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::HolderFeePercentage, storage.proposal);
        assert_eq!(Uint128(15), storage.amount);

        // Test success proposal MinAmountValidator
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to a more expensive".to_string(),
            proposal: Proposal::MinAmountValidator,
            amount: Option::from(Uint128(15_000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&2_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::MinAmountValidator, storage.proposal);
        assert_eq!(Uint128(15000), storage.amount);

        // Test success proposal PrizePerRank
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new prize rank".to_string(),
            proposal: Proposal::PrizePerRank,
            amount: None,
            prizePerRank: Option::from(vec![60, 20, 10, 10]),
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&3_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::PrizePerRank, storage.proposal);
        assert_eq!(vec![60, 20, 10, 10], storage.prizeRank);

        // Test success proposal MinAmountDelegator
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new amount delegator".to_string(),
            proposal: Proposal::MinAmountDelegator,
            amount: Option::from(Uint128(10_000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&4_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::MinAmountDelegator, storage.proposal);
        assert_eq!(Uint128(10000), storage.amount);

        // Test success proposal JackpotRewardPercentage
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new jackpot percentage".to_string(),
            proposal: Proposal::JackpotRewardPercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&5_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::JackpotRewardPercentage, storage.proposal);
        assert_eq!(Uint128(10), storage.amount);

        // Test success proposal DrandWorkerFeePercentage
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new worker percentage".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&6_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::DrandWorkerFeePercentage, storage.proposal);
        assert_eq!(Uint128(10), storage.amount);

        // Test success proposal LotteryEveryBlockTime
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&7_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::LotteryEveryBlockTime, storage.proposal);
        assert_eq!(Uint128(100000), storage.amount);

        // Test success proposal ClaimEveryBlock
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::ClaimEveryBlock,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&8_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::ClaimEveryBlock, storage.proposal);
        assert_eq!(Uint128(100000), storage.amount);

        // Test saved correctly the state
        let res = config_read(deps.as_ref().storage).load().unwrap();
        assert_eq!(8, res.pollCount);

        // Test error description too short
        let msg = HandleMsg::Proposal {
            description: "I".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DescriptionTooShort(msg)) => {
                assert_eq!("6", msg);
            }
            _ => panic!("Unexpected error"),
        }

        // Test error description too long
        let msg = HandleMsg::Proposal {
            description: "I erweoi oweijr w oweijr woerwe oirjewoi rewoirj ewoirjewr weiorjewoirjwerieworijewewriewo werioj ew".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DescriptionTooLong(msg)) => {
                assert_eq!("64", msg);
            }
            _ => panic!("Unexpected error"),
        }

        // Test error description too long
        let msg = HandleMsg::Proposal {
            description: "Default".to_string(),
            proposal: Proposal::NotExist,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalNotFound {}) => {}
            _ => panic!("Unexpected error"),
        }

        // Test error sending funds with proposal
        let info = mock_info(
            HumanAddr::from("address2"),
            &[Coin {
                denom: "kii".to_string(),
                amount: Uint128(500),
            }],
        );
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DoNotSendFunds(msg)) => {
                assert_eq!("Proposal", msg);
            }
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn vote() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("address2"), &[]);

        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: false,
        };
        // Error not found storage key
        let errMsg = "lottery::state::PollInfoState".to_string();
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(Std(NotFound { kind: errMsg, .. })) => {}
            _ => panic!("Unexpected error"),
        }

        // Init proposal
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());

        // Test success proposal HolderFeePercentage
        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: false,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        // Test success added to the no voter array
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert!(storage.no_voters.contains(
            &deps
                .api
                .canonical_address(&HumanAddr::from("address2"))
                .unwrap()
        ));
        // Test only added 1 time
        assert_eq!(1, storage.no_voters.len());

        // Test only can vote one time per proposal
        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: true,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::AlreadyVoted {}) => {}
            _ => panic!("Unexpected error"),
        }
        // Test sender is not added to vote cause multiple times votes
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert!(!storage.yes_voters.contains(
            &deps
                .api
                .canonical_address(&HumanAddr::from("address2"))
                .unwrap()
        ));

        // Test proposal is expired
        let env = mock_env();
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the status to rejected by the creator
            pollData.status = PollStatus::RejectedByCreator;
            // Update the end eight to now
            pollData.end_height = env.block.height - 1;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalExpired {}) => {}
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn creator_reject_proposal() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("creator"), &[]);
        // Init proposal
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());

        // Init reject proposal
        let msg = HandleMsg::RejectProposal { pollId: 1 };
        let env = mock_env();

        // test error do not send funds with reject proposal
        let info = mock_info(
            HumanAddr::from("creator"),
            &[Coin {
                denom: "xcoin".to_string(),
                amount: Uint128(19_000),
            }],
        );
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DoNotSendFunds(msg)) => {
                assert_eq!("RejectProposal", msg)
            }
            _ => panic!("Unexpected error"),
        }

        // Success reject the proposal
        let info = mock_info(HumanAddr::from("creator"), &[]);
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        let store = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(env.block.height, store.end_height);
        assert_eq!(PollStatus::RejectedByCreator, store.status);

        // Test error since the sender is not the creator
        let info = mock_info(HumanAddr::from("otherUser"), &[]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Unexpected error"),
        }

        // Test error the proposal is not inProgress
        let info = mock_info(HumanAddr::from("creator"), &[]);
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the status to rejected by the creator
            pollData.status = PollStatus::RejectedByCreator;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Unexpected error"),
        }
        // Test error the proposal already expired
        let info = mock_info(HumanAddr::from("creator"), &[]);
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the end eight to now
            pollData.end_height = env.block.height - 1;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalExpired {}) => {}
            _ => panic!("Unexpected error"),
        }
    }

    mod present_proposal {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        fn init_proposal(deps: DepsMut, env: &Env) {
            let info = mock_info(HumanAddr::from("creator"), &[]);
            let msg = HandleMsg::Proposal {
                description: "I think we need to up to new block time".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(200_000)),
                prizePerRank: None,
            };
            handle(deps, env.clone(), info.clone(), msg.clone());
        }

        fn init_voter(address: String, deps: DepsMut, vote: bool, env: &Env) {
            let info = mock_info(HumanAddr::from(address.clone()), &[]);
            let msg = HandleMsg::Vote {
                pollId: 1,
                approve: vote,
            };
            handle(deps, env.clone(), info.clone(), msg.clone());
        }

        #[test]
        fn present_proposal_with_empty_voters() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "rejected")
                ]
            );
        }

        #[test]
        fn present_proposal_with_voters_NO_WEIGHT() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let stateBefore = config_read(deps.as_ref().storage).load().unwrap();
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), false, &env);
            init_voter("address2".to_string(), deps.as_mut(), false, &env);

            // Init present proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "rejected")
                ]
            );
            assert_eq!(0, res.messages.len());
            // check if state remain the same since the vote was rejected
            let stateAfter = config_read(deps.as_ref().storage).load().unwrap();
            assert_eq!(
                stateBefore.everyBlockTimePlay,
                stateAfter.everyBlockTimePlay
            );
        }
        #[test]
        fn present_proposal_with_voters_YES_WEIGHT() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let mut state = config(deps.as_mut().storage).load().unwrap();
            state.tokenHolderSupply = Uint128(1_000_000);
            config(deps.as_mut().storage).save(&state);

            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), true, &env);
            init_voter("address2".to_string(), deps.as_mut(), true, &env);

            // Init present proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "approved")
                ]
            );
            assert_eq!(0, res.messages.len());
            // check if state the vote was rejected
            let state = config_read(deps.as_ref().storage).load().unwrap();
            assert_eq!(200000, state.everyBlockTimePlay);

            // Test can't REpresent the proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn error_present_proposal() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), true, &env);
            init_voter("address2".to_string(), deps.as_mut(), true, &env);

            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let info = mock_info(
                HumanAddr::from("sender"),
                &[Coin {
                    denom: "funds".to_string(),
                    amount: Uint128(324),
                }],
            );
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!("PresentProposal", msg)
                }
                _ => panic!("Unexpected error"),
            }

            // proposal not expired
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::ProposalNotExpired {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }*/
}
