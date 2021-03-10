use cosmwasm_std::{
    to_binary, Api, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Decimal, Empty, Env, Extern,
    HandleResponse, HumanAddr, InitResponse, LogAttribute, Order, Querier, QueryRequest, StdError,
    StdResult, Storage, Uint128, WasmMsg, WasmQuery,
};

use crate::msg::{
    AllCombinationResponse, AllWinnerResponse, CombinationInfo, ConfigResponse, GetPollResponse,
    HandleMsg, InitMsg, QueryMsg, RoundResponse, WinnerInfo,
};
use crate::query::{LoterraBalanceResponse, TerrandResponse, GetHolderResponse, GetAllBondedResponse};
use crate::state::{
    combination_storage, combination_storage_read, config, config_read, poll_storage,
    poll_storage_read, winner_storage, winner_storage_read, Combination, PollInfoState, PollStatus,
    Proposal, State, Winner, WinnerInfoState,
};

use hex;
use std::ops::{Add, Mul, Sub};

const DRAND_GENESIS_TIME: u64 = 1595431050;
const DRAND_PERIOD: u64 = 30;
const DRAND_NEXT_ROUND_SECURITY: u64 = 10;
const MAX_DESCRIPTION_LEN: u64 = 255;
const MIN_DESCRIPTION_LEN: u64 = 6;

// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
// #[serde(rename_all = "snake_case")]
pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    let state = State {
        admin: deps.api.canonical_address(&env.message.sender)?,
        block_time_play: msg.block_time_play,
        every_block_time_play: msg.every_block_time_play,
        public_sale_end_block: msg.public_sale_end_block,
        denom_stable: msg.denom_stable,
        token_holder_supply: msg.token_holder_supply,
        poll_default_end_height: msg.poll_default_end_height,
        combination_len: 6,
        jackpot_reward: Uint128::zero(),
        jackpot_percentage_reward: 80,
        token_holder_percentage_fee_reward: 10,
        fee_for_drand_worker_in_percentage: 1,
        prize_rank_winner_percentage: vec![84, 10, 5, 1],
        poll_count: 0,
        holders_max_percentage_reward: 20,
        worker_drand_max_percentage_reward: 10,
        price_per_ticket_to_register: Uint128(1_000_000),
        terrand_contract_address: deps.api.canonical_address(&msg.terrand_contract_address)?,
        loterra_cw20_contract_address: deps
            .api
            .canonical_address(&msg.loterra_cw20_contract_address)?,
        lottera_staking_contract_address: deps
            .api
            .canonical_address(&msg.lottera_staking_contract_address)?,
        safe_lock: false,
        latest_winning_number: "".to_string(),
        dao_funds: msg.dao_funds,
        lottery_counter: 0,
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
        HandleMsg::Jackpot {} => handle_jackpot(deps, env),
        HandleMsg::Proposal {
            description,
            proposal,
            amount,
            prize_per_rank,
            contract_migration_address,
        } => handle_proposal(
            deps,
            env,
            description,
            proposal,
            amount,
            prize_per_rank,
            contract_migration_address,
        ),
        HandleMsg::Vote { poll_id, approve } => handle_vote(deps, env, poll_id, approve),
        HandleMsg::PresentProposal { poll_id } => handle_present_proposal(deps, env, poll_id),
        HandleMsg::RejectProposal { poll_id } => handle_reject_proposal(deps, env, poll_id),
        HandleMsg::SafeLock {} => handle_safe_lock(deps, env),
        HandleMsg::Renounce {} => handle_renounce(deps, env),
    }
}

pub fn handle_renounce<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    let sender = deps.api.canonical_address(&env.message.sender)?;
    if state.admin != sender {
        return Err(StdError::Unauthorized { backtrace: None });
    }
    if state.safe_lock {
        return Err(StdError::generic_err("Contract is locked"));
    }

    state.admin = deps.api.canonical_address(&env.contract.address)?;
    config(&mut deps.storage).save(&state)?;
    Ok(HandleResponse::default())
}

pub fn handle_safe_lock<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    let sender = deps.api.canonical_address(&env.message.sender)?;
    if state.admin != sender {
        return Err(StdError::Unauthorized { backtrace: None });
    }

    state.safe_lock = !state.safe_lock;
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse::default())
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
    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
    }
    // Regex to check if the combination is allowed
    if !is_lower_hex(&combination, state.combination_len) {
        return Err(StdError::generic_err(format!(
            "Not authorized use combination of [a-f] and [0-9] with length {}",
            state.combination_len
        )));
    }
    // Check if some funds are sent
    let sent = match env.message.sent_funds.len() {
        0 => Err(StdError::generic_err(format!(
            "you need to send {}{} in order to register",
            state.price_per_ticket_to_register.clone(),
            state.denom_stable.clone()
        ))),
        1 => {
            if env.message.sent_funds[0].denom == state.denom_stable {
                Ok(env.message.sent_funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "To register you need to send {}{}",
                    state.price_per_ticket_to_register,
                    state.denom_stable.clone()
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Only send {} to register",
            state.denom_stable.clone()
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "you need to send {}{} in order to register",
            state.price_per_ticket_to_register.clone(),
            state.denom_stable.clone()
        )));
    }
    // Handle the player is not sending too much or too less
    if sent.u128() != state.price_per_ticket_to_register.u128() {
        return Err(StdError::generic_err(format!(
            "send {}{}",
            state.price_per_ticket_to_register.clone(),
            state.denom_stable.clone()
        )));
    }

    // Check if the lottery is about to play and cancel new ticket to enter until play
    if env.block.time >= state.block_time_play {
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

fn encode_msg_query(msg: QueryMsg, address: HumanAddr) -> StdResult<QueryRequest<Empty>> {
    Ok(WasmQuery::Smart {
        contract_addr: address,
        msg: to_binary(&msg)?,
    }
    .into())
}
fn wrapper_msg_terrand<S: Storage, A: Api, Q: Querier>(
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

    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
    }

    let from_genesis = state.block_time_play - DRAND_GENESIS_TIME;
    let next_round = (from_genesis / DRAND_PERIOD) + DRAND_NEXT_ROUND_SECURITY;

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
    if env.block.time > state.block_time_play {
        // Update the state
        state.block_time_play = env.block.time + state.every_block_time_play;
    } else {
        return Err(StdError::generic_err(format!(
            "Lottery registration is still in progress... Retry after block time {}",
            state.block_time_play
        )));
    }

    let msg = QueryMsg::GetRandomness { round: next_round };
    let terrand_human = deps
        .api
        .human_address(&state.terrand_contract_address.clone())?;
    let res = encode_msg_query(msg, terrand_human)?;
    let res = wrapper_msg_terrand(&deps, res)?;
    let randomness_hash = hex::encode(res.randomness.as_slice());

    state.latest_winning_number = randomness_hash.clone();

    let n = randomness_hash
        .char_indices()
        .rev()
        .nth(state.combination_len as usize - 1)
        .map(|(i, _)| i)
        .unwrap();
    let winning_combination = &randomness_hash[n..];

    // Set jackpot amount
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &state.denom_stable)
        .unwrap();
    // Max amount winners can claim
    let jackpot = balance
        .amount
        .mul(Decimal::percent(state.jackpot_percentage_reward as u64));
    // Drand worker fee
    let fee_for_drand_worker = jackpot.mul(Decimal::percent(
        state.fee_for_drand_worker_in_percentage as u64,
    ));
    // Double adjust fee to divide by two
    let fee_for_drand_worker = fee_for_drand_worker.mul(Decimal::percent(
        state.fee_for_drand_worker_in_percentage as u64,
    ));
    // Amount token holders can claim of the reward is a fee
    let token_holder_fee_reward = jackpot.mul(Decimal::percent(
        state.token_holder_percentage_fee_reward as u64,
    ));
    // Total fees if winner of the jackpot
    let total_fee = jackpot.mul(Decimal::percent(
        state.fee_for_drand_worker_in_percentage.add(state.token_holder_percentage_fee_reward) as u64
    ));
    // The jackpot after worker fee applied
    let mut jackpot_after = jackpot.sub(fee_for_drand_worker).unwrap();
    let mut holders_rewards = Uint128::zero();
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
                // Set the new jackpot after fee
                jackpot_after = jackpot.sub(total_fee).unwrap();
                holders_rewards = holders_rewards.add(token_holder_fee_reward);

                let mut data_winner: Vec<WinnerInfoState> = vec![];
                for winner_address in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winner_address,
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
                for winner_address in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winner_address,
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
                for winner_address in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winner_address,
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
                for winner_address in combination.addresses {
                    data_winner.push(WinnerInfoState {
                        claimed: false,
                        address: winner_address,
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
            }
            // Re init the counter for the next players
            count = 0;
        }
    }

    let msg_fee_worker = BankMsg::Send {
        from_address: env.contract.address.clone(),
        to_address: res.worker,
        amount: vec![Coin {
            denom: state.denom_stable.clone(),
            amount: fee_for_drand_worker,
        }],
    };

    let mut all_msg = vec![msg_fee_worker.into()];

    if !holders_rewards.is_zero() {
        let msg_payout = QueryMsg::PayoutReward {};
        let lottera_human = deps
            .api
            .human_address(&state.lottera_staking_contract_address.clone())?;
        let res_payout = encode_msg_execute(
            msg_payout,
            lottera_human,
            vec![Coin {
                denom: state.denom_stable.clone(),
                amount: holders_rewards,
            }],
        )?;

        all_msg.push(res_payout.into());
    }

    // Update the state
    state.jackpot_reward = jackpot_after;
    state.lottery_counter += 1;
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
        messages: all_msg,
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

