use crate::helpers::count_match;
use crate::msg::{
    AllCombinationResponse, AllWinnersResponse, CombinationInfo, ConfigResponse, GetPollResponse,
    HandleMsg, InitMsg, QueryMsg, RoundResponse, WinnerResponse,
};
use crate::query::{GetHolderResponse, LoterraBalanceResponse, TerrandResponse};
use crate::state::{
    all_winners, combination_bucket, combination_bucket_read, config, config_read, poll_storage,
    poll_storage_read, poll_vote_storage, save_winner, winner_count_by_rank_read, winner_storage,
    winner_storage_read, PollInfoState, PollStatus, Proposal, State,
};
use cosmwasm_std::{
    to_binary, Api, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Decimal, Empty, Env, Extern,
    HandleResponse, HumanAddr, InitResponse, LogAttribute, Order, Querier, QueryRequest, StdError,
    StdResult, Storage, Uint128, WasmMsg, WasmQuery,
};
use std::ops::{Add, Mul, Sub};
use terra_cosmwasm::{TaxCapResponse, TerraQuerier};

const DRAND_GENESIS_TIME: u64 = 1595431050;
const DRAND_PERIOD: u64 = 30;
const DRAND_NEXT_ROUND_SECURITY: u64 = 10;
const MAX_DESCRIPTION_LEN: u64 = 255;
const MIN_DESCRIPTION_LEN: u64 = 6;
const HOLDERS_MAX_REWARD: u8 = 20;
const WORKER_MAX_REWARD: u8 = 10;
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
        price_per_ticket_to_register: Uint128(1_000_000),
        terrand_contract_address: deps.api.canonical_address(&msg.terrand_contract_address)?,
        loterra_cw20_contract_address: deps
            .api
            .canonical_address(&msg.loterra_cw20_contract_address)?,
        loterra_staking_contract_address: deps
            .api
            .canonical_address(&msg.loterra_staking_contract_address)?,
        safe_lock: false,
        latest_winning_number: "".to_string(),
        dao_funds: msg.dao_funds,
        lottery_counter: 1,
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

    // Check if the lottery is about to play and cancel new ticket to enter until play
    if env.block.time > state.block_time_play {
        return Err(StdError::generic_err(
            "Lottery is about to start wait until the end before register",
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
            state.denom_stable
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
            state.denom_stable
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "you need to send {}{} in order to register",
            state.price_per_ticket_to_register.clone(),
            state.denom_stable
        )));
    }
    // Handle the player is not sending too much or too less
    if sent.u128() != state.price_per_ticket_to_register.u128() {
        return Err(StdError::generic_err(format!(
            "send {}{}",
            state.price_per_ticket_to_register.clone(),
            state.denom_stable
        )));
    }

    // save combination
    let addr = deps.api.canonical_address(&env.message.sender)?;
    combination_bucket(&mut deps.storage, state.lottery_counter).update(
        combination.as_bytes(),
        |exists| match exists {
            Some(addrs) => {
                let mut modified = addrs;
                modified.push(addr);
                Ok(modified)
            }
            None => Ok(vec![addr]),
        },
    )?;

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
    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with play"));
    }

    // Load the state
    let mut state = config(&mut deps.storage).load()?;

    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
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

    // calculate next round randomness
    let from_genesis = state.block_time_play - DRAND_GENESIS_TIME;
    let next_round = (from_genesis / DRAND_PERIOD) + DRAND_NEXT_ROUND_SECURITY;

    let msg = QueryMsg::GetRandomness { round: next_round };
    let terrand_human = deps.api.human_address(&state.terrand_contract_address)?;
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
    let fee_for_drand_worker = jackpot
        .mul(Decimal::percent(
            state.fee_for_drand_worker_in_percentage as u64,
        ))
        .mul(Decimal::percent(
            state.fee_for_drand_worker_in_percentage as u64,
        ));

    // Amount token holders can claim of the reward is a fee
    let token_holder_fee_reward = jackpot.mul(Decimal::percent(
        state.token_holder_percentage_fee_reward as u64,
    ));
    // Total fees if winner of the jackpot
    let total_fee = fee_for_drand_worker.add(token_holder_fee_reward);

    // The jackpot after worker fee applied
    let mut jackpot_after = jackpot.sub(fee_for_drand_worker).unwrap();
    let mut holders_rewards = Uint128::zero();

    let winners: Vec<(usize, Vec<CanonicalAddr>)> =
        combination_bucket_read(&deps.storage, state.lottery_counter)
            .range(None, None, Order::Ascending)
            .filter_map(|res| {
                let (comb_raw, addrs) = res.ok()?;
                let comb = String::from_utf8(comb_raw).ok()?;
                let match_count = count_match(comb.as_str(), winning_combination);
                if match_count < winning_combination.len() - 3 {
                    return None;
                }

                Some((match_count, addrs))
            })
            .collect();

    // rank winners
    let winners_ranked: Vec<(u8, Vec<CanonicalAddr>)> = winners
        .into_iter()
        .filter_map(|(match_count, addrs)| {
            let rank = match match_count {
                count if count == winning_combination.len() => {
                    // Set the new jackpot after new fee calculation
                    jackpot_after = jackpot.sub(total_fee).unwrap();
                    holders_rewards = holders_rewards.add(token_holder_fee_reward);
                    1
                }
                count if count == winning_combination.len() - 1 => 2,
                count if count == winning_combination.len() - 2 => 3,
                count if count == winning_combination.len() - 3 => 4,
                _ => 0,
            } as u8;

            if rank == 0 {
                return None;
            }
            Some((rank, addrs))
        })
        .collect();

    // save winners by rank
    for (rank, winners) in winners_ranked {
        for addr in winners {
            save_winner(&mut deps.storage, state.lottery_counter, addr, rank)?;
        }
    }

    let querier = TerraQuerier::new(&deps.querier);
    let tax_cap: TaxCapResponse = querier.query_tax_cap(&state.denom_stable)?;
    let amount_to_send = fee_for_drand_worker.sub(tax_cap.cap)?;

    let msg_fee_worker = BankMsg::Send {
        from_address: env.contract.address.clone(),
        to_address: res.worker,
        amount: vec![Coin {
            denom: state.denom_stable.clone(),
            amount: amount_to_send,
        }],
    };

    let mut all_msg = vec![msg_fee_worker.into()];

    if !holders_rewards.is_zero() {
        let amount_to_send = holders_rewards.sub(tax_cap.cap)?;
        let msg_payout = QueryMsg::PayoutReward {};
        let loterra_human = deps
            .api
            .human_address(&state.loterra_staking_contract_address)?;

        let res_payout = encode_msg_execute(
            msg_payout,
            loterra_human,
            vec![Coin {
                denom: state.denom_stable.clone(),
                amount: amount_to_send,
            }],
        )?;

        all_msg.push(res_payout);
    }

    // Update the state
    state.jackpot_reward = jackpot_after;
    state.lottery_counter += 1;

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
            state.denom_stable
        ))),
        1 => {
            if env.message.sent_funds[0].denom == state.denom_stable {
                Ok(env.message.sent_funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "Only {} is accepted",
                    state.denom_stable
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Send only {}, no extra denom",
            state.denom_stable
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "Send some {} to participate at public sale",
            state.denom_stable
        )));
    };
    // Get the contract balance prepare the tx
    let msg_balance = QueryMsg::Balance {
        address: env.contract.address,
    };
    let loterra_human = deps
        .api
        .human_address(&state.loterra_cw20_contract_address)?;
    let res_balance = encode_msg_query(msg_balance, loterra_human)?;
    let loterra_balance = wrapper_msg_loterra(&deps, res_balance)?;

    let adjusted_contract_balance = loterra_balance.balance.sub(state.dao_funds)?;

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
    let loterra_human = deps
        .api
        .human_address(&state.loterra_cw20_contract_address)?;
    let res_transfer = encode_msg_execute(msg_transfer, loterra_human, vec![])?;

    state.token_holder_supply += sent;
    // Save the new state
    config(&mut deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![res_transfer],
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
    // Ensure the sender is not sending funds
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with jackpot"));
    }

    // Load state
    let state = config(&mut deps.storage).load()?;

    if state.safe_lock {
        return Err(StdError::generic_err(
            "Contract deactivated for update or/and preventing security issue",
        ));
    }

    // Ensure there is jackpot reward to claim
    if state.jackpot_reward.is_zero() {
        return Err(StdError::generic_err("No jackpot reward"));
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

    let canonical_addr = deps.api.canonical_address(&env.message.sender)?;
    // Load winner
    let last_lottery_counter_round = state.lottery_counter - 1;
    let may_claim = winner_storage_read(&deps.storage, last_lottery_counter_round)
        .may_load(canonical_addr.as_slice())?;

    if may_claim.is_none() {
        return Err(StdError::generic_err("Address is not a winner"));
    }

    let mut rewards = may_claim.unwrap();

    if rewards.claimed {
        return Err(StdError::generic_err("Already claimed"));
    }

    // Ensure the contract have sufficient balance to handle the transaction
    if balance.amount < state.jackpot_reward {
        return Err(StdError::generic_err("Not enough funds in the contract"));
    }

    let mut total_prize: u128 = 0;
    for rank in rewards.clone().ranks {
        let rank_count = winner_count_by_rank_read(&deps.storage, last_lottery_counter_round)
            .load(&rank.to_be_bytes())?;
        let prize = state
            .jackpot_reward
            .mul(Decimal::percent(
                // TODO claim.rank checked anywhere? this can cause panic
                state.prize_rank_winner_percentage[rank as usize - 1] as u64,
            ))
            .u128()
            / rank_count.u128() as u128;
        total_prize += prize
    }

    // update the winner to claimed true
    rewards.claimed = true;
    winner_storage(&mut deps.storage, last_lottery_counter_round)
        .save(canonical_addr.as_slice(), &rewards)?;

    // Build the amount transaction
    let amount_to_send: Vec<Coin> = vec![Coin {
        denom: state.denom_stable,
        amount: Uint128::from(total_prize),
    }];

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
                value: "handle_jackpot".to_string(),
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
                if percentage.u128() as u8 > HOLDERS_MAX_REWARD {
                    return Err(StdError::generic_err(format!(
                        "Amount between 0 to {}",
                        HOLDERS_MAX_REWARD
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
                if percentage.u128() as u8 > WORKER_MAX_REWARD {
                    return Err(StdError::generic_err(format!(
                        "Amount between 0 to {}",
                        WORKER_MAX_REWARD
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
                let contract_address = deps.api.canonical_address(&env.contract.address)?;
                if state.admin != contract_address && state.admin != sender {
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
                let sender = deps.api.canonical_address(&env.message.sender)?;
                let contract_address = deps.api.canonical_address(&env.contract.address)?;
                if state.admin != contract_address && state.admin != sender {
                    return Err(StdError::Unauthorized { backtrace: None });
                }
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
    } else if let Proposal::StakingContractMigration = proposal {
        match contract_migration_address {
            Some(migration_address) => {
                let sender = deps.api.canonical_address(&env.message.sender)?;
                let contract_address = deps.api.canonical_address(&env.contract.address)?;
                if state.admin != contract_address && state.admin != sender {
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
        Proposal::StakingContractMigration
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
        weight_yes_vote: Uint128::zero(),
        weight_no_vote: Uint128::zero(),
        yes_vote: 0,
        no_vote: 0,
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

fn user_total_weight<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    state: &State,
    address: &CanonicalAddr,
) -> Uint128 {
    let mut weight = Uint128::zero();
    let human_address = deps.api.human_address(&address).unwrap();

    // Ensure sender have some reward tokens
    let msg = QueryMsg::GetHolder {
        address: human_address,
    };
    let loterra_human = deps
        .api
        .human_address(&state.loterra_staking_contract_address.clone())
        .unwrap();
    let res = encode_msg_query(msg, loterra_human).unwrap();
    let loterra_balance = wrapper_msg_loterra_staking(&deps, res).unwrap();

    if !loterra_balance.bonded.is_zero() {
        weight += loterra_balance.bonded;
    }

    weight
}

pub fn handle_vote<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    poll_id: u64,
    approve: bool,
) -> StdResult<HandleResponse> {
    // Ensure the sender not sending funds accidentally
    if !env.message.sent_funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with vote"));
    }

    let sender = deps.api.canonical_address(&env.message.sender)?;
    let state = config(&mut deps.storage).load()?;
    let mut poll_info = poll_storage_read(&deps.storage).load(&poll_id.to_be_bytes())?;

    // Ensure the poll is still valid
    if env.block.height > poll_info.end_height {
        return Err(StdError::generic_err("Proposal expired"));
    }
    // Ensure the poll is still valid
    if poll_info.status != PollStatus::InProgress {
        return Err(StdError::generic_err("Proposal is deactivated"));
    }

    // if user voted fail, else store the vote
    poll_vote_storage(&mut deps.storage, poll_id).update(
        &sender.as_slice(),
        |exists| match exists {
            None => Ok(approve),
            Some(_) => Err(StdError::generic_err("Already voted")),
        },
    )?;

    // Get the sender weight
    let weight = user_total_weight(&deps, &state, &sender);

    // Only stakers can vote
    if weight.is_zero() {
        return Err(StdError::generic_err("Only stakers can vote"));
    }

    // save weight
    let voice = 1;
    if approve {
        poll_info.yes_vote += voice;
        poll_info.weight_yes_vote = poll_info.weight_yes_vote.add(weight);
    } else {
        poll_info.no_vote += voice;
        poll_info.weight_no_vote = poll_info.weight_no_vote.add(weight);
    }
    // overwrite poll info
    poll_storage(&mut deps.storage).save(&poll_id.to_be_bytes(), &poll_info)?;

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
    let store = poll_storage_read(&deps.storage).load(&poll_id.to_be_bytes())?;
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

pub fn handle_present_proposal<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    poll_id: u64,
) -> StdResult<HandleResponse> {
    // Load storage
    let mut state = config(&mut deps.storage).load().unwrap();
    let store = poll_storage_read(&deps.storage)
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

    let total_vote_weight = store.weight_yes_vote.add(store.weight_no_vote);
    let total_yes_weight_percentage = if !store.weight_yes_vote.is_zero() {
        store.weight_yes_vote.u128() * 100 / total_vote_weight.u128()
    } else {
        0
    };
    let total_no_weight_percentage = if !store.weight_no_vote.is_zero() {
        store.weight_no_vote.u128() * 100 / total_vote_weight.u128()
    } else {
        0
    };

    // Reject the proposal
    // Based on the recommendation of security audit
    // We recommend to not reject votes based on the number of votes, but rather by the stake of the voters.
    if total_yes_weight_percentage < 50 || total_no_weight_percentage > 33 {
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
    }

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
            state.token_holder_percentage_fee_reward = store.amount.u128() as u8
        }
        Proposal::SecurityMigration => {
            let contract_balance = deps
                .querier
                .query_balance(&env.contract.address, &state.denom_stable)?;
            let querier = TerraQuerier::new(&deps.querier);
            let tax_cap: TaxCapResponse = querier.query_tax_cap(&state.denom_stable)?;
            let amount_to_send = contract_balance.amount.sub(tax_cap.cap)?;

            let msg = BankMsg::Send {
                from_address: env.contract.address,
                to_address: store.migration_address.unwrap(),
                amount: vec![Coin {
                    denom: state.denom_stable.to_string(),
                    amount: amount_to_send,
                }],
            };
            msgs.push(msg.into())
        }
        Proposal::DaoFunding => {
            let recipient = deps.api.human_address(&store.creator)?;
            let msg_transfer = QueryMsg::Transfer {
                recipient,
                amount: store.amount,
            };
            let loterra_human = deps
                .api
                .human_address(&state.loterra_cw20_contract_address)?;
            let res_transfer = encode_msg_execute(msg_transfer, loterra_human, vec![])?;
            state.dao_funds = state.dao_funds.sub(store.amount)?;
            msgs.push(res_transfer)
        }
        Proposal::StakingContractMigration => {
            state.loterra_staking_contract_address = deps
                .api
                .canonical_address(&store.migration_address.unwrap())?;
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
        QueryMsg::Combination { lottery_id } => {
            to_binary(&query_all_combination(deps, lottery_id)?)?
        }
        QueryMsg::Winner { lottery_id } => to_binary(&query_all_winner(deps, lottery_id)?)?,
        QueryMsg::GetPoll { poll_id } => to_binary(&query_poll(deps, poll_id)?)?,
        QueryMsg::GetRound {} => to_binary(&query_round(deps)?)?,
        QueryMsg::GetRandomness { round: _ } => to_binary(&query_terrand_randomness(deps)?)?,
        QueryMsg::Balance { .. } => to_binary(&query_loterra_balance(deps)?)?,
        QueryMsg::Transfer { .. } => to_binary(&query_loterra_transfer(deps)?)?,
        QueryMsg::PayoutReward {} => to_binary(&query_payout_reward(deps)?)?,
        QueryMsg::GetHolder { .. } => to_binary(&query_loterra_staking_holder(deps)?)?,
        QueryMsg::GetAllBonded {} => to_binary(&query_loterra_staking_total_bonded(deps)?)?,
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
    Err(StdError::Unauthorized { backtrace: None })
}
fn query_loterra_balance<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    Err(StdError::Unauthorized { backtrace: None })
}
fn query_loterra_transfer<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    Err(StdError::Unauthorized { backtrace: None })
}
fn query_payout_reward<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    Err(StdError::Unauthorized { backtrace: None })
}
fn query_loterra_staking_holder<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    Err(StdError::Unauthorized { backtrace: None })
}
fn query_loterra_staking_total_bonded<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
) -> StdResult<StdError> {
    Err(StdError::Unauthorized { backtrace: None })
}

fn query_all_combination<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    lottery_id: u64,
) -> StdResult<AllCombinationResponse> {
    let combinations: StdResult<Vec<CombinationInfo>> =
        combination_bucket_read(&deps.storage, lottery_id)
            .range(None, None, Order::Descending)
            .map(|item| {
                let (comb, cann_addrs) = item?;
                let addrs: StdResult<Vec<HumanAddr>> = cann_addrs
                    .iter()
                    .map(|c| deps.api.human_address(c))
                    .collect();
                // TODO the safety of unwrap
                let key = String::from_utf8(comb).unwrap();
                Ok(CombinationInfo {
                    combination: key,
                    addresses: addrs?,
                })
            })
            .collect();
    Ok(AllCombinationResponse {
        combination: combinations?,
    })
}

fn query_all_winner<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    lottery_id: u64,
) -> StdResult<AllWinnersResponse> {
    let winners = all_winners(&deps.storage, lottery_id)?;
    let res: StdResult<Vec<WinnerResponse>> = winners
        .into_iter()
        .map(|(can_addr, claims)| {
            Ok(WinnerResponse {
                address: deps.api.human_address(&can_addr)?,
                claims,
            })
        })
        .collect();

    Ok(AllWinnersResponse { winners: res? })
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
        weight_yes_vote: poll.weight_yes_vote,
        weight_no_vote: poll.weight_no_vote,
        yes_vote: poll.yes_vote,
        no_vote: poll.no_vote,
        proposal: poll.proposal,
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
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
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
            loterra_staking_contract_address: HumanAddr::from(
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
        save_winner(&mut deps.storage, 1u64, winner_address, 2).unwrap();
        save_winner(&mut deps.storage, 1u64, winner_address2, 2).unwrap();

        let res = query_all_winner(&deps, 1u64).unwrap();
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
                }) => assert_eq!(
                    msg,
                    "Contract deactivated for update or/and preventing security issue"
                ),
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
            let store = combination_bucket(&mut deps.storage, 1u64)
                .load(&"1e3fab".as_bytes())
                .unwrap();
            assert_eq!(1, store.len());
            let player1 = deps
                .api
                .canonical_address(&before_all.default_sender)
                .unwrap();
            assert!(store.contains(&player1));
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
            let store = combination_bucket_read(&mut deps.storage, 1u64)
                .load(&"1e3fab".as_bytes())
                .unwrap();
            let player2 = deps
                .api
                .canonical_address(&before_all.default_sender_two)
                .unwrap();
            assert_eq!(2, store.len());
            assert!(store.contains(&player1));
            assert!(store.contains(&player2));
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
                }) => assert_eq!(msg, "you need to send 1000000ust in order to register"),
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
                }) => assert_eq!(msg, "Only send ust to register"),
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
                }) => assert_eq!(msg, "To register you need to send 1000000ust"),
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
                }) => assert_eq!(
                    msg,
                    "Not authorized use combination of [a-f] and [0-9] with length 6"
                ),
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
                }) => assert_eq!(msg, "send 1000000ust"),
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
                }) => assert_eq!(msg, "send 1000000ust"),
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
                }) => assert_eq!(
                    msg,
                    "Lottery is about to start wait until the end before register"
                ),
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
                }) => assert_eq!(
                    msg,
                    "Contract deactivated for update or/and preventing security issue"
                ),
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
                }) => assert_eq!(msg, "All tokens have been sold"),
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
                }) => assert_eq!(msg, "Only ust is accepted"),
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
                }) => assert_eq!(msg, "Send only ust, no extra denom"),
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
                }) => assert_eq!(msg, "Send some ust to participate at public sale"),
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
                }) => assert_eq!(msg, "Public sale is ended"),
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
        use crate::state::WinnerRewardClaims;
        use cosmwasm_std::log;

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
                }) => assert_eq!(
                    msg,
                    "Contract deactivated for update or/and preventing security issue"
                ),
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
                }) => assert_eq!(msg, "Do not send funds with play"),
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
            let contract_balance = Uint128(9_000_000);
            let mut deps = mock_dependencies_custom(
                before_all.default_length,
                &[Coin {
                    denom: "ust".to_string(),
                    amount: contract_balance.clone(),
                }],
            );

            default_init(&mut deps);
            // register some combination
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            handle(
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
            handle(
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
                        amount: Uint128(719)
                    }]
                })
            );

            assert_eq!(
                res.messages[1],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .human_address(&state.loterra_staking_contract_address)
                        .unwrap(),
                    msg: Binary::from(r#"{"payout_reward":{}}"#.as_bytes()),
                    send: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(719999)
                    }]
                })
            );

            let winners: StdResult<Vec<WinnerRewardClaims>> =
                winner_storage_read(&deps.storage, 1u64)
                    .range(None, None, Order::Ascending)
                    .map(|item| {
                        let (_, v) = item?;
                        Ok(v)
                    })
                    .collect();
            let expected_winners = vec![WinnerRewardClaims {
                claimed: false,
                ranks: vec![1],
            }];
            if winners.unwrap() != expected_winners {
                panic!("winners do not match")
            }

            let state_after = config(&mut deps.storage).load().unwrap();
            println!("{:?}", state_after.jackpot_reward);
            assert_eq!(state.jackpot_reward, Uint128::zero());
            assert_ne!(state_after.jackpot_reward, state.jackpot_reward);
            // 720720 total fees
            assert_eq!(state_after.jackpot_reward, Uint128(6_479_280));
            assert_eq!(state_after.latest_winning_number, "4f64526c2b6a3650486e4e3834647931326e344f71314272476b74443733465734534b50696878664239493d");
            assert_eq!(state_after.lottery_counter, 2);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }

        #[test]
        fn success_no_big_winner() {
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
                combination: "39498d".to_string(),
            };
            handle(
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
            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: env.contract.address.clone(),
                    to_address: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker"),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(719)
                    }]
                })
            );

            let winners: StdResult<Vec<WinnerRewardClaims>> =
                winner_storage_read(&deps.storage, 1u64)
                    .range(None, None, Order::Ascending)
                    .map(|item| {
                        let (_, v) = item?;
                        Ok(v)
                    })
                    .collect();

            let expected_winners = vec![WinnerRewardClaims {
                claimed: false,
                ranks: vec![2],
            }];
            if winners.unwrap() != expected_winners {
                panic!("winners do not match")
            }

            let state_after = config(&mut deps.storage).load().unwrap();
            println!("{:?}", state_after.jackpot_reward);
            assert_eq!(state.jackpot_reward, Uint128::zero());
            assert_ne!(state_after.jackpot_reward, state.jackpot_reward);
            // 720 total fees
            assert_eq!(state_after.jackpot_reward, Uint128(7_199_280));
            assert_eq!(state_after.latest_winning_number, "4f64526c2b6a3650486e4e3834647931326e344f71314272476b74443733465734534b50696878664239493d");
            assert_eq!(state_after.lottery_counter, 2);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }

        #[test]
        fn no_winners() {
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
                combination: "000000".to_string(),
            };
            handle(
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
            let expected = HandleResponse {
                messages: vec![BankMsg::Send {
                    from_address: HumanAddr("cosmos2contract".to_string()),
                    to_address: HumanAddr::from(
                        "terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker".to_string(),
                    ),
                    amount: vec![Coin {
                        denom: "ust".into(),
                        amount: Uint128(719),
                    }],
                }
                .into()],
                log: vec![
                    log("action", "reward"),
                    log("to", "terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k"),
                ],
                data: None,
            };
            if res != expected {
                println!("{:?}", res);
                println!("{:?}", expected);
                panic!()
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
                    }) => assert_eq!(
                        msg,
                        "Contract deactivated for update or/and preventing security issue"
                    ),
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
                    }) => assert_eq!(msg, "Do not send funds with jackpot"),
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
                    }) => assert_eq!(msg, "No jackpot reward"),
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
                    }) => assert_eq!(msg, "Address is not a winner"),
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
                state_before.lottery_counter = 1;
                config(&mut deps.storage).save(&state_before).unwrap();

                let addr1 = deps
                    .api
                    .canonical_address(&HumanAddr("address1".to_string()))
                    .unwrap();
                let addr2 = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();
                println!(
                    "{:?}",
                    deps.api
                        .canonical_address(&HumanAddr("address1".to_string()))
                        .unwrap()
                );

                save_winner(&mut deps.storage, 1u64, addr1.clone(), 1).unwrap();
                println!(
                    "{:?}",
                    winner_storage_read(&deps.storage, 1u64)
                        .load(addr1.as_slice())
                        .unwrap()
                );

                save_winner(&mut deps.storage, 1u64, addr2, 1).unwrap();

                let env = mock_env("address1", &[]);
                let res = handle_jackpot(&mut deps, env.clone());
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Empty contract balance"),
                    _ => panic!("Unexpected error"),
                }
                /*
                let store = winner_storage(&mut deps.storage, 1u64)
                    .load(&1_u8.to_be_bytes())
                    .unwrap();
                let claimed_address = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();
                assert_eq!(store.winners[1].address, claimed_address);
                //assert!(!store.winners[1].claimed);
                println!("{:?}", store.winners[1].claimed);

                 */
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
                state_before.lottery_counter = 2;
                config(&mut deps.storage).save(&state_before).unwrap();

                let addr = deps
                    .api
                    .canonical_address(&HumanAddr("address".to_string()))
                    .unwrap();
                let addr_default = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();

                save_winner(&mut deps.storage, 1u64, addr.clone(), 1).unwrap();
                save_winner(&mut deps.storage, 1u64, addr_default.clone(), 1).unwrap();

                save_winner(&mut deps.storage, 1u64, addr.clone(), 4).unwrap();
                save_winner(&mut deps.storage, 1u64, addr_default.clone(), 4).unwrap();

                let env = mock_env(before_all.default_sender_two.clone(), &[]);
                let res = handle_jackpot(&mut deps, env.clone());
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Address is not a winner"),
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
                state_before.lottery_counter = 2;
                config(&mut deps.storage).save(&state_before).unwrap();

                let addr2 = deps
                    .api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap();
                let default_addr = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();

                save_winner(&mut deps.storage, 1u64, addr2.clone(), 1).unwrap();
                save_winner(&mut deps.storage, 1u64, default_addr.clone(), 1).unwrap();

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
                    }) => assert_eq!(msg, "Already claimed"),
                    _ => panic!("Unexpected error"),
                }

                let claimed_address = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();
                let winner_claim = winner_storage(&mut deps.storage, 1u64)
                    .load(claimed_address.as_slice())
                    .unwrap();

                assert_eq!(winner_claim.claimed, true);

                let not_claimed = winner_storage(&mut deps.storage, 1u64)
                    .load(addr2.as_slice())
                    .unwrap();
                assert_eq!(not_claimed.claimed, false);

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
                state_before.lottery_counter = 2;
                config(&mut deps.storage).save(&state_before).unwrap();

                let addr2 = deps
                    .api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap();
                let default_addr = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();

                // rank 1
                save_winner(&mut deps.storage, 1u64, addr2.clone(), 1).unwrap();
                save_winner(&mut deps.storage, 1u64, default_addr.clone(), 1).unwrap();

                // rank 5
                save_winner(&mut deps.storage, 1u64, addr2.clone(), 2).unwrap();
                save_winner(&mut deps.storage, 1u64, default_addr.clone(), 2).unwrap();
                save_winner(&mut deps.storage, 1u64, default_addr.clone(), 2).unwrap();

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
                    }) => assert_eq!(msg, "Already claimed"),
                    _ => panic!("Unexpected error"),
                }

                let claimed_address = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();

                let claimed = winner_storage(&mut deps.storage, 1u64)
                    .load(claimed_address.as_slice())
                    .unwrap();
                assert_eq!(claimed.claimed, true);

                let not_claimed = winner_storage(&mut deps.storage, 1u64)
                    .load(addr2.as_slice())
                    .unwrap();
                assert_eq!(not_claimed.claimed, false);

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
                    }) => assert_eq!(msg, "Description min length 6"),
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
                    }) => assert_eq!(msg, "Description max length 255"),
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
                    }) => assert_eq!(msg, "Do not send funds with proposal"),
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
                let msg_holder_fee_per_percentage =
                    msg_constructor_none(Proposal::HolderFeePercentage);
                let msg_amount_to_register = msg_constructor_none(Proposal::AmountToRegister);
                let msg_security_migration = msg_constructor_none(Proposal::SecurityMigration);
                let msg_dao_funding = msg_constructor_none(Proposal::DaoFunding);
                let msg_staking_contract_migration =
                    msg_constructor_none(Proposal::StakingContractMigration);

                let res = handle(&mut deps, env.clone(), msg_dao_funding);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_security_migration);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Migration address is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_staking_contract_migration);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Migration address is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_lottery_every_block_time);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount block time required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_drand_worker_fee_percentage);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_jackpot_reward_percentage);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_holder_fee_per_percentage);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Rank is required"),
                    _ => panic!("Unexpected error"),
                }

                let res = handle(&mut deps, env.clone(), msg_amount_to_register);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount is required"),
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
                    }) => assert_eq!(msg, "Amount between 0 to 10"),
                    _ => panic!("Unexpected error"),
                }
                let res = handle(&mut deps, env.clone(), msg_jackpot_reward_percentage);
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount between 0 to 100"),
                    _ => panic!("Unexpected error"),
                }
                let res = handle(&mut deps, env.clone(), msg_holder_fee_per_percentage);
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Amount between 0 to 20"),
                    _ => panic!("Unexpected error"),
                }

                let msg_prize_per_rank = msg_constructor_prize_len_out(Proposal::PrizePerRank);
                let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(
                        msg,
                        "Ranks need to be in this format [0, 90, 10, 0] numbers between 0 to 100"
                    ),
                    _ => panic!("Unexpected error"),
                }
                let msg_prize_per_rank = msg_constructor_prize_sum_out(Proposal::PrizePerRank);
                let res = handle(&mut deps, env.clone(), msg_prize_per_rank);
                println!("{:?}", res);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Numbers total sum need to be equal to 100"),
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

                let msg_staking_contract_migration = msg_constructor_success(
                    Proposal::StakingContractMigration,
                    None,
                    None,
                    Option::from(before_all.default_sender_two.clone()),
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

                // Admin create proposal migration
                let env = mock_env(before_all.default_sender_owner.clone(), &[]);
                let res = handle(&mut deps, env.clone(), msg_security_migration.clone()).unwrap();
                assert_eq!(res.log.len(), 4);
                let res = handle(
                    &mut deps,
                    env.clone(),
                    msg_staking_contract_migration.clone(),
                )
                .unwrap();
                assert_eq!(res.log.len(), 4);
                let res = handle(&mut deps, env.clone(), msg_dao_funding.clone()).unwrap();
                assert_eq!(res.log.len(), 4);

                // Admin renounce so all can create proposal migration
                handle_renounce(&mut deps, env.clone()).unwrap();
                let env = mock_env(before_all.default_sender.clone(), &[]);

                let res = handle(&mut deps, env.clone(), msg_security_migration).unwrap();
                assert_eq!(res.log.len(), 4);
                let res = handle(&mut deps, env.clone(), msg_staking_contract_migration).unwrap();
                assert_eq!(res.log.len(), 4);
                let res = handle(&mut deps, env.clone(), msg_dao_funding).unwrap();
                assert_eq!(res.log.len(), 4);
            }
        }
        mod vote {
            use super::*;
            // handle_vote
            fn create_poll<S: Storage, A: Api, Q: Querier>(
                mut deps: &mut Extern<S, A, Q>,
                env: Env,
            ) {
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
                    }) => assert_eq!(msg, "Do not send funds with vote"),
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
                    }) => assert_eq!(msg, "Proposal is deactivated"),
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
                    }) => assert_eq!(msg, "Proposal expired"),
                    _ => panic!("Unexpected error"),
                }
            }
            #[test]
            fn only_stakers_with_bonded_tokens_can_vote() {
                let before_all = before_all();
                let mut deps = mock_dependencies_custom(
                    before_all.default_length,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(9_000_000),
                    }],
                );
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(0),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );
                default_init(&mut deps);
                let env = mock_env(before_all.default_sender.clone(), &[]);
                create_poll(&mut deps, env.clone());

                let env = mock_env(before_all.default_sender.clone(), &[]);
                let msg = HandleMsg::Vote {
                    poll_id: 1,
                    approve: false,
                };
                let res = handle(&mut deps, env.clone(), msg.clone());
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Only stakers can vote"),
                    _ => panic!("Unexpected error"),
                }
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
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(150_000),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );
                default_init(&mut deps);
                let env = mock_env(before_all.default_sender.clone(), &[]);
                create_poll(&mut deps, env.clone());

                let env = mock_env(before_all.default_sender.clone(), &[]);
                let poll_id: u64 = 1;
                let approve = false;
                let msg = HandleMsg::Vote { poll_id, approve };
                let res = handle(&mut deps, env.clone(), msg.clone()).unwrap();
                let poll_state = poll_storage(&mut deps.storage)
                    .load(&poll_id.to_be_bytes())
                    .unwrap();
                assert_eq!(res.log.len(), 3);
                assert_eq!(poll_state.no_vote, 1);
                assert_eq!(poll_state.yes_vote, 0);
                assert_eq!(poll_state.weight_yes_vote, Uint128::zero());
                assert_eq!(poll_state.weight_no_vote, Uint128(150_000));

                let sender_to_canonical = deps
                    .api
                    .canonical_address(&before_all.default_sender)
                    .unwrap();
                let vote_state = poll_vote_storage(&mut deps.storage, poll_id)
                    .load(sender_to_canonical.as_slice())
                    .unwrap();
                assert_eq!(vote_state, approve);

                // Try to vote multiple times
                let res = handle(&mut deps, env.clone(), msg);
                match res {
                    Err(GenericErr {
                        msg,
                        backtrace: None,
                    }) => assert_eq!(msg, "Already voted"),
                    _ => panic!("Unexpected error"),
                }
            }
        }
        mod reject {
            use super::*;
            // handle_reject
            fn create_poll<S: Storage, A: Api, Q: Querier>(
                mut deps: &mut Extern<S, A, Q>,
                env: Env,
            ) {
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
                    }) => assert_eq!(msg, "Do not send funds with reject proposal"),
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
                    }) => assert_eq!(msg, "Proposal expired"),
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
            fn create_poll<S: Storage, A: Api, Q: Querier>(
                mut deps: &mut Extern<S, A, Q>,
                env: Env,
            ) {
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
                    contract_migration_address: Option::from(HumanAddr::from(
                        "newAddress".to_string(),
                    )),
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
            fn create_poll_statking_contract_migration<S: Storage, A: Api, Q: Querier>(
                mut deps: &mut Extern<S, A, Q>,
                env: Env,
            ) {
                let msg = HandleMsg::Proposal {
                    description: "This is my first proposal".to_string(),
                    proposal: Proposal::StakingContractMigration,
                    amount: None,
                    prize_per_rank: None,
                    contract_migration_address: Option::from(HumanAddr::from(
                        "newAddress".to_string(),
                    )),
                };
                let _res = handle(&mut deps, env, msg).unwrap();
                println!("{:?}", _res);
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
                    }) => assert_eq!(msg, "Do not send funds with present proposal"),
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
                    }) => assert_eq!(msg, "Proposal still in progress"),
                    _ => panic!("Unexpected error"),
                }
            }
            #[test]
            fn success_with_reject() {
                let before_all = before_all();
                let mut deps = mock_dependencies_custom(
                    before_all.default_length,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(9_000_000),
                    }],
                );
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
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(150_000),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );

                default_init(&mut deps);
                // with admin renounce
                let env = mock_env(before_all.default_sender_owner, &[]);
                handle_renounce(&mut deps, env);

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
                let state_before = config(&mut deps.storage).load().unwrap();

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
                let state_after = config(&mut deps.storage).load().unwrap();
                println!("{:?}", state_after.dao_funds);
                println!("{:?}", state_before.dao_funds);
                assert_eq!(
                    state_before.dao_funds.sub(poll_state.amount).unwrap(),
                    state_after.dao_funds
                )
            }
            #[test]
            fn success_staking_migration() {
                let before_all = before_all();
                let mut deps = mock_dependencies_custom(
                    before_all.default_length,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(9_000_000),
                    }],
                );
                deps.querier.with_token_balances(Uint128(200_000));
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(150_000),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );
                default_init(&mut deps);
                let env = mock_env(before_all.default_sender_owner.clone(), &[]);
                create_poll_statking_contract_migration(&mut deps, env.clone());

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
                let state_before = config(&mut deps.storage).load().unwrap();

                let msg = HandleMsg::PresentProposal { poll_id: 1 };
                let res = handle(&mut deps, env.clone(), msg).unwrap();
                println!("{:?}", res);
                assert_eq!(res.log.len(), 3);
                assert_eq!(res.messages.len(), 0);

                let poll_state = poll_storage(&mut deps.storage)
                    .load(&1_u64.to_be_bytes())
                    .unwrap();
                assert_eq!(poll_state.status, PollStatus::Passed);
                //let state = config(&mut deps);
                let state_after = config(&mut deps.storage).load().unwrap();
                assert_ne!(
                    state_after.loterra_staking_contract_address,
                    state_before.loterra_staking_contract_address
                );
                assert_eq!(
                    deps.api
                        .human_address(&state_after.loterra_staking_contract_address)
                        .unwrap(),
                    HumanAddr::from("newAddress".to_string())
                );
            }
            #[test]
            fn success_security_migration() {
                let before_all = before_all();
                let mut deps = mock_dependencies_custom(
                    before_all.default_length,
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(9_000_000),
                    }],
                );
                deps.querier.with_token_balances(Uint128(200_000));
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(150_000),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );
                default_init(&mut deps);
                let env = mock_env(before_all.default_sender_owner.clone(), &[]);
                create_poll_security_migration(&mut deps, env.clone());

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
                config(&mut deps.storage).load().unwrap();

                let msg = HandleMsg::PresentProposal { poll_id: 1 };
                let res = handle(&mut deps, env.clone(), msg);
                println!("{:?}", res);
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
                deps.querier.with_holder(
                    before_all.default_sender.clone(),
                    Uint128(150_000),
                    Uint128(10_000),
                    Uint128(0),
                    0,
                );
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
                println!("{:?}", res);
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
}