fn encode_msg_execute(msg: QueryMsg, address: HumanAddr, coin: Vec<Coin>) -> StdResult<CosmosMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: address,
        msg: to_binary(&msg)?,
        send: coin,
    }
    .into())
}
fn wrapper_msg_loterra<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<LoterraBalanceResponse> {
    let res: LoterraBalanceResponse = deps.querier.query(&query)?;

    Ok(res)
}

fn wrapper_msg_loterra_staking<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<GetHolderResponse> {
    let res: GetHolderResponse = deps.querier.query(&query)?;

    Ok(res)
}
fn wrapper_msg_loterra_staking_all_bonded<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<GetAllBondedResponse> {
    let res: GetAllBondedResponse = deps.querier.query(&query)?;

    Ok(res)
}

pub fn handle_public_sale<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load the state
    let mut state = config(&mut deps.storage).load()?;
    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
    }
    // Public sale expire after blockTime
    if state.public_sale_end_block < env.block.height {
        return Err(StdError::generic_err("Public sale is ended"));
    }
    // Check if some funds are sent
    let sent = match env.message.sent_funds.len() {
        0 => Err(StdError::generic_err(format!(
            "Send some {} to participate at public sale",
            state.denom_stable.clone()
        ))),
        1 => {
            if env.message.sent_funds[0].denom == state.denom_stable {
                Ok(env.message.sent_funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "Only {} is accepted",
                    state.denom_stable.clone()
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Send only {}, no extra denom",
            state.denom_stable.clone()
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "Send some {} to participate at public sale",
            state.denom_stable.clone()
        )));
    };
    // Get the contract balance prepare the tx
    let msg_balance = QueryMsg::Balance {
        address: env.contract.address,
    };
    let lottera_human = deps
        .api
        .human_address(&state.loterra_cw20_contract_address.clone())?;
    let res_balance = encode_msg_query(msg_balance, lottera_human)?;
    let lottera_balance = wrapper_msg_loterra(&deps, res_balance)?;

    let adjusted_contract_balance = lottera_balance.balance.sub(state.dao_funds)?;

    if adjusted_contract_balance.is_zero() {
        return Err(StdError::generic_err("All tokens have been sold"));
    }

    if adjusted_contract_balance.u128() < sent.u128() {
        return Err(StdError::generic_err(format!(
            "you want to buy {} the contract balance only remain {} token on public sale",
            sent.u128(),
            adjusted_contract_balance.u128()
        )));
    }

    let msg_transfer = QueryMsg::Transfer {
        recipient: env.message.sender.clone(),
        amount: sent,
    };
    let lottera_human = deps
        .api
        .human_address(&state.loterra_cw20_contract_address.clone())?;
    let res_transfer = encode_msg_execute(msg_transfer, lottera_human, vec![])?;

    state.token_holder_supply += sent;
    // Save the new state
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![res_transfer.into()],
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

// Players claim the jackpot
pub fn handle_jackpot<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    // Load state
    let state = config(&mut deps.storage).load()?;
    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
    }
    let sender_to_canonical = deps.api.canonical_address(&env.message.sender).unwrap();
    // Load winners
    let store = query_all_winner(&deps).unwrap();
    // Ensure the sender is not sending funds
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with jackpot"));
    }
    // Ensure there is jackpot reward to claim
    if state.jackpot_reward.is_zero() {
        return Err(StdError::generic_err("No jackpot reward"));
    }
    // Ensure there is some winner
    if store.winner.is_empty() {
        return Err(StdError::generic_err("No winners"));
    }

    let mut jackpot_amount: Uint128 = Uint128(0);
    let mut win_prize_position: Vec<u8> = vec![];
    for winner in store.winner.clone() {
        for winner_info in winner.winners.clone() {
            if winner_info.address == deps.api.canonical_address(&env.message.sender).unwrap() {
                if winner_info.claimed {
                    return Err(StdError::generic_err("Already claimed"));
                }

                match winner.rank {
                    1 => {
                        // Prizes first rank
                        let prize = state
                            .jackpot_reward
                            .mul(Decimal::percent(
                                state.prize_rank_winner_percentage[0] as u64,
                            ))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        win_prize_position.push(1);
                    }
                    2 => {
                        // Prizes second rank
                        let prize = state
                            .jackpot_reward
                            .mul(Decimal::percent(
                                state.prize_rank_winner_percentage[1] as u64,
                            ))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);

                        win_prize_position.push(2);
                    }
                    3 => {
                        // Prizes third rank
                        let prize = state
                            .jackpot_reward
                            .mul(Decimal::percent(
                                state.prize_rank_winner_percentage[2] as u64,
                            ))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        win_prize_position.push(3);
                    }
                    4 => {
                        // Prizes four rank
                        let prize = state
                            .jackpot_reward
                            .mul(Decimal::percent(
                                state.prize_rank_winner_percentage[3] as u64,
                            ))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpot_amount += Uint128(prize);
                        win_prize_position.push(4);
                    }
                    _ => (),
                }
            }
        }
    }

    // Get the contract balance
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &state.denom_stable)
        .unwrap();
    // Ensure the contract have the balance
    if balance.amount.is_zero() {
        return Err(StdError::generic_err("Empty contract balance"));
    }
    // Ensure the contract have sufficient balance to handle the transaction
    if balance.amount < jackpot_amount {
        return Err(StdError::generic_err("Not enough funds in the contract"));
    }
    // Ensure there is some reward to send
    if jackpot_amount.is_zero() {
        return Err(StdError::generic_err("No jackpot to claim, try next time"));
    }

    // Build the amount transaction
    let amount_to_send: Vec<Coin> = vec![Coin {
        denom: state.denom_stable.clone(),
        amount: jackpot_amount,
    }];

    let msg = BankMsg::Send {
        from_address: env.contract.address,
        to_address: deps
            .api
            .human_address(&deps.api.canonical_address(&env.message.sender).unwrap())
            .unwrap(),
        amount: amount_to_send,
    };

    // update the winner to claimed true
    for position in win_prize_position {
        winner_storage(&mut deps.storage).update::<_>(&position.to_be_bytes(), |winners| {
            let mut winners_data = winners.unwrap();
            for index in 0..winners_data.winners.clone().len() {
                if winners_data.winners[index].address == sender_to_canonical {
                    winners_data.winners[index].claimed = true;
                }
            }
            Ok(winners_data)
        })?;
    }

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
    contract_migration_address: Option<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut state = config(&mut deps.storage).load().unwrap();
    // Increment and get the new poll id for bucket key
    let poll_id = state.poll_count + 1;
    // Set the new counter
    state.poll_count = poll_id;

    //Handle sender is not sending funds
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with proposal"));
    }

    // Handle the description is respecting length
    if (description.len() as u64) < MIN_DESCRIPTION_LEN {
        return Err(StdError::generic_err(format!(
            "Description min length {}",
            MIN_DESCRIPTION_LEN.to_string()
        )));
    } else if (description.len() as u64) > MAX_DESCRIPTION_LEN {
        return Err(StdError::generic_err(format!(
            "Description max length {}",
            MAX_DESCRIPTION_LEN.to_string()
        )));
    }

    let mut proposal_amount: Uint128 = Uint128::zero();
    let mut proposal_prize_rank: Vec<u8> = vec![];
    let mut proposal_human_address: Option<HumanAddr> = None;

    let proposal_type = if let Proposal::HolderFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.holders_max_percentage_reward {
                    return Err(StdError::generic_err(format!(
                        "Amount between 0 to {}",
                        state.holders_max_percentage_reward
                    )));
                }
                proposal_amount = percentage;
            }
            None => {
                return Err(StdError::generic_err("Amount is required".to_string()));
            }
        }

        Proposal::HolderFeePercentage
    } else if let Proposal::DrandWorkerFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.worker_drand_max_percentage_reward {
                    return Err(StdError::generic_err(format!(
                        "Amount between 0 to {}",
                        state.worker_drand_max_percentage_reward
                    )));
                }
                proposal_amount = percentage;
            }
            None => {
                return Err(StdError::generic_err("Amount is required".to_string()));
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
                return Err(StdError::generic_err("Amount is required".to_string()));
            }
        }

        Proposal::JackpotRewardPercentage
    } else if let Proposal::LotteryEveryBlockTime = proposal {
        match amount {
            Some(block_time) => {
                proposal_amount = block_time;
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
    } else if let Proposal::AmountToRegister = proposal {
        match amount {
            Some(amount_to_register) => {
                proposal_amount = amount_to_register;
            }
            None => {
                return Err(StdError::generic_err("Amount is required".to_string()));
            }
        }
        Proposal::AmountToRegister
    } else if let Proposal::SecurityMigration = proposal {
        match contract_migration_address {
            Some(migration_address) => {
                let sender = deps.api.canonical_address(&env.message.sender)?;
                if state.admin != sender {
                    return Err(StdError::Unauthorized { backtrace: None });
                }
                proposal_human_address = Option::from(migration_address);
            }
            None => {
                return Err(StdError::generic_err(
                    "Migration address is required".to_string(),
                ));
            }
        }
        Proposal::SecurityMigration
    } else if let Proposal::DaoFunding = proposal {
        match amount {
            Some(amount) => {
                if amount.is_zero() {
                    return Err(StdError::generic_err("Amount be higher than 0".to_string()));
                }
                if state.dao_funds.is_zero() {
                    return Err(StdError::generic_err(
                        "No more funds to fund project".to_string(),
                    ));
                }
                if state.dao_funds.u128() < amount.u128() {
                    return Err(StdError::generic_err(format!(
                        "You need {} we only can fund you up to {}",
                        amount, state.dao_funds
                    )));
                }
                proposal_amount = amount;
            }
            None => {
                return Err(StdError::generic_err("Amount required".to_string()));
            }
        }
        Proposal::DaoFunding
    } else {
        return Err(StdError::generic_err(
            "Proposal type not founds".to_string(),
        ));
    };

    let sender_to_canonical = deps.api.canonical_address(&env.message.sender).unwrap();

    let new_poll = PollInfoState {
        creator: sender_to_canonical,
        status: PollStatus::InProgress,
        end_height: env.block.height + state.poll_default_end_height,
        start_height: env.block.height,
        description,
        yes_voters: vec![],
        no_voters: vec![],
        amount: proposal_amount,
        prize_rank: proposal_prize_rank,
        proposal: proposal_type,
        migration_address: proposal_human_address,
    };

    // Save poll
    poll_storage(&mut deps.storage).save(&state.poll_count.to_be_bytes(), &new_poll)?;

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
    // Ensure the poll is still valid
    if store.status != PollStatus::InProgress {
        return Err(StdError::generic_err("Proposal is deactivated"));
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

        // Ensure sender have some reward tokens
        let msg = QueryMsg::GetHolder {
            address: human_address,
        };
        let lottera_human = deps
            .api
            .human_address(&state.lottera_staking_contract_address.clone())
            .unwrap();
        let res = encode_msg_query(msg, lottera_human).unwrap();
        let lottera_balance = wrapper_msg_loterra_staking(&deps, res).unwrap();

        if !lottera_balance.bonded.is_zero() {
            weight += lottera_balance.bonded;
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

    //Get total bonded from staking contract
    let msg = QueryMsg::GetAllBonded {};
    let lottera_human = deps
        .api
        .human_address(&state.lottera_staking_contract_address.clone())
        .unwrap();
    let res = encode_msg_query(msg, lottera_human).unwrap();
    let lottera_total_bonded = wrapper_msg_loterra_staking_all_bonded(&deps, res).unwrap();

    // Get the vote weight
    let mut final_vote_weight_in_percentage: u128 = 0;
    if !yes_weight.is_zero() {
        let yes_weight_by_hundred = yes_weight.u128() * 100;
        final_vote_weight_in_percentage = yes_weight_by_hundred / lottera_total_bonded.total_bonded.u128();
    }

    // Reject the proposal
    if final_vote_weight_in_percentage < 60 || store.yes_voters.len() <= store.no_voters.len() {
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
    let mut msgs = vec![];
    // Valid the proposal
    match store.proposal {
        Proposal::LotteryEveryBlockTime => {
            state.every_block_time_play = store.amount.u128() as u64;
        }
        Proposal::DrandWorkerFeePercentage => {
            state.fee_for_drand_worker_in_percentage = store.amount.u128() as u8;
        }
        Proposal::JackpotRewardPercentage => {
            state.jackpot_percentage_reward = store.amount.u128() as u8;
        }
        Proposal::AmountToRegister => {
            state.price_per_ticket_to_register = store.amount;
        }
        Proposal::PrizePerRank => {
            state.prize_rank_winner_percentage = store.prize_rank;
        }
        Proposal::HolderFeePercentage => {
            state.holders_max_percentage_reward = store.amount.u128() as u8
        }
        Proposal::SecurityMigration => {
            let contract_balance = deps
                .querier
                .query_balance(&env.contract.address, &state.denom_stable.clone())?;
            let msg = BankMsg::Send {
                from_address: env.contract.address,
                to_address: store.migration_address.unwrap(),
                amount: vec![Coin {
                    denom: state.denom_stable.to_string(),
                    amount: contract_balance.amount,
                }],
            };
            msgs.push(msg.into())
        }
        Proposal::DaoFunding => {
            let recipient = deps.api.human_address(&store.creator.clone())?;
            let msg_transfer = QueryMsg::Transfer {
                recipient,
                amount: store.amount,
            };
            let lottera_human = deps
                .api
                .human_address(&state.loterra_cw20_contract_address.clone())?;
            let res_transfer = encode_msg_execute(msg_transfer, lottera_human, vec![])?;
            state.dao_funds = state.dao_funds.sub(store.amount)?;
            msgs.push(res_transfer.into())
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
        messages: msgs,
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
        QueryMsg::GetPoll { poll_id } => to_binary(&query_poll(deps, poll_id)?)?,
        QueryMsg::GetRound {} => to_binary(&query_round(deps)?)?,
        QueryMsg::GetRandomness { round: _ } => to_binary(&query_terrand_randomness(deps)?)?,
        QueryMsg::Balance { .. } => to_binary(&query_loterra_balance(deps)?)?,
        QueryMsg::Transfer { .. } => to_binary(&query_loterra_transfer(deps)?)?,
        QueryMsg::PayoutReward {} => to_binary(&query_payout_reward(deps)?)?,
        QueryMsg::GetHolder { .. } => to_binary(&query_loterra_staking_holder(deps)?)?,
        QueryMsg::GetAllBonded {} => to_binary(&query_loterra_staking_total_bonded(deps)?)?
    };
    Ok(response)
}

fn query_config<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<ConfigResponse> {
    let state = config_read(&deps.storage).load()?;
    Ok(state)
}
fn query_terrand_randomness<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}
fn query_loterra_balance<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}
fn query_loterra_transfer<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}
fn query_payout_reward<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}
fn query_loterra_staking_holder<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    return Err(StdError::Unauthorized { backtrace: None });
}
fn query_loterra_staking_total_bonded<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
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
        prize_per_rank: poll.prize_rank,
        migration_address: poll.migration_address,
        yes_voters: poll.yes_voters,
        no_voters: poll.no_voters,
    })
}

fn query_round<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<RoundResponse> {
    let state = config_read(&deps.storage).load()?;
    let from_genesis = state.block_time_play - DRAND_GENESIS_TIME;
    let next_round = (from_genesis / DRAND_PERIOD) + DRAND_NEXT_ROUND_SECURITY;

    Ok(RoundResponse { next_round })
}
/*{
"denom_stable":"uusd",
"block_time_play":1610566920,
"every_block_time_play": 30,
"public_sale_end_block": 2520000,
"poll_default_end_height": 30,
"token_holder_supply": "1000000",
"terrand_contract_address":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k",
"loterra_cw20_contract_address": "terra1jzxdryg2x8vdcwydhzddk68hrl4kve6yk43u8p",
"lottera_staking_contract_address": "terra1jzxdryg2x8vdcwydhzddk68hrl4kve6yk43u8p"
}
{"name":"loterra","symbol":"LOTA","decimals": 6,"initial_balances":[{"address":"terra1np82azjrpfr2ax77s854w4nyh9k63ng7vj26h0","amount":"5000000"}]}

//erc20 loterra
terra18vd8fpwxzck93qlwghaj6arh4p7c5n896xzem5

terra18vd8fpwxzck93qlwghaj6arh4p7c5n896xzem5 '{"transfer":{"recipient": "terra10pyejy66429refv3g35g2t7am0was7ya7kz2a4", "amount": "1000000"}}' --from test1 --chain-id=localterra 1000000uluna  --fees=1000000uluna --gas=auto --broadcast-mode=block
// lottera contract
terra10pyejy66429refv3g35g2t7am0was7ya7kz2a4
{
    "proposal":{
        "description":"my first proposal",
        "proposal":"DrandWorkerFeePercentage",
        "amount":5
    }
}
*/
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_querier::mock_dependencies_custom;
    use crate::msg::{HandleMsg, InitMsg};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, MockApi, MockStorage};
    use cosmwasm_std::StdError::GenericErr;
    use cosmwasm_std::{Api, CosmosMsg, HumanAddr, Storage, Uint128};

    struct BeforeAll {
        default_length: usize,
        default_sender: HumanAddr,
        default_sender_two: HumanAddr,
        default_sender_owner: HumanAddr,
    }
    fn before_all() -> BeforeAll {
        BeforeAll {
            default_length: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k").len(),
            default_sender: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007"),
            default_sender_two: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q008"),
            default_sender_owner: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k"),
        }
    }

    fn default_init<S: Storage, A: Api, Q: Querier>(mut deps: &mut Extern<S, A, Q>) {
        const DENOM_STABLE: &str = "ust";
        const BLOCK_TIME_PLAY: u64 = 1610566920;
        const EVERY_BLOCK_TIME_PLAY: u64 = 50000;
        const PUBLIC_SALE_END_BLOCK: u64 = 1000000000;
        const POLL_DEFAULT_END_HEIGHT: u64 = 40_000;
        const TOKEN_HOLDER_SUPPLY: Uint128 = Uint128(300_000);
        const DAO_FUNDS: Uint128 = Uint128(10_000);

        let init_msg = InitMsg {
            denom_stable: DENOM_STABLE.to_string(),
            block_time_play: BLOCK_TIME_PLAY,
            every_block_time_play: EVERY_BLOCK_TIME_PLAY,
            public_sale_end_block: PUBLIC_SALE_END_BLOCK,
            poll_default_end_height: POLL_DEFAULT_END_HEIGHT,
            token_holder_supply: TOKEN_HOLDER_SUPPLY,
            terrand_contract_address: HumanAddr::from(
                "terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5terrand",
            ),
            loterra_cw20_contract_address: HumanAddr::from(
                "terra1q88h7ewu6h3am4mxxeqhu3srt7zloterracw20",
            ),
            lottera_staking_contract_address: HumanAddr::from(
                "terra1q88h7ewu6h3am4mxxeqhu3srloterrastaking",
            ),
            dao_funds: DAO_FUNDS,
        };

        init(
            &mut deps,
            mock_env("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k", &[]),
            init_msg,
        )
        .unwrap();
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
        println!("{:?}", res.next_round);
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
        let _x = winner_storage(&mut deps.storage)
            .save(
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
            )
            .unwrap();
        let res = query_all_winner(&deps).unwrap();
        println!("{:?}", res);
    }
    mod register {
        use super::*;
        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            //let r = CanonicalAddr(&"DZuks7zPRv9wp2lJTEKdihcInQc=");
            let f = deps
                .api
                .canonical_address(&HumanAddr::from(
                    "terra1umd70qd4jv686wjrsnk92uxgewca3805dxd46p",
                ))
                .unwrap();
            println!("{}", f);
            let mut state = config(&mut deps.storage).load().unwrap();
            state.safe_lock = true;
            config(&mut deps.storage).save(&state).unwrap();
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
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Contract deactivated for update or/and preventing security issue"
                    )
                }
                _ => panic!("Unexpected error"),
            }
        }
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
            let store = combination_storage(&mut deps.storage)
                .load(&"1e3fab".as_bytes())
                .unwrap();
            assert_eq!(1, store.addresses.len());
            let player1 = deps
                .api
                .canonical_address(&before_all.default_sender)
                .unwrap();
            assert!(store.addresses.contains(&player1));
            //New player
            let _res = handle(
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
            let store = combination_storage(&mut deps.storage)
                .load(&"1e3fab".as_bytes())
                .unwrap();
            let player2 = deps
                .api
                .canonical_address(&before_all.default_sender_two)
                .unwrap();
            assert_eq!(2, store.addresses.len());
            assert!(store.addresses.contains(&player1));
            assert!(store.addresses.contains(&player2));
        }
        #[test]
        fn register_fail_if_sender_sent_empty_funds() {
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
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "you need to send 1000000ust in order to register")
                }
                _ => panic!("Unexpected error"),
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
                    &[
                        Coin {
                            denom: "ust".to_string(),
                            amount: Uint128(1_000_000),
                        },
                        Coin {
                            denom: "wrong".to_string(),
                            amount: Uint128(10),
                        },
                    ],
                ),
                msg.clone(),
            );

            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Only send ust to register")
                }
                _ => panic!("Unexpected error"),
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
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "To register you need to send 1000000ust")
                }
                _ => panic!("Unexpected error"),
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
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Not authorized use combination of [a-f] and [0-9] with length 6"
                    )
                }
                _ => panic!("Unexpected error"),
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
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "send 1000000ust")
                }
                _ => panic!("Unexpected error"),
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
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "send 1000000ust")
                }
                _ => panic!("Unexpected error"),
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
            // Block time is superior to block_time_play so the lottery is about to start
            env.block.time = state.block_time_play + 1000;
            let res = handle(&mut deps, env, msg.clone());
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Lottery is about to start wait until the end before register"
                    )
                }
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod public_sale {
        use super::*;
        //handle_public_sale
        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let mut state = config(&mut deps.storage).load().unwrap();
            state.safe_lock = true;
            config(&mut deps.storage).save(&state).unwrap();
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000),
                    }],
                ),
            );
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Contract deactivated for update or/and preventing security issue"
                    )
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_sold_out() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(before_all.default_length, &[]);
            deps.querier.with_token_balances(Uint128(10_000));
            default_init(&mut deps);
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000),
                    }],
                ),
            );
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "All tokens have been sold")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_not_enough() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(before_all.default_length, &[]);
            deps.querier.with_token_balances(Uint128(10_100));
            default_init(&mut deps);
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000),
                    }],
                ),
            );
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "you want to buy 1000 the contract balance only remain 100 token on public sale")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sent_wrong_denom() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "lota".to_string(),
                    amount: Uint128(900),
                }],
            );
            default_init(&mut deps);
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "wrong".to_string(),
                        amount: Uint128(1_000),
                    }],
                ),
            );
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Only ust is accepted")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sent_multiple_denom_right_included() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "lota".to_string(),
                    amount: Uint128(900),
                }],
            );
            default_init(&mut deps);
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[
                        Coin {
                            denom: "wrong".to_string(),
                            amount: Uint128(1_000),
                        },
                        Coin {
                            denom: "ust".to_string(),
                            amount: Uint128(1_000),
                        },
                    ],
                ),
            );

            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Send only ust, no extra denom")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sent_empty() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "lota".to_string(),
                    amount: Uint128(900),
                }],
            );
            default_init(&mut deps);
            let res = handle_public_sale(
                &mut deps,
                mock_env(
                    before_all.default_sender,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(0),
                    }],
                ),
            );
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Send some ust to participate at public sale")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn public_sale_ended() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "lota".to_string(),
                    amount: Uint128(900),
                }],
            );
            default_init(&mut deps);
            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(
                before_all.default_sender,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            env.block.height = state.public_sale_end_block + 1000;
            let res = handle_public_sale(&mut deps, env);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Public sale is ended")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(before_all.default_length, &[]);
            deps.querier.with_token_balances(Uint128(10_900));
            default_init(&mut deps);
            let state_before = config(&mut deps.storage).load().unwrap();
            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(100),
                }],
            );
            let res = handle_public_sale(&mut deps, env.clone()).unwrap();
            let state_after = config(&mut deps.storage).load().unwrap();

            assert_eq!(
                res.messages[0],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps.api.human_address(&state_after.loterra_cw20_contract_address).unwrap(),
                    msg: Binary::from(r#"{"transfer":{"recipient":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007","amount":"100"}}"#.as_bytes()),
                    send: vec![]
                })
            );
            assert!(state_after.token_holder_supply > state_before.token_holder_supply);
        }
    }
    mod play {
        use super::*;

        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let mut state = config(&mut deps.storage).load().unwrap();
            state.safe_lock = true;
            config(&mut deps.storage).save(&state).unwrap();
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_play(&mut deps, env);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Contract deactivated for update or/and preventing security issue"
                    )
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_registration_in_progress() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_play(&mut deps, env.clone());
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Lottery registration is still in progress... Retry after block time 1610566920")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9),
                }],
            );
            env.block.time = state.block_time_play + 1000;
            let res = handle_play(&mut deps, env.clone());
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with play")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn multi_contract_call_terrand() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );

            default_init(&mut deps);
            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(before_all.default_sender.clone(), &[]);
            env.block.time = state.block_time_play + 1000;
            let res = handle_play(&mut deps, env.clone()).unwrap();
            assert_eq!(res.messages.len(), 1);
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );

            default_init(&mut deps);
            // register some combination
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

            let msg = HandleMsg::Register {
                combination: "39493d".to_string(),
            };
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

            let state = config(&mut deps.storage).load().unwrap();
            let mut env = mock_env(before_all.default_sender_owner.clone(), &[]);
            env.block.time = state.block_time_play + 1000;
            let res = handle_play(&mut deps, env.clone()).unwrap();
            println!("{:?}", res);
            assert_eq!(res.messages.len(), 2);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: env.contract.address.clone(),
                    to_address: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker"),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(720)
                    }]
                })
            );

            assert_eq!(
                res.messages[1],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .human_address(&state.lottera_staking_contract_address)
                        .unwrap(),
                    msg: Binary::from(r#"{"payout_reward":{}}"#.as_bytes()),
                    send: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(720000)
                    }]
                })
            );

            let store = winner_storage(&mut deps.storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert_ne!(store.winners.len(), 0);
            assert!(!store.winners[0].claimed);
            assert_eq!(
                store.winners[0].address,
                deps.api
                    .canonical_address(&before_all.default_sender_two)
                    .unwrap()
            );
            let state_after = config(&mut deps.storage).load().unwrap();
            println!("{:?}", state_after.jackpot_reward);
            assert_eq!(state.jackpot_reward, Uint128::zero());
            assert_ne!(state_after.jackpot_reward, state.jackpot_reward);
            assert_eq!(state_after.latest_winning_number, "4f64526c2b6a3650486e4e3834647931326e344f71314272476b74443733465734534b50696878664239493d");
            assert_eq!(state_after.lottery_counter, 1);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }
    }
    mod jackpot {
        use super::*;

        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let mut state = config(&mut deps.storage).load().unwrap();
            state.safe_lock = true;
            config(&mut deps.storage).save(&state).unwrap();
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Contract deactivated for update or/and preventing security issue"
                    )
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "uluna".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let res = handle_jackpot(&mut deps, env.clone());
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with jackpot")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_jackpot_rewards() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone());
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "No jackpot reward")
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn no_winners() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let mut state = config(&mut deps.storage).load().unwrap();
            state.jackpot_reward = Uint128(1_000_000);
            config(&mut deps.storage).save(&state).unwrap();

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone());

            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "No winners")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_empty() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(0),
                }],
            );

            default_init(&mut deps);
            let mut state_before = config(&mut deps.storage).load().unwrap();
            state_before.jackpot_reward = Uint128(1_000_000);
            config(&mut deps.storage).save(&state_before).unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &1_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone());
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Empty contract balance")
                }
                _ => panic!("Unexpected error"),
            }
            let store = winner_storage(&mut deps.storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            let claimed_address = deps
                .api
                .canonical_address(&before_all.default_sender)
                .unwrap();
            assert_eq!(store.winners[1].address, claimed_address);
            //assert!(!store.winners[1].claimed);
            println!("{:?}", store.winners[1].claimed);
        }
        #[test]
        fn some_winner_sender_excluded() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let mut state_before = config(&mut deps.storage).load().unwrap();
            state_before.jackpot_reward = Uint128(1_000_000);
            config(&mut deps.storage).save(&state_before).unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &1_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &5_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();

            let env = mock_env(before_all.default_sender_two.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone());
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "No jackpot to claim, try next time")
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let mut state_before = config(&mut deps.storage).load().unwrap();
            state_before.jackpot_reward = Uint128(1_000_000);
            config(&mut deps.storage).save(&state_before).unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &1_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &5_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone()).unwrap();
            println!("{:?}", res);
            let amount_claimed = Uint128(420000);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: env.contract.address.clone(),
                    to_address: before_all.default_sender.clone(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: amount_claimed.clone()
                    }]
                })
            );
            // Handle can't claim multiple times
            let res = handle_jackpot(&mut deps, env.clone());
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Already claimed")
                }
                _ => panic!("Unexpected error"),
            }

            let store = winner_storage(&mut deps.storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            let claimed_address = deps
                .api
                .canonical_address(&before_all.default_sender)
                .unwrap();
            assert_eq!(store.winners[1].address, claimed_address);
            assert!(store.winners[1].claimed);
            assert!(!store.winners[0].claimed);

            let state_after = config(&mut deps.storage).load().unwrap();
            assert_eq!(state_after.jackpot_reward, state_before.jackpot_reward);
        }
        #[test]
        fn success_multiple_win() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let mut state_before = config(&mut deps.storage).load().unwrap();
            state_before.jackpot_reward = Uint128(1_000_000);
            config(&mut deps.storage).save(&state_before).unwrap();

            winner_storage(&mut deps.storage)
                .save(
                    &1_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();
            winner_storage(&mut deps.storage)
                .save(
                    &2_u8.to_be_bytes(),
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
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                            WinnerInfoState {
                                claimed: false,
                                address: deps
                                    .api
                                    .canonical_address(&before_all.default_sender)
                                    .unwrap(),
                            },
                        ],
                    },
                )
                .unwrap();

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle_jackpot(&mut deps, env.clone()).unwrap();
            let amount_claimed = Uint128(486666);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: env.contract.address.clone(),
                    to_address: before_all.default_sender.clone(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: amount_claimed.clone()
                    }]
                })
            );
            // Handle can't claim multiple times
            let res = handle_jackpot(&mut deps, env.clone());
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Already claimed")
                }
                _ => panic!("Unexpected error"),
            }

            let claimed_address = deps
                .api
                .canonical_address(&before_all.default_sender)
                .unwrap();
            let store = winner_storage(&mut deps.storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert_eq!(store.winners[1].address, claimed_address);
            assert!(store.winners[1].claimed);
            assert!(!store.winners[0].claimed);

            let store = winner_storage(&mut deps.storage)
                .load(&2_u8.to_be_bytes())
                .unwrap();
            assert_eq!(store.winners[1].address, claimed_address);
            assert!(store.winners[1].claimed);
            assert_eq!(store.winners[2].address, claimed_address);
            println!("{:?}", store.winners[2].claimed);
            assert!(store.winners[2].claimed);
            assert!(!store.winners[0].claimed);

            let state_after = config(&mut deps.storage).load().unwrap();
            assert_eq!(state_after.jackpot_reward, state_before.jackpot_reward);
        }
    }

    mod proposal {
        use super::*;
        // handle_proposal
        #[test]
        fn description_min_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Proposal {
                description: "This".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Description min length 6")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn description_max_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Proposal {
                description: "let env = mock_env(before_all.default_sender.clone(), &[]);\
                 let env = mock_env(before_all.default_sender.clone(), &[]); let env \
                 = mock_env(before_all.default_sender.clone(), &[]); let env = mock_env(before_all.default_sender.clone(), &[]);\
                 let env = mock_env(before_all.default_sender.clone(), &[]);let env = mock_env(before_all.default_sender.clone(), &[]);
                 ".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Description max length 255")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }

        fn msg_constructor_none(proposal: Proposal) -> HandleMsg {
            HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: None,
                contract_migration_address: None,
            }
        }
        fn msg_constructor_amount_out(proposal: Proposal) -> HandleMsg {
            HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: Option::from(Uint128(250)),
                prize_per_rank: None,
                contract_migration_address: None,
            }
        }

        fn msg_constructor_prize_len_out(proposal: Proposal) -> HandleMsg {
            HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: Option::from(vec![10, 20, 23, 23, 23, 23]),
                contract_migration_address: None,
            }
        }

        fn msg_constructor_prize_sum_out(proposal: Proposal) -> HandleMsg {
            HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: Option::from(vec![100, 20, 23, 23]),
                contract_migration_address: None,
            }
        }

        #[test]
        fn all_proposal_amount_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);

            let msg_drand_worker_fee_percentage =
                msg_constructor_none(Proposal::DrandWorkerFeePercentage);
            let msg_lottery_every_block_time =
                msg_constructor_none(Proposal::LotteryEveryBlockTime);
            let msg_jackpot_reward_percentage =
                msg_constructor_none(Proposal::JackpotRewardPercentage);
            let msg_prize_per_rank = msg_constructor_none(Proposal::PrizePerRank);
            let msg_holder_fee_per_percentage = msg_constructor_none(Proposal::HolderFeePercentage);
            let msg_amount_to_register = msg_constructor_none(Proposal::AmountToRegister);
            let msg_security_migration = msg_constructor_none(Proposal::SecurityMigration);
            let msg_dao_funding = msg_constructor_none(Proposal::DaoFunding);

            let res = handle(&mut deps, env.clone(), msg_dao_funding);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_security_migration);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Migration address is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_lottery_every_block_time);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount block time required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_drand_worker_fee_percentage);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_jackpot_reward_percentage);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_holder_fee_per_percentage);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Rank is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = handle(&mut deps, env.clone(), msg_amount_to_register);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount is required")
                }
                _ => panic!("Unexpected error"),
            }

            let msg_drand_worker_fee_percentage =
                msg_constructor_amount_out(Proposal::DrandWorkerFeePercentage);
            let msg_jackpot_reward_percentage =
                msg_constructor_amount_out(Proposal::JackpotRewardPercentage);
            let msg_holder_fee_per_percentage =
                msg_constructor_amount_out(Proposal::HolderFeePercentage);

            let res = handle(&mut deps, env.clone(), msg_drand_worker_fee_percentage);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount between 0 to 10")
                }
                _ => panic!("Unexpected error"),
            }
            let res = handle(&mut deps, env.clone(), msg_jackpot_reward_percentage);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount between 0 to 100")
                }
                _ => panic!("Unexpected error"),
            }
            let res = handle(&mut deps, env.clone(), msg_holder_fee_per_percentage);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Amount between 0 to 20")
                }
                _ => panic!("Unexpected error"),
            }

            let msg_prize_per_rank = msg_constructor_prize_len_out(Proposal::PrizePerRank);
            let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(
                        msg,
                        "Ranks need to be in this format [0, 90, 10, 0] numbers between 0 to 100"
                    )
                }
                _ => panic!("Unexpected error"),
            }
            let msg_prize_per_rank = msg_constructor_prize_sum_out(Proposal::PrizePerRank);
            let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Numbers total sum need to be equal to 100")
                }
                _ => panic!("Unexpected error"),
            }
        }
        fn msg_constructor_success(
            proposal: Proposal,
            amount: Option<Uint128>,
            prize_per_rank: Option<Vec<u8>>,
            contract_migration_address: Option<HumanAddr>,
        ) -> HandleMsg {
            HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal,
                amount,
                prize_per_rank,
                contract_migration_address,
            }
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let state = config(&mut deps.storage).load().unwrap();
            assert_eq!(state.poll_count, 0);
            let env = mock_env(before_all.default_sender.clone(), &[]);

            let msg_lottery_every_block_time = msg_constructor_success(
                Proposal::LotteryEveryBlockTime,
                Option::from(Uint128(22)),
                None,
                None,
            );
            let msg_amount_to_register = msg_constructor_success(
                Proposal::AmountToRegister,
                Option::from(Uint128(22)),
                None,
                None,
            );
            let msg_holder_fee_percentage = msg_constructor_success(
                Proposal::HolderFeePercentage,
                Option::from(Uint128(20)),
                None,
                None,
            );
            let msg_prize_rank = msg_constructor_success(
                Proposal::PrizePerRank,
                None,
                Option::from(vec![10, 10, 10, 70]),
                None,
            );
            let msg_jackpot_reward_percentage = msg_constructor_success(
                Proposal::JackpotRewardPercentage,
                Option::from(Uint128(80)),
                None,
                None,
            );
            let msg_drand_fee_worker = msg_constructor_success(
                Proposal::DrandWorkerFeePercentage,
                Option::from(Uint128(10)),
                None,
                None,
            );
            let msg_security_migration = msg_constructor_success(
                Proposal::SecurityMigration,
                None,
                None,
                Option::from(before_all.default_sender_two.clone()),
            );
            let msg_dao_funding = msg_constructor_success(
                Proposal::DaoFunding,
                Option::from(Uint128(80)),
                None,
                None,
            );

            let res = handle(&mut deps, env.clone(), msg_lottery_every_block_time).unwrap();
            assert_eq!(res.log.len(), 4);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(
                poll_state.creator,
                deps.api
                    .canonical_address(&before_all.default_sender)
                    .unwrap()
            );
            let state = config(&mut deps.storage).load().unwrap();
            assert_eq!(state.poll_count, 1);

            let res = handle(&mut deps, env.clone(), msg_amount_to_register).unwrap();
            assert_eq!(res.log.len(), 4);
            let res = handle(&mut deps, env.clone(), msg_holder_fee_percentage).unwrap();
            assert_eq!(res.log.len(), 4);
            let res = handle(&mut deps, env.clone(), msg_prize_rank).unwrap();
            assert_eq!(res.log.len(), 4);
            let res = handle(&mut deps, env.clone(), msg_jackpot_reward_percentage).unwrap();
            assert_eq!(res.log.len(), 4);
            let res = handle(&mut deps, env.clone(), msg_drand_fee_worker).unwrap();
            assert_eq!(res.log.len(), 4);
            // Only owner init proposal for security migration
            let env = mock_env(before_all.default_sender_owner.clone(), &[]);
            let res = handle(&mut deps, env.clone(), msg_security_migration).unwrap();
            assert_eq!(res.log.len(), 4);
            let res = handle(&mut deps, env.clone(), msg_dao_funding).unwrap();
            assert_eq!(res.log.len(), 4);
        }
    }
    mod vote {
        use super::*;
        // handle_vote
        fn create_poll<S: Storage, A: Api, Q: Querier>(mut deps: &mut Extern<S, A, Q>, env: Env) {
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let _res = handle(&mut deps, env, msg).unwrap();
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with vote")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_deactivated() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            // Save to storage
            poll_storage(&mut deps.storage)
                .update::<_>(&1_u64.to_be_bytes(), |poll| {
                    let mut poll_data = poll.unwrap();
                    // Update the status to passed
                    poll_data.status = PollStatus::RejectedByCreator;
                    Ok(poll_data)
                })
                .unwrap();

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Proposal is deactivated")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let mut env = mock_env(before_all.default_sender.clone(), &[]);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Proposal expired")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = handle(&mut deps, env.clone(), msg.clone()).unwrap();
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(res.log.len(), 3);
            assert_eq!(poll_state.no_voters.len(), 1);

            // Try to vote multiple times
            let res = handle(&mut deps, env.clone(), msg);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Already voted")
                }
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod reject {
        use super::*;
        // handle_reject
        fn create_poll<S: Storage, A: Api, Q: Querier>(mut deps: &mut Extern<S, A, Q>, env: Env) {
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let _res = handle(&mut deps, env, msg).unwrap();
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());
            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let msg = HandleMsg::RejectProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with reject proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());
            let mut env = mock_env(before_all.default_sender.clone(), &[]);

            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            let msg = HandleMsg::RejectProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Proposal expired")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn only_creator_can_reject() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());
            let msg = HandleMsg::RejectProposal { poll_id: 1 };
            let env = mock_env(before_all.default_sender_two.clone(), &[]);
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(StdError::Unauthorized { .. }) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());
            let msg = HandleMsg::RejectProposal { poll_id: 1 };

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let res = handle(&mut deps, env.clone(), msg).unwrap();
            assert_eq!(res.messages.len(), 0);
            assert_eq!(res.log.len(), 2);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::RejectedByCreator);
        }
    }
    mod present {
        use super::*;
        // handle_present
        fn create_poll<S: Storage, A: Api, Q: Querier>(mut deps: &mut Extern<S, A, Q>, env: Env) {
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let _res = handle(&mut deps, env, msg).unwrap();
        }
        fn create_poll_security_migration<S: Storage, A: Api, Q: Querier>(
            mut deps: &mut Extern<S, A, Q>,
            env: Env,
        ) {
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::SecurityMigration,
                amount: None,
                prize_per_rank: None,
                contract_migration_address: Option::from(HumanAddr::from("newAddress".to_string())),
            };
            let _res = handle(&mut deps, env, msg).unwrap();
            println!("{:?}", _res);
        }
        fn create_poll_dao_funding<S: Storage, A: Api, Q: Querier>(
            mut deps: &mut Extern<S, A, Q>,
            env: Env,
        ) {
            let msg = HandleMsg::Proposal {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::DaoFunding,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                contract_migration_address: None,
            };
            let _res = handle(&mut deps, env, msg).unwrap();
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let env = mock_env(
                before_all.default_sender.clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Do not send funds with present proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());
            // Save to storage
            poll_storage(&mut deps.storage)
                .update::<_>(&1_u64.to_be_bytes(), |poll| {
                    let mut poll_data = poll.unwrap();
                    // Update the status to passed
                    poll_data.status = PollStatus::Rejected;
                    Ok(poll_data)
                })
                .unwrap();
            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(StdError::Unauthorized { .. }) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_still_in_progress() {
            let before_all = before_all();
            let mut deps = mock_dependencies(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg);
            println!("{:?}", res);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Proposal still in progress")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success_with_reject() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(before_all.default_length,
                                                    &[Coin {
                                                        denom: "ust".to_string(),
                                                        amount: Uint128(9_000_000),
                                                    }],);
            deps.querier.with_token_balances(Uint128(200_000));
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let mut env = mock_env(before_all.default_sender.clone(), &[]);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg).unwrap();
            assert_eq!(res.log.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Rejected);
        }
        #[test]
        fn success_dao_funding() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(before_all.default_sender.clone(),Uint128(150_000), Uint128(10_000), Uint128(0), 0);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll_dao_funding(&mut deps, env.clone());

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = handle(&mut deps, env.clone(), msg);

            let mut env = mock_env(before_all.default_sender.clone(), &[]);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            let mut state_before = config(&mut deps.storage).load().unwrap();

            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg).unwrap();
            println!("{:?}", res);
            assert_eq!(res.log.len(), 3);
            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps.api.human_address(&state_before.loterra_cw20_contract_address).unwrap(),
                    msg: Binary::from(r#"{"transfer":{"recipient":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007","amount":"22"}}"#.as_bytes()),
                    send: vec![]
                })
            );

            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);
            //let state = config(&mut deps);
            let mut state_after = config(&mut deps.storage).load().unwrap();
            println!("{:?}", state_after.dao_funds);
            println!("{:?}", state_before.dao_funds);
            assert_eq!(
                state_before.dao_funds.sub(poll_state.amount).unwrap(),
                state_after.dao_funds
            )
        }
        #[test]
        fn success_with_passed() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(before_all.default_sender.clone(),Uint128(150_000), Uint128(10_000), Uint128(0), 0);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender.clone(), &[]);
            create_poll(&mut deps, env.clone());

            let env = mock_env(before_all.default_sender.clone(), &[]);
            let msg = HandleMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = handle(&mut deps, env.clone(), msg);

            let mut env = mock_env(before_all.default_sender.clone(), &[]);
            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = HandleMsg::PresentProposal { poll_id: 1 };
            let res = handle(&mut deps, env.clone(), msg).unwrap();
            assert_eq!(res.log.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = poll_storage(&mut deps.storage)
                .load(&1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);

            let env = mock_env(before_all.default_sender_owner.clone(), &[]);
            create_poll_security_migration(&mut deps, env.clone());
            let msg = HandleMsg::Vote {
                poll_id: 2,
                approve: true,
            };
            let res = handle(&mut deps, env.clone(), msg).unwrap();
        }
    }
    mod safe_lock {
        use super::*;
        // handle_switch

        #[test]
        fn only_admin() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender_two, &[]);

            let res = handle_safe_lock(&mut deps, env);
            match res {
                Err(StdError::Unauthorized { .. }) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender_owner, &[]);

            // Switch to Off
            let res = handle_safe_lock(&mut deps, env.clone()).unwrap();
            assert_eq!(res.messages.len(), 0);
            let state = config(&mut deps.storage).load().unwrap();
            assert!(state.safe_lock);
            // Switch to On
            let res = handle_safe_lock(&mut deps, env).unwrap();
            println!("{:?}", res);
            let state = config(&mut deps.storage).load().unwrap();
            assert!(!state.safe_lock);
        }
    }

    mod renounce {
        use super::*;
        // handle_renounce
        #[test]
        fn only_admin() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender_two, &[]);

            let res = handle_renounce(&mut deps, env);
            match res {
                Err(StdError::Unauthorized { .. }) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn safe_lock_on() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender_owner, &[]);

            let mut state = config(&mut deps.storage).load().unwrap();
            state.safe_lock = true;
            config(&mut deps.storage).save(&state).unwrap();

            let res = handle_renounce(&mut deps, env);
            match res {
                Err(GenericErr {
                    msg,
                    backtrace: None,
                }) => {
                    assert_eq!(msg, "Contract is locked");
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(before_all.default_length, &[]);
            default_init(&mut deps);
            let env = mock_env(before_all.default_sender_owner.clone(), &[]);

            // Transfer power to admin
            let res = handle_renounce(&mut deps, env.clone()).unwrap();
            assert_eq!(res.messages.len(), 0);
            let state = config(&mut deps.storage).load().unwrap();
            assert_ne!(
                state.admin,
                deps.api
                    .canonical_address(&before_all.default_sender_owner)
                    .unwrap()
            );
            assert_eq!(
                state.admin,
                deps.api.canonical_address(&env.contract.address).unwrap()
            );
        }
    }
}
