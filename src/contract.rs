use crate::helpers::{count_match, is_lower_hex, reject_proposal, total_weight, user_total_weight};
use crate::msg::{
    AllCombinationResponse, AllWinnersResponse, ConfigResponse, ExecuteMsg, GetPollResponse,
    InstantiateMsg, QueryMsg, RoundResponse, WinnerResponse,
};
use crate::state::{
    all_winners, combination_save, read_state, save_winner, store_state, PollInfoState, PollStatus,
    Proposal, State, ALL_USER_COMBINATION, COUNT_PLAYERS, COUNT_TICKETS, JACKPOT, POLL,
    PREFIXED_POLL_VOTE, PREFIXED_RANK, PREFIXED_USER_COMBINATION, PREFIXED_WINNER, STATE,
    WINNING_COMBINATION,
};
use crate::taxation::deduct_tax;
use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, BankMsg, Binary, CanonicalAddr, Coin, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Timestamp, Uint128, WasmMsg,
    WasmQuery,
};
use cw20::{BalanceResponse, Cw20ExecuteMsg, Cw20QueryMsg};
use std::ops::{Add, Mul};

const DRAND_GENESIS_TIME: u64 = 1595431050;
const DRAND_PERIOD: u64 = 30;
const DRAND_NEXT_ROUND_SECURITY: u64 = 10;
const MAX_DESCRIPTION_LEN: u64 = 255;
const MIN_DESCRIPTION_LEN: u64 = 6;
const HOLDERS_MAX_REWARD: u8 = 20;
const WORKER_MAX_REWARD: u8 = 10;
const DIV_BLOCK_TIME_BY_X: u64 = 2;
// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
// #[serde(rename_all = "snake_case")]
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let state = State {
        admin: deps.api.addr_canonicalize(&info.sender.as_str())?,
        block_time_play: msg.block_time_play,
        every_block_time_play: msg.every_block_time_play,
        denom_stable: msg.denom_stable,
        poll_default_end_height: msg.poll_default_end_height,
        combination_len: 6,
        jackpot_percentage_reward: 20,
        token_holder_percentage_fee_reward: 50,
        fee_for_drand_worker_in_percentage: 1,
        prize_rank_winner_percentage: vec![87, 10, 2, 1],
        poll_count: 0,
        price_per_ticket_to_register: Uint128(1_000_000),
        terrand_contract_address: deps.api.addr_canonicalize(&msg.terrand_contract_address)?,
        loterra_cw20_contract_address: deps
            .api
            .addr_canonicalize(&msg.loterra_cw20_contract_address)?,
        loterra_staking_contract_address: deps
            .api
            .addr_canonicalize(&msg.loterra_staking_contract_address)?,
        safe_lock: false,
        lottery_counter: 1,
        holders_bonus_block_time_end: msg.holders_bonus_block_time_end,
    };
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    match msg {
        ExecuteMsg::Register {
            address,
            combination,
        } => handle_register(deps, env, info, address, combination),
        ExecuteMsg::Play {} => handle_play(deps, env, info),
        ExecuteMsg::Claim { addresses } => handle_claim(deps, env, info, addresses),
        ExecuteMsg::Collect { address } => handle_collect(deps, env, info, address),
        ExecuteMsg::Poll {
            description,
            proposal,
            amount,
            prize_per_rank,
            recipient,
        } => handle_proposal(
            deps,
            env,
            info,
            description,
            proposal,
            amount,
            prize_per_rank,
            recipient,
        ),
        ExecuteMsg::Vote { poll_id, approve } => handle_vote(deps, env, info, poll_id, approve),
        ExecuteMsg::PresentPoll { poll_id } => handle_present_proposal(deps, env, info, poll_id),
        ExecuteMsg::RejectPoll { poll_id } => handle_reject_proposal(deps, env, info, poll_id),
        ExecuteMsg::SafeLock {} => handle_safe_lock(deps, info),
        ExecuteMsg::Renounce {} => handle_renounce(deps, env, info),
    }
}

pub fn handle_renounce(deps: DepsMut, env: Env, info: MessageInfo) -> StdResult<Response> {
    // Load the state
    let mut state = read_state(deps.storage)?;
    let sender = deps.api.addr_canonicalize(info.sender.as_str())?;
    if state.admin != sender {
        return Err(StdError::generic_err("Unauthorized"));
    }
    if state.safe_lock {
        return Err(StdError::generic_err("Locked"));
    }

    state.admin = deps.api.addr_canonicalize(env.contract.address.as_str())?;
    store_state(deps.storage, &state)?;

    Ok(Response::default())
}

pub fn handle_safe_lock(deps: DepsMut, info: MessageInfo) -> StdResult<Response> {
    // Load the state
    let mut state = read_state(deps.storage)?;
    let sender = deps.api.addr_canonicalize(info.sender.as_str())?;
    if state.admin != sender {
        return Err(StdError::generic_err("Unauthorized"));
    }

    state.safe_lock = !state.safe_lock;
    store_state(deps.storage, &state)?;

    Ok(Response::default())
}

pub fn handle_register(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    address: Option<String>,
    combination: Vec<String>,
) -> StdResult<Response> {
    // Load the state
    let state = read_state(deps.storage)?;
    if state.safe_lock {
        return Err(StdError::generic_err("Deactivated"));
    }
    // Check if the lottery is about to play and cancel new ticket to enter until play
    if env.block.time > Timestamp::from_seconds(state.block_time_play) {
        return Err(StdError::generic_err("Lottery about to start"));
    }

    // Check if address filled as param
    let addr = match address {
        None => info.sender.clone(),
        Some(addr) => Addr::unchecked(addr),
    };

    for combo in combination.clone() {
        // Regex to check if the combination is allowed
        if !is_lower_hex(&combo, state.combination_len) {
            return Err(StdError::generic_err(format!(
                "Not authorized use combination of [a-f] and [0-9] with length {}",
                state.combination_len
            )));
        }
    }

    // Check if some funds are sent
    let sent = match info.funds.len() {
        0 => Err(StdError::generic_err(format!(
            "you need to send {}{} per combination in order to register",
            &state.price_per_ticket_to_register, &state.denom_stable
        ))),
        1 => {
            if info.funds[0].denom == state.denom_stable {
                Ok(info.funds[0].amount)
            } else {
                Err(StdError::generic_err(format!(
                    "you need to send {}{} per combination in order to register",
                    &state.price_per_ticket_to_register, &state.denom_stable
                )))
            }
        }
        _ => Err(StdError::generic_err(format!(
            "Only send {0} to register",
            &state.denom_stable
        ))),
    }?;

    if sent.is_zero() {
        return Err(StdError::generic_err(format!(
            "you need to send {}{} per combination in order to register",
            &state.price_per_ticket_to_register, &state.denom_stable
        )));
    }
    // Handle the player is not sending too much or too less
    if sent.u128() != state.price_per_ticket_to_register.u128() * combination.len() as u128 {
        return Err(StdError::generic_err(format!(
            "send {}{}",
            state.price_per_ticket_to_register.u128() * combination.len() as u128,
            state.denom_stable
        )));
    }

    // save combination
    let addr_raw = deps.api.addr_canonicalize(&addr.as_str())?;

    combination_save(
        deps.storage,
        state.lottery_counter,
        addr_raw,
        combination.clone(),
    )?;

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![
            attr("action", "register"),
            attr("price_per_ticket", state.price_per_ticket_to_register),
            attr("amount_ticket_purchased", combination.len()),
            attr("buyer", info.sender),
        ],
    })
}

pub fn handle_play(deps: DepsMut, env: Env, info: MessageInfo) -> StdResult<Response> {
    // Ensure the sender not sending funds accidentally
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with play"));
    }

    // Load the state
    let mut state = read_state(deps.storage)?;

    if state.safe_lock {
        return Err(StdError::generic_err("Deactivated"));
    }

    // calculate next round randomness
    let from_genesis = state
        .block_time_play
        .checked_sub(DRAND_GENESIS_TIME)
        .unwrap();
    let next_round = (from_genesis / DRAND_PERIOD)
        .checked_add(DRAND_NEXT_ROUND_SECURITY)
        .unwrap();

    // Make the contract callable for everyone every x blocks

    if env.block.time > Timestamp::from_seconds(state.block_time_play) {
        // Update the state
        state.block_time_play = env
            .block
            .time
            .plus_seconds(state.every_block_time_play)
            .nanos()
            / 1_000_000_000;
    } else {
        return Err(StdError::generic_err(format!(
            "Lottery registration is still in progress... Retry after block time {}",
            state.block_time_play
        )));
    }

    let msg = terrand::msg::QueryMsg::GetRandomness { round: next_round };
    let terrand_human = deps.api.addr_humanize(&state.terrand_contract_address)?;
    let wasm = WasmQuery::Smart {
        contract_addr: terrand_human.to_string(),
        msg: to_binary(&msg)?,
    };
    let res: terrand::msg::GetRandomResponse = deps.querier.query(&wasm.into())?;
    let randomness_hash = hex::encode(res.randomness.as_slice());

    let n = randomness_hash
        .char_indices()
        .rev()
        .nth(state.combination_len as usize - 1)
        .map(|(i, _)| i)
        .unwrap();
    let winning_combination = &randomness_hash[n..];

    // Save the combination for the current lottery count
    WINNING_COMBINATION.save(
        deps.storage,
        &state.lottery_counter.to_be_bytes(),
        &winning_combination.to_string(),
    )?;

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

    // The jackpot after worker fee applied
    let jackpot_after = jackpot.checked_sub(fee_for_drand_worker)?;

    let msg_fee_worker = BankMsg::Send {
        to_address: res.worker.clone(),
        amount: vec![deduct_tax(
            &deps.querier,
            Coin {
                denom: state.denom_stable.clone(),
                amount: fee_for_drand_worker,
            },
        )?],
    };
    if env.block.time > Timestamp::from_seconds(state.holders_bonus_block_time_end)
        && state.token_holder_percentage_fee_reward > HOLDERS_MAX_REWARD
    {
        state.token_holder_percentage_fee_reward = 20;
    }
    // Save jackpot to storage
    JACKPOT.save(
        deps.storage,
        &state.lottery_counter.to_be_bytes(),
        &jackpot_after,
    )?;
    // Update the state
    state.lottery_counter += 1;

    // Save the new state
    store_state(deps.storage, &state)?;

    Ok(Response {
        submessages: vec![],
        messages: vec![msg_fee_worker.into()],
        data: None,
        attributes: vec![
            attr("action", "reward"),
            attr("by", info.sender.to_string()),
            attr("to", res.worker),
        ],
    })
}

pub fn handle_claim(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    addresses: Option<Vec<String>>,
) -> StdResult<Response> {
    let state = read_state(deps.storage)?;
    if state.safe_lock {
        return Err(StdError::generic_err("Deactivated"));
    }

    if env.block.time
        > Timestamp::from_seconds(
            state
                .block_time_play
                .checked_sub(state.every_block_time_play / DIV_BLOCK_TIME_BY_X)
                .unwrap(),
        )
    {
        return Err(StdError::generic_err("Claiming is closed"));
    }
    let last_lottery_counter_round = state.lottery_counter - 1;

    let lottery_winning_combination = match WINNING_COMBINATION
        .may_load(deps.storage, &last_lottery_counter_round.to_be_bytes())?
    {
        Some(combination) => Some(combination),
        None => {
            return Err(StdError::generic_err("No winning combination"));
        }
    }
    .unwrap();
    let addr = deps.api.addr_canonicalize(&info.sender.as_str())?;

    let mut combination: Vec<(CanonicalAddr, Vec<String>)> = vec![];

    match addresses {
        None => {
            match PREFIXED_USER_COMBINATION.may_load(
                deps.storage,
                (&last_lottery_counter_round.to_be_bytes(), addr.as_slice()),
            )? {
                None => {}
                Some(combo) => combination.push((addr, combo)),
            };
        }
        Some(addresses) => {
            for address in addresses {
                let addr = deps.api.addr_canonicalize(&address.as_str())?;
                match PREFIXED_USER_COMBINATION.may_load(
                    deps.storage,
                    (&last_lottery_counter_round.to_be_bytes(), addr.as_slice()),
                )? {
                    None => {}
                    Some(combo) => combination.push((addr, combo)),
                };
            }
        }
    }

    if combination.is_empty() {
        return Err(StdError::generic_err("No combination found"));
    }
    let mut some_winner = 0;
    for (addr, comb_raw) in combination {
        match PREFIXED_WINNER.may_load(
            deps.storage,
            (&last_lottery_counter_round.to_be_bytes(), addr.as_slice()),
        )? {
            None => {
                for combo in comb_raw {
                    let match_count = count_match(&combo, &lottery_winning_combination);
                    let rank = match match_count {
                        count if count == lottery_winning_combination.len() => 1,
                        count if count == lottery_winning_combination.len() - 1 => 2,
                        count if count == lottery_winning_combination.len() - 2 => 3,
                        count if count == lottery_winning_combination.len() - 3 => 4,
                        _ => 0,
                    } as u8;

                    if rank > 0 {
                        save_winner(deps.storage, last_lottery_counter_round, addr.clone(), rank)?;
                        some_winner += 1;
                    }
                }
            }
            Some(_) => {}
        }
    }

    if some_winner == 0 {
        return Err(StdError::generic_err(
            "No winning combination or already claimed",
        ));
    }

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![attr("action", "claim")],
    })
}
// Players claim the jackpot
pub fn handle_collect(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    address: Option<String>,
) -> StdResult<Response> {
    // Ensure the sender is not sending funds
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with jackpot"));
    }

    // Load state
    let state = read_state(deps.storage)?;
    let last_lottery_counter_round = state.lottery_counter - 1;
    let jackpot_reward = JACKPOT.load(deps.storage, &last_lottery_counter_round.to_be_bytes())?;

    if state.safe_lock {
        return Err(StdError::generic_err("Deactivated"));
    }
    if env.block.time
        < Timestamp::from_seconds(
            state
                .block_time_play
                .checked_sub(state.every_block_time_play / DIV_BLOCK_TIME_BY_X)
                .unwrap(),
        )
    {
        return Err(StdError::generic_err("Collecting jackpot is closed"));
    }
    // Ensure there is jackpot reward to claim
    if jackpot_reward.is_zero() {
        return Err(StdError::generic_err("No jackpot reward"));
    }
    let addr = match address {
        None => info.sender.clone(),
        Some(addr) => Addr::unchecked(addr),
    };

    // Get the contract balance
    let balance = deps
        .querier
        .query_balance(&env.contract.address, &state.denom_stable)?;
    // Ensure the contract have the balance
    if balance.amount.is_zero() {
        return Err(StdError::generic_err("Empty contract balance"));
    }

    let canonical_addr = deps.api.addr_canonicalize(&addr.as_str())?;
    // Load winner
    let mut rewards = match PREFIXED_WINNER.may_load(
        deps.storage,
        (
            &last_lottery_counter_round.to_be_bytes(),
            canonical_addr.as_slice(),
        ),
    )? {
        None => {
            return Err(StdError::generic_err("Address is not a winner"));
        }
        Some(rewards) => Some(rewards),
    }
    .unwrap();

    if rewards.claimed {
        return Err(StdError::generic_err("Already claimed"));
    }

    // Ensure the contract have sufficient balance to handle the transaction
    if balance.amount < jackpot_reward {
        return Err(StdError::generic_err("Not enough funds in the contract"));
    }

    let mut total_prize: u128 = 0;
    for rank in rewards.clone().ranks {
        let rank_count = PREFIXED_RANK.load(
            deps.storage,
            (
                &last_lottery_counter_round.to_be_bytes(),
                &rank.to_be_bytes(),
            ),
        )?;
        let prize = jackpot_reward
            .mul(Decimal::percent(
                state.prize_rank_winner_percentage[rank as usize - 1] as u64,
            ))
            .u128()
            / rank_count.u128() as u128;
        total_prize += prize
    }

    // update the winner to claimed true
    rewards.claimed = true;
    PREFIXED_WINNER.save(
        deps.storage,
        (
            &last_lottery_counter_round.to_be_bytes(),
            canonical_addr.as_slice(),
        ),
        &rewards,
    )?;

    let total_prize = Uint128::from(total_prize);
    // Amount token holders can claim of the reward as fee
    let token_holder_fee_reward = total_prize.mul(Decimal::percent(
        state.token_holder_percentage_fee_reward as u64,
    ));

    let total_prize_after = total_prize.checked_sub(token_holder_fee_reward)?;

    let loterra_human = deps
        .api
        .addr_humanize(&state.loterra_staking_contract_address)?;
    let msg_update_global_index = QueryMsg::UpdateGlobalIndex {};

    let res_update_global_index = WasmMsg::Execute {
        contract_addr: loterra_human.to_string(),
        msg: to_binary(&msg_update_global_index)?,
        send: vec![deduct_tax(
            &deps.querier,
            Coin {
                denom: state.denom_stable.clone(),
                amount: token_holder_fee_reward,
            },
        )?],
    }
    .into();

    // Build the amount transaction
    let msg = BankMsg::Send {
        to_address: addr.to_string(),
        amount: vec![deduct_tax(
            &deps.querier,
            Coin {
                denom: state.denom_stable,
                amount: total_prize_after,
            },
        )?],
    };

    // Send the jackpot
    Ok(Response {
        submessages: vec![],
        messages: vec![msg.into(), res_update_global_index],
        data: None,
        attributes: vec![
            attr("action", "handle_collect"),
            attr("by", info.sender),
            attr("to", addr),
            attr("prize_collect", "true"),
        ],
    })
}
#[allow(clippy::too_many_arguments)]
pub fn handle_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    description: String,
    proposal: Proposal,
    amount: Option<Uint128>,
    prize_per_rank: Option<Vec<u8>>,
    recipient: Option<String>,
) -> StdResult<Response> {
    let mut state = read_state(deps.storage)?;
    // Increment and get the new poll id for bucket key
    let poll_id = state.poll_count + 1;
    // Set the new counter
    state.poll_count = poll_id;

    //Handle sender is not sending funds
    if !info.funds.is_empty() {
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
    let mut proposal_human_address: Option<String> = None;

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
        match recipient {
            Some(migration_address) => {
                let sender = deps.api.addr_canonicalize(info.sender.as_str())?;
                let contract_address =
                    deps.api.addr_canonicalize(&env.contract.address.as_str())?;
                if state.admin != contract_address && state.admin != sender {
                    return Err(StdError::generic_err("Unauthorized"));
                }

                proposal_human_address = Some(migration_address);
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
                let sender = deps.api.addr_canonicalize(&info.sender.as_str())?;
                let contract_address =
                    deps.api.addr_canonicalize(&env.contract.address.as_str())?;
                if state.admin != contract_address && state.admin != sender {
                    return Err(StdError::generic_err("Unauthorized"));
                }
                if amount.is_zero() {
                    return Err(StdError::generic_err("Amount be higher than 0".to_string()));
                }

                // Get the contract balance prepare the tx
                let msg_balance = Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                };
                let loterra_human = deps
                    .api
                    .addr_humanize(&state.loterra_cw20_contract_address)?;

                let res_balance = WasmQuery::Smart {
                    contract_addr: loterra_human.to_string(),
                    msg: to_binary(&msg_balance)?,
                };
                let loterra_balance: BalanceResponse = deps.querier.query(&res_balance.into())?;

                if loterra_balance.balance.is_zero() {
                    return Err(StdError::generic_err(
                        "No more funds to fund project".to_string(),
                    ));
                }
                if loterra_balance.balance.u128() < amount.u128() {
                    return Err(StdError::generic_err(format!(
                        "You need {} we only can fund you up to {}",
                        amount, loterra_balance.balance
                    )));
                }

                proposal_amount = amount;
                proposal_human_address = recipient;
            }
            None => {
                return Err(StdError::generic_err("Amount required".to_string()));
            }
        }
        Proposal::DaoFunding
    } else if let Proposal::StakingContractMigration = proposal {
        match recipient {
            Some(migration_address) => {
                let sender = deps.api.addr_canonicalize(&info.sender.as_str())?;
                let contract_address =
                    deps.api.addr_canonicalize(&env.contract.address.as_str())?;
                if state.admin != contract_address && state.admin != sender {
                    return Err(StdError::generic_err("Unauthorized"));
                }
                proposal_human_address = Some(migration_address);
            }
            None => {
                return Err(StdError::generic_err(
                    "Migration address is required".to_string(),
                ));
            }
        }
        Proposal::StakingContractMigration
    } else if let Proposal::PollSurvey = proposal {
        Proposal::PollSurvey
    } else {
        return Err(StdError::generic_err(
            "Proposal type not founds".to_string(),
        ));
    };

    let sender_to_canonical = deps.api.addr_canonicalize(&info.sender.as_str())?;

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
    POLL.save(deps.storage, &state.poll_count.to_be_bytes(), &new_poll)?;

    // Save state
    store_state(deps.storage, &state)?;

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![
            attr("action", "create poll"),
            attr("poll_id", poll_id),
            attr("poll_creation_result", "success"),
        ],
    })
}

pub fn handle_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    poll_id: u64,
    approve: bool,
) -> StdResult<Response> {
    // Ensure the sender not sending funds accidentally
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("Do not send funds with vote"));
    }

    let sender = deps.api.addr_canonicalize(&info.sender.as_str())?;
    let state = read_state(deps.storage)?;
    let mut poll_info = POLL.load(deps.storage, &poll_id.to_be_bytes())?;

    // Ensure the poll is still valid
    if env.block.height > poll_info.end_height {
        return Err(StdError::generic_err("Proposal expired"));
    }
    // Ensure the poll is still valid
    if poll_info.status != PollStatus::InProgress {
        return Err(StdError::generic_err("Proposal is deactivated"));
    }

    // if user voted fail, else store the vote
    PREFIXED_POLL_VOTE.update(
        deps.storage,
        (&poll_id.to_be_bytes(), &sender.as_slice()),
        |exist| match exist {
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
    POLL.save(deps.storage, &poll_id.to_be_bytes(), &poll_info)?;

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![
            attr("action", "vote"),
            attr("poll_id", poll_id),
            attr("voting_result", "success"),
        ],
    })
}

pub fn handle_reject_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    poll_id: u64,
) -> StdResult<Response> {
    let store = POLL.load(deps.storage, &poll_id.to_be_bytes())?;
    let sender = deps.api.addr_canonicalize(&info.sender.as_str())?;

    // Ensure the sender not sending funds accidentally
    if !info.funds.is_empty() {
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
        return Err(StdError::generic_err("Unauthorized"));
    }

    POLL.update(deps.storage, &poll_id.to_be_bytes(), |poll| match poll {
        None => Err(StdError::generic_err("Poll not found")),
        Some(poll_info) => {
            let mut poll = poll_info;
            poll.status = PollStatus::RejectedByCreator;
            poll.end_height = env.block.height;
            Ok(poll)
        }
    })?;

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![
            attr("action", "creator reject the proposal"),
            attr("poll_id", poll_id),
        ],
    })
}

pub fn handle_present_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    poll_id: u64,
) -> StdResult<Response> {
    // Load storage
    let mut state = read_state(deps.storage)?;
    let poll = POLL.load(deps.storage, &poll_id.to_be_bytes())?;

    // Ensure the sender not sending funds accidentally
    if !info.funds.is_empty() {
        return Err(StdError::generic_err(
            "Do not send funds with present proposal",
        ));
    }
    // Ensure the proposal is still in Progress
    if poll.status != PollStatus::InProgress {
        return Err(StdError::generic_err("Unauthorized"));
    }

    let total_weight_bonded = total_weight(&deps, &state);
    let total_vote_weight = poll.weight_yes_vote.add(poll.weight_no_vote);
    let total_yes_weight_percentage = if !poll.weight_yes_vote.is_zero() {
        poll.weight_yes_vote.u128() * 100 / total_vote_weight.u128()
    } else {
        0
    };
    let total_no_weight_percentage = if !poll.weight_no_vote.is_zero() {
        poll.weight_no_vote.u128() * 100 / total_vote_weight.u128()
    } else {
        0
    };

    if poll.weight_yes_vote.add(poll.weight_no_vote).u128() * 100 / total_weight_bonded.u128() < 50
    {
        // Ensure the proposal is ended
        if poll.end_height > env.block.height {
            return Err(StdError::generic_err("Proposal still in progress"));
        }
    }

    // Reject the proposal
    // Based on the recommendation of security audit
    // We recommend to not reject votes based on the number of votes, but rather by the stake of the voters.
    if total_yes_weight_percentage < 50 || total_no_weight_percentage > 33 {
        return reject_proposal(deps.storage, poll_id);
    }

    let mut msgs = vec![];
    // Valid the proposal
    match poll.proposal {
        Proposal::LotteryEveryBlockTime => {
            state.every_block_time_play = poll.amount.u128() as u64;
        }
        Proposal::DrandWorkerFeePercentage => {
            state.fee_for_drand_worker_in_percentage = poll.amount.u128() as u8;
        }
        Proposal::JackpotRewardPercentage => {
            state.jackpot_percentage_reward = poll.amount.u128() as u8;
        }
        Proposal::AmountToRegister => {
            state.price_per_ticket_to_register = poll.amount;
        }
        Proposal::PrizePerRank => {
            state.prize_rank_winner_percentage = poll.prize_rank;
        }
        Proposal::HolderFeePercentage => {
            state.token_holder_percentage_fee_reward = poll.amount.u128() as u8
        }
        Proposal::SecurityMigration => {
            let contract_balance = deps
                .querier
                .query_balance(&env.contract.address, &state.denom_stable)?;

            let msg = BankMsg::Send {
                to_address: poll.migration_address.unwrap(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: state.denom_stable.to_string(),
                        amount: contract_balance.amount,
                    },
                )?],
            };
            msgs.push(msg.into())
        }
        Proposal::DaoFunding => {
            let recipient = match poll.migration_address {
                None => deps.api.addr_humanize(&poll.creator)?,
                Some(address) => Addr::unchecked(address),
            };

            // Get the contract balance prepare the tx
            let msg_balance = Cw20QueryMsg::Balance {
                address: env.contract.address.to_string(),
            };

            let loterra_human = deps
                .api
                .addr_humanize(&state.loterra_cw20_contract_address)?;

            let res_balance = WasmQuery::Smart {
                contract_addr: loterra_human.to_string(),
                msg: to_binary(&msg_balance)?,
            }
            .into();
            let loterra_balance: BalanceResponse = deps.querier.query(&res_balance)?;

            if loterra_balance.balance.u128() < poll.amount.u128() {
                return reject_proposal(deps.storage, poll_id);
            }
            let msg_transfer = Cw20ExecuteMsg::Transfer {
                recipient: recipient.to_string(),
                amount: poll.amount,
            };

            let loterra_human = deps
                .api
                .addr_humanize(&state.loterra_cw20_contract_address)?;
            let res_transfer = WasmMsg::Execute {
                contract_addr: loterra_human.to_string(),
                msg: to_binary(&msg_transfer)?,
                send: vec![],
            };

            msgs.push(res_transfer.into())
        }
        Proposal::StakingContractMigration => {
            state.loterra_staking_contract_address = deps
                .api
                .addr_canonicalize(&poll.migration_address.unwrap())?;
        }
        Proposal::PollSurvey => {}
        _ => {
            return Err(StdError::generic_err("Proposal not funds"));
        }
    }

    // Save to storage
    POLL.update(deps.storage, &poll_id.to_be_bytes(), |poll| match poll {
        None => Err(StdError::generic_err("Not found")),
        Some(poll_info) => {
            let mut poll = poll_info;
            poll.status = PollStatus::Passed;
            Ok(poll)
        }
    })?;

    store_state(deps.storage, &state)?;

    Ok(Response {
        submessages: vec![],
        messages: msgs,
        data: None,
        attributes: vec![
            attr("action", "present poll"),
            attr("poll_id", poll_id),
            attr("poll_result", "approved"),
        ],
    })
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    let response = match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?)?,
        QueryMsg::Combination {
            lottery_id,
            address,
        } => to_binary(&query_all_combination(deps, lottery_id, address)?)?,
        QueryMsg::Winner { lottery_id } => to_binary(&query_all_winner(deps, lottery_id)?)?,
        QueryMsg::GetPoll { poll_id } => to_binary(&query_poll(deps, poll_id)?)?,
        QueryMsg::GetRound {} => to_binary(&query_round(deps)?)?,
        QueryMsg::CountPlayer { lottery_id } => to_binary(&query_count_player(deps, lottery_id)?)?,
        QueryMsg::CountTicket { lottery_id } => to_binary(&query_count_ticket(deps, lottery_id)?)?,
        QueryMsg::WinningCombination { lottery_id } => {
            to_binary(&query_winning_combination(deps, lottery_id)?)?
        }
        QueryMsg::CountWinner { lottery_id, rank } => {
            to_binary(&query_winner_rank(deps, lottery_id, rank)?)?
        }
        QueryMsg::Jackpot { lottery_id } => to_binary(&query_jackpot(deps, lottery_id)?)?,
        QueryMsg::Players { lottery_id } => to_binary(&query_all_players(deps, lottery_id)?)?,
        _ => to_binary(&())?,
    };
    Ok(response)
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let state = read_state(deps.storage)?;
    Ok(state)
}
fn query_winner_rank(deps: Deps, lottery_id: u64, rank: u8) -> StdResult<Uint128> {
    let amount = match PREFIXED_RANK.may_load(
        deps.storage,
        (&lottery_id.to_be_bytes(), &rank.to_be_bytes()),
    )? {
        None => Uint128::zero(),
        Some(winners) => winners,
    };
    Ok(amount)
}

fn query_all_players(deps: Deps, lottery_id: u64) -> StdResult<Vec<String>> {
    let players = match ALL_USER_COMBINATION.may_load(deps.storage, &lottery_id.to_be_bytes())? {
        None => {
            return Err(StdError::generic_err("Not found"));
        }
        Some(players) => players
            .iter()
            .map(|e| deps.api.addr_humanize(&e).unwrap().to_string())
            .collect(),
    };

    Ok(players)
}
fn query_jackpot(deps: Deps, lottery_id: u64) -> StdResult<Uint128> {
    let amount = match JACKPOT.may_load(deps.storage, &lottery_id.to_be_bytes())? {
        None => Uint128::zero(),
        Some(jackpot) => jackpot,
    };
    Ok(amount)
}
fn query_count_ticket(deps: Deps, lottery_id: u64) -> StdResult<Uint128> {
    let amount = match COUNT_TICKETS.may_load(deps.storage, &lottery_id.to_be_bytes())? {
        None => Uint128::zero(),
        Some(ticket) => ticket,
    };
    Ok(amount)
}
fn query_count_player(deps: Deps, lottery_id: u64) -> StdResult<Uint128> {
    let amount = match COUNT_PLAYERS.may_load(deps.storage, &lottery_id.to_be_bytes())? {
        None => Uint128::zero(),
        Some(players) => players,
    };

    Ok(amount)
}
fn query_winning_combination(deps: Deps, lottery_id: u64) -> StdResult<String> {
    let combination = match WINNING_COMBINATION.may_load(deps.storage, &lottery_id.to_be_bytes())? {
        None => {
            return Err(StdError::generic_err("Not found"));
        }
        Some(combo) => combo,
    };

    Ok(combination)
}
fn query_all_combination(
    deps: Deps,
    lottery_id: u64,
    address: String,
) -> StdResult<AllCombinationResponse> {
    let addr = deps.api.addr_canonicalize(&address)?;
    let combo = match PREFIXED_USER_COMBINATION
        .may_load(deps.storage, (&lottery_id.to_be_bytes(), &addr.as_slice()))?
    {
        None => {
            return Err(StdError::generic_err("Not found"));
        }
        Some(combination) => combination,
    };

    Ok(AllCombinationResponse { combination: combo })
}

fn query_all_winner(deps: Deps, lottery_id: u64) -> StdResult<AllWinnersResponse> {
    let winners = all_winners(&deps, lottery_id)?;
    let res: StdResult<Vec<WinnerResponse>> = winners
        .into_iter()
        .map(|(can_addr, claims)| {
            Ok(WinnerResponse {
                address: deps.api.addr_humanize(&can_addr)?.to_string(),
                claims,
            })
        })
        .collect();

    Ok(AllWinnersResponse { winners: res? })
}

fn query_poll(deps: Deps, poll_id: u64) -> StdResult<GetPollResponse> {
    let poll = match POLL.may_load(deps.storage, &poll_id.to_be_bytes())? {
        Some(poll) => Some(poll),
        None => {
            return Err(StdError::generic_err("Not found"));
        }
    }
    .unwrap();

    Ok(GetPollResponse {
        creator: deps.api.addr_humanize(&poll.creator)?.to_string(),
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

fn query_round(deps: Deps) -> StdResult<RoundResponse> {
    let state = read_state(deps.storage)?;
    let from_genesis = state
        .block_time_play
        .checked_sub(DRAND_GENESIS_TIME)
        .unwrap();
    let next_round = (from_genesis / DRAND_PERIOD) + DRAND_NEXT_ROUND_SECURITY;

    Ok(RoundResponse { next_round })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_querier::mock_dependencies_custom;
    use crate::msg::{ExecuteMsg, InstantiateMsg};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{Api, Uint128, Uint64};

    struct BeforeAll {
        default_sender: String,
        default_sender_two: String,
        default_sender_owner: String,
    }
    fn before_all() -> BeforeAll {
        BeforeAll {
            default_sender: "addr0000".to_string(),
            default_sender_two: "addr0001".to_string(),
            default_sender_owner: "addr0002".to_string(),
        }
    }

    fn default_init(deps: DepsMut) {
        const DENOM_STABLE: &str = "ust";
        const BLOCK_TIME_PLAY: u64 = 1610566920;
        const EVERY_BLOCK_TIME_PLAY: u64 = 50000;
        const POLL_DEFAULT_END_HEIGHT: u64 = 40_000;
        const BONUS_BLOCK_TIME_END: u64 = 1610567920;

        let init_msg = InstantiateMsg {
            denom_stable: DENOM_STABLE.to_string(),
            block_time_play: BLOCK_TIME_PLAY,
            every_block_time_play: EVERY_BLOCK_TIME_PLAY,
            poll_default_end_height: POLL_DEFAULT_END_HEIGHT,
            terrand_contract_address: "terrand".to_string(),
            loterra_cw20_contract_address: "cw20".to_string(),
            loterra_staking_contract_address: "staking".to_string(),
            holders_bonus_block_time_end: BONUS_BLOCK_TIME_END,
        };
        instantiate(deps, mock_env(), mock_info("addr0002", &[]), init_msg).unwrap();
    }

    #[test]
    fn proper_init() {
        let before_all = before_all();
        let mut deps = mock_dependencies(&[]);

        default_init(deps.as_mut());
    }
    #[test]
    fn get_round_play() {
        let before_all = before_all();
        let mut deps = mock_dependencies(&[]);
        default_init(deps.as_mut());
        let res = query_round(deps.as_ref()).unwrap();
        println!("{:?}", res.next_round);
    }

    #[test]
    fn testing_saved_address_winner() {
        let before_all = before_all();
        let mut deps = mock_dependencies(&[Coin {
            denom: "uscrt".to_string(),
            amount: Uint128(100_000_000),
        }]);
        default_init(deps.as_mut());

        let winner_address = deps.api.addr_canonicalize(&"address".to_string()).unwrap();
        let winner_address2 = deps.api.addr_canonicalize(&"address2".to_string()).unwrap();
        save_winner(&mut deps.storage, 1u64, winner_address, 2).unwrap();
        save_winner(&mut deps.storage, 1u64, winner_address2, 2).unwrap();

        let res = query_all_winner(deps.as_ref(), 1u64).unwrap();
        println!("{:?}", res);
    }
    mod claim {
        // handle_claim
        use super::*;
        #[test]
        fn claim_is_closed() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Claim { addresses: None };

            let mut state = read_state(deps.as_ref().storage).unwrap();
            let info = mock_info(before_all.default_sender.as_str(), &[]);
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(state.block_time_play);
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Claiming is closed"),
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn no_winning_combination() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());

            let msg = ExecuteMsg::Claim { addresses: None };
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                (state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / DIV_BLOCK_TIME_BY_X)
                    .unwrap()),
            );
            println!("{:?}", env.block.time);
            println!("{:?}", state.block_time_play);
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(before_all.default_sender.as_str(), &[]),
                msg,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "No winning combination"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_combination() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let mut state = read_state(deps.as_ref().storage).unwrap();
            state.lottery_counter = 2;
            store_state(deps.as_mut().storage, &state).unwrap();
            let last_lottery_counter_round = state.lottery_counter - 1;
            // Save winning combination
            WINNING_COMBINATION
                .save(
                    deps.as_mut().storage,
                    &last_lottery_counter_round.to_be_bytes(),
                    &"123456".to_string(),
                )
                .unwrap();
            let msg = ExecuteMsg::Claim { addresses: None };

            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / DIV_BLOCK_TIME_BY_X)
                    .unwrap(),
            );
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(before_all.default_sender.as_str(), &[]),
                msg,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "No combination found"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let mut state = read_state(deps.as_ref().storage).unwrap();
            // Save combination by senders

            combination_save(
                deps.as_mut().storage,
                state.lottery_counter,
                addr.clone(),
                vec![
                    "123456".to_string(),
                    "12345f".to_string(),
                    "1234a6".to_string(),
                    "000000".to_string(),
                ],
            )
            .unwrap();

            // Save winning combination
            WINNING_COMBINATION
                .save(
                    deps.as_mut().storage,
                    &state.lottery_counter.to_be_bytes(),
                    &"123456".to_string(),
                )
                .unwrap();
            state.lottery_counter = 2;
            store_state(deps.as_mut().storage, &state).unwrap();

            let msg = ExecuteMsg::Claim { addresses: None };
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / DIV_BLOCK_TIME_BY_X)
                    .unwrap(),
            );
            let res = execute(
                deps.as_mut(),
                env.clone(),
                mock_info(before_all.default_sender.as_str().clone(), &[]),
                msg.clone(),
            )
            .unwrap();

            println!("{:?}", res);
            assert_eq!(
                res,
                Response {
                    submessages: vec![],
                    messages: vec![],
                    data: None,
                    attributes: vec![attr("action", "claim")]
                }
            );
            // Claim again is not possible
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(before_all.default_sender.as_str().clone(), &[]),
                msg.clone(),
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "No winning combination or already claimed")
                }
                _ => panic!("Unexpected error"),
            }

            let last_lottery_counter_round = state.lottery_counter - 1;
            let winners = PREFIXED_WINNER
                .load(
                    deps.as_ref().storage,
                    (&last_lottery_counter_round.to_be_bytes(), &addr.as_slice()),
                )
                .unwrap();

            println!("{:?}", winners);
            assert!(!winners.claimed);
            assert_eq!(winners.ranks.len(), 3);
            assert_eq!(winners.ranks[0], 1);
            assert_eq!(winners.ranks[1], 2);
            assert_eq!(winners.ranks[2], 2);
        }
    }

    mod register {
        // handle_register
        use super::*;
        use cosmwasm_std::from_binary;

        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());

            let mut state = read_state(deps.as_ref().storage).unwrap();
            state.safe_lock = true;
            store_state(deps.as_mut().storage, &state).unwrap();
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };
            let res = execute(
                deps.as_mut(),
                mock_env(),
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Deactivated"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec![
                    "1e3fab".to_string(),
                    "abcdef".to_string(),
                    "123456".to_string(),
                ],
            };
            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(3_000_000),
                    }],
                ),
                msg,
            )
            .unwrap();

            assert_eq!(
                res,
                Response {
                    submessages: vec![],
                    messages: vec![],
                    data: None,
                    attributes: vec![
                        attr("action", "register"),
                        attr("price_per_ticket", "1000000"),
                        attr("amount_ticket_purchased", "3"),
                        attr("buyer", "addr0000")
                    ]
                }
            );
            // Check combination added with success
            let addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let store_two = PREFIXED_USER_COMBINATION
                .load(
                    deps.as_mut().storage,
                    (&1u64.to_be_bytes(), &addr.as_slice()),
                )
                .unwrap();
            assert_eq!(3, store_two.len());
            let msg_query = QueryMsg::Players { lottery_id: 1 };
            let res = query(deps.as_ref(), mock_env(), msg_query).unwrap();
            let formated_binary = String::from_utf8(res.into()).unwrap();
            println!("sdsds {:?}", formated_binary);

            // Play 2 more combination
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["affe3b".to_string(), "098765".to_string()],
            };

            let res = execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(2_000_000),
                    }],
                ),
                msg,
            )
            .unwrap();

            // Check combination added with success
            let addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let store_two = PREFIXED_USER_COMBINATION
                .load(
                    deps.as_mut().storage,
                    (&1u64.to_be_bytes(), &addr.as_slice()),
                )
                .unwrap();

            assert_eq!(5, store_two.len());
            assert_eq!(store_two[3], "affe3b".to_string());
            assert_eq!(store_two[4], "098765".to_string());

            // Someone registering combination for other player
            let msg = ExecuteMsg::Register {
                address: Some(before_all.default_sender_two.clone()),
                combination: vec!["aaaaaa".to_string(), "bbbbbb".to_string()],
            };
            // default_sender_two sending combination for default_sender
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(2_000_000),
                    }],
                ),
                msg,
            )
            .unwrap();

            // Check combination added with success
            let addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender_two)
                .unwrap();
            let store_two = PREFIXED_USER_COMBINATION
                .load(
                    deps.as_mut().storage,
                    (&1u64.to_be_bytes(), &addr.as_slice()),
                )
                .unwrap();
            assert_eq!(2, store_two.len());
            assert_eq!(store_two[0], "aaaaaa".to_string());
            assert_eq!(store_two[1], "bbbbbb".to_string());

            let msg = QueryMsg::CountPlayer { lottery_id: 1 };
            let res = query(deps.as_ref(), mock_env(), msg).unwrap();
            let r: Uint128 = from_binary(&res).unwrap();
            //let r = String::from_utf8(res.into()).unwrap();
            assert_eq!(Uint128(2), r);
        }
        #[test]
        fn register_fail_if_sender_sent_empty_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };
            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128::zero(),
                    }],
                ),
                msg,
            );

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(
                    msg,
                    "you need to send 1000000ust per combination in order to register"
                ),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_if_sender_sent_multiple_denom() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };
            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
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
                msg,
            );

            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Only send ust to register")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_if_sender_sent_wrong_denom() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };

            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "wrong".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg,
            );

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(
                    msg,
                    "you need to send 1000000ust per combination in order to register"
                ),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_wrong_combination() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3far".to_string()],
            };
            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg,
            );

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(
                    msg,
                    "Not authorized use combination of [a-f] and [0-9] with length 6"
                ),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_multiple_wrong_combination() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3far".to_string(), "1e3fac".to_string()],
            };
            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(2_000_000),
                    }],
                ),
                msg,
            );

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(
                    msg,
                    "Not authorized use combination of [a-f] and [0-9] with length 6"
                ),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_sent_too_much_or_less() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fae".to_string(), "1e3fa2".to_string()],
            };

            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            env.block.time = Timestamp::from_seconds(state.block_time_play.checked_sub(1).unwrap());
            // Fail sending less than required (1_000_000)
            let res = execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "send 2000000ust"),
                _ => panic!("Unexpected error"),
            }
            // Fail sending more than required (2_000_000)
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(3_000_000),
                    }],
                ),
                msg,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "send 2000000ust"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn register_fail_lottery_about_to_start() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fae".to_string()],
            };
            let state = read_state(deps.as_ref().storage).unwrap();

            let mut env = mock_env();
            let state = read_state(deps.as_ref().storage).unwrap();
            // Block time is superior to block_time_play so the lottery is about to start
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(1000).unwrap());
            let res = execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(3_000_000),
                    }],
                ),
                msg,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Lottery about to start"),
                _ => panic!("Unexpected error"),
            }
        }
    }

    mod play {
        use super::*;
        use cosmwasm_std::{CosmosMsg, Uint64};

        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let mut state = read_state(deps.as_ref().storage).unwrap();
            state.safe_lock = true;
            store_state(deps.as_mut().storage, &state).unwrap();

            let info = mock_info(before_all.default_sender.as_str(), &[]);
            let res = handle_play(deps.as_mut(), mock_env(), info);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Deactivated"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_registration_in_progress() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let mut state = read_state(deps.as_ref().storage).unwrap();
            let env = mock_env();
            state.block_time_play =
                env.block.time.plus_seconds(state.block_time_play).nanos() / 1_000_000_000;
            store_state(deps.as_mut().storage, &state).unwrap();
            let info = mock_info(before_all.default_sender.as_str(), &[]);
            let res = handle_play(deps.as_mut(), env, info);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Lottery registration is still in progress... Retry after block time 3182364339")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            let info = mock_info(
                before_all.default_sender.as_str(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9),
                }],
            );
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(1000).unwrap());
            let res = handle_play(deps.as_mut(), env, info);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with play")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn multi_contract_call_terrand() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);

            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(1000).unwrap());
            let info = mock_info(before_all.default_sender_owner.as_str(), &[]);
            let res = handle_play(deps.as_mut(), env, info).unwrap();
            assert_eq!(res.messages.len(), 1);
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let contract_balance = Uint128(9_000_000);
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: contract_balance.clone(),
            }]);

            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_sub(1000).unwrap());
            // register some combination
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };
            execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();

            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["39493d".to_string()],
            };
            execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender_two.as_str(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();

            let state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();

            let jackpot_reward_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();

            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(1000).unwrap());

            let info = mock_info(before_all.default_sender_owner.as_str(), &[]);
            let res = handle_play(deps.as_mut(), env, info).unwrap();
            println!("{:?}", res);
            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: "terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker".to_string(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(178)
                    }]
                })
            );

            let store = WINNING_COMBINATION
                .load(deps.as_ref().storage, &state.lottery_counter.to_be_bytes())
                .unwrap();
            assert_eq!(store, "39493d");
            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_reward_after = JACKPOT
                .load(deps.as_ref().storage, &state.lottery_counter.to_be_bytes())
                .unwrap();

            // TODO add winner checks

            println!("{:?}", jackpot_reward_after);
            assert_eq!(50, state_after.token_holder_percentage_fee_reward);
            assert_eq!(jackpot_reward_before, Uint128::zero());
            assert_ne!(jackpot_reward_after, jackpot_reward_before);
            // 720720 total fees
            assert_eq!(jackpot_reward_after, Uint128(1_799_820));
            assert_eq!(state_after.lottery_counter, 2);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }

        #[test]
        fn success_no_big_winner() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);

            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_sub(1000).unwrap());
            // register some combination
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["39498d".to_string()],
            };
            execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender_two.as_str().clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();

            let state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();

            let jackpot_reward_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(1000).unwrap());
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            let res = handle_play(deps.as_mut(), env.clone(), info.clone()).unwrap();
            println!("{:?}", res);
            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: "terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker".to_string(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(178)
                    }]
                })
            );

            // TODO add winner check
            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_reward_after = JACKPOT
                .load(deps.as_ref().storage, &state.lottery_counter.to_be_bytes())
                .unwrap();

            println!("{:?}", jackpot_reward_after);
            assert_eq!(50, state_after.token_holder_percentage_fee_reward);
            assert_eq!(jackpot_reward_before, Uint128::zero());
            assert_ne!(jackpot_reward_after, jackpot_reward_before);
            // 720720 total fees
            assert_eq!(jackpot_reward_after, Uint128(1_799_820));
            assert_eq!(state_after.lottery_counter, 2);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }
        #[test]
        fn success_bonus_holder_end_fee_superior_20_percent() {
            let before_all = before_all();
            let contract_balance = Uint128(9_000_000);
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: contract_balance.clone(),
            }]);

            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_sub(1000).unwrap());
            // register some combination
            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["1e3fab".to_string()],
            };
            execute(
                deps.as_mut(),
                env.clone(),
                mock_info(
                    before_all.default_sender.as_str().clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();

            let msg = ExecuteMsg::Register {
                address: None,
                combination: vec!["39493d".to_string()],
            };
            execute(
                deps.as_mut(),
                env,
                mock_info(
                    before_all.default_sender_two.as_str().clone(),
                    &[Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(1_000_000),
                    }],
                ),
                msg.clone(),
            )
            .unwrap();

            let state = read_state(deps.as_ref().storage).unwrap();
            assert_eq!(50, state.token_holder_percentage_fee_reward);
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();
            let jackpot_reward_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();

            let mut env = mock_env();
            env.block.time =
                Timestamp::from_seconds(state.block_time_play.checked_add(10_000).unwrap());
            let info = mock_info(before_all.default_sender_owner.as_str(), &[]);
            let res = handle_play(deps.as_mut(), env.clone(), info.clone()).unwrap();
            println!("{:?}", res);

            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: "terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker".to_string(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(178)
                    }]
                })
            );

            let store = WINNING_COMBINATION
                .load(deps.as_ref().storage, &state.lottery_counter.to_be_bytes())
                .unwrap();
            assert_eq!(store, "39493d");

            // TODO add winner checks
            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_reward_after = JACKPOT
                .load(deps.as_ref().storage, &state.lottery_counter.to_be_bytes())
                .unwrap();

            println!("{:?}", jackpot_reward_after);
            assert_eq!(20, state_after.token_holder_percentage_fee_reward);
            assert_eq!(jackpot_reward_before, Uint128::zero());
            assert_ne!(jackpot_reward_after, jackpot_reward_before);
            // 720720 total fees
            assert_eq!(jackpot_reward_after, Uint128(1_799_820));
            assert_eq!(state_after.lottery_counter, 2);
            assert_ne!(state_after.lottery_counter, state.lottery_counter);
        }
    }

    mod collect {
        use super::*;
        use cosmwasm_std::CosmosMsg;

        #[test]
        fn security_active() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let mut state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();
            state.safe_lock = true;
            store_state(deps.as_mut().storage, &state).unwrap();

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Deactivated"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let info = mock_info(
                before_all.default_sender.as_str().clone(),
                &[Coin {
                    denom: "uluna".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), mock_env(), info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with jackpot")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn collect_jackpot_is_closed() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play)
                    .unwrap(),
            );

            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Collecting jackpot is closed")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_jackpot_rewards() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128::zero(),
                )
                .unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / 2)
                    .unwrap(),
            );
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "No jackpot reward"),
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn no_winners() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let mut state = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / 2)
                    .unwrap(),
            );
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Address is not a winner"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_empty() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(0),
            }]);

            default_init(deps.as_mut());
            let mut state_before = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();

            let addr1 = deps.api.addr_canonicalize(&"address1".to_string()).unwrap();
            let addr2 = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            println!(
                "{:?}",
                deps.api.addr_canonicalize(&"address1".to_string()).unwrap()
            );

            save_winner(deps.as_mut().storage, 1u64, addr1.clone(), 1).unwrap();

            save_winner(deps.as_mut().storage, 1u64, addr2, 1).unwrap();
            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state
                    .block_time_play
                    .checked_sub(state.every_block_time_play / 2)
                    .unwrap(),
            );
            let info = mock_info("address1", &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);

            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Empty contract balance"),
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
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let mut state_before = read_state(deps.as_ref().storage).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();

            let addr = deps.api.addr_canonicalize(&"address".to_string()).unwrap();
            let addr_default = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();

            save_winner(deps.as_mut().storage, 1u64, addr.clone(), 1).unwrap();
            save_winner(deps.as_mut().storage, 1u64, addr_default.clone(), 1).unwrap();

            save_winner(deps.as_mut().storage, 1u64, addr.clone(), 4).unwrap();
            save_winner(deps.as_mut().storage, 1u64, addr_default.clone(), 4).unwrap();

            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state_before
                    .block_time_play
                    .checked_sub(state_before.every_block_time_play / 2)
                    .unwrap(),
            );
            let msg = ExecuteMsg::Collect { address: None };
            let info = mock_info(before_all.default_sender_two.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env, info, msg);

            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Address is not a winner"),
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());

            let mut state_before = read_state(deps.as_ref().storage).unwrap();
            state_before.lottery_counter = 2;
            store_state(deps.as_mut().storage, &state_before).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();

            let addr2 = deps.api.addr_canonicalize(&"address2".to_string()).unwrap();
            let default_addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();

            save_winner(deps.as_mut().storage, 1u64, addr2.clone(), 1).unwrap();
            save_winner(deps.as_mut().storage, 1u64, default_addr.clone(), 1).unwrap();

            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state_before
                    .block_time_play
                    .checked_sub(state_before.every_block_time_play / 2)
                    .unwrap(),
            );

            let msg = ExecuteMsg::Collect { address: None };
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
            println!("{:?}", res);
            assert_eq!(res.messages.len(), 2);
            let amount_claimed = Uint128(215346);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: before_all.default_sender.clone(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: amount_claimed.clone()
                    }]
                })
            );

            assert_eq!(
                res.messages[1],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .addr_humanize(&state_before.loterra_staking_contract_address)
                        .unwrap()
                        .to_string(),
                    msg: Binary::from(r#"{"update_global_index":{}}"#.as_bytes()),
                    send: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(215346)
                    }]
                })
            );
            // Handle can't claim multiple times
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env, info, msg);

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Already claimed"),
                _ => panic!("Unexpected error"),
            }

            let claimed_address = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let winner_claim = PREFIXED_WINNER
                .load(
                    deps.as_mut().storage,
                    (&1u64.to_be_bytes(), claimed_address.as_slice()),
                )
                .unwrap();
            assert_eq!(winner_claim.claimed, true);

            let not_claimed = PREFIXED_WINNER
                .load(
                    deps.as_mut().storage,
                    (&1u64.to_be_bytes(), addr2.as_slice()),
                )
                .unwrap();
            assert_eq!(not_claimed.claimed, false);

            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();

            let jackpot_after = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_after.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            assert_eq!(state_after, state_before);
        }
        #[test]
        fn success_collecting_for_someone() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());

            let mut state_before = read_state(deps.as_ref().storage).unwrap();
            state_before.lottery_counter = 2;
            store_state(deps.as_mut().storage, &&state_before).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();

            let addr2 = deps.api.addr_canonicalize(&"address2".to_string()).unwrap();
            let default_addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();

            save_winner(&mut deps.storage, 1u64, addr2.clone(), 1).unwrap();
            save_winner(&mut deps.storage, 1u64, default_addr.clone(), 1).unwrap();

            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state_before
                    .block_time_play
                    .checked_sub(state_before.every_block_time_play / 2)
                    .unwrap(),
            );
            let info = mock_info(before_all.default_sender_two.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect {
                address: Some(before_all.default_sender.clone()),
            };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
            println!("{:?}", res);

            assert_eq!(res.messages.len(), 2);
            let amount_claimed = Uint128(215346);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: before_all.default_sender.clone(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: amount_claimed.clone()
                    }]
                })
            );
            assert_eq!(
                res.messages[1],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .addr_humanize(&state_before.loterra_staking_contract_address)
                        .unwrap()
                        .to_string(),
                    msg: Binary::from(r#"{"update_global_index":{}}"#.as_bytes()),
                    send: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(215346)
                    }]
                })
            );
            // Handle can't claim multiple times
            let msg = ExecuteMsg::Collect {
                address: Some(before_all.default_sender.clone()),
            };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Already claimed"),
                _ => panic!("Unexpected error"),
            }

            let claimed_address = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let winner_claim = PREFIXED_WINNER
                .load(
                    deps.as_ref().storage,
                    (&1u64.to_be_bytes(), claimed_address.as_slice()),
                )
                .unwrap();
            assert_eq!(winner_claim.claimed, true);

            let not_claimed = PREFIXED_WINNER
                .load(
                    deps.as_ref().storage,
                    (&1u64.to_be_bytes(), addr2.as_slice()),
                )
                .unwrap();
            assert_eq!(not_claimed.claimed, false);

            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            let jackpot_after = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_after.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            assert_eq!(state_after, state_before);
        }
        #[test]
        fn success_multiple_win() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());

            let mut state_before = read_state(deps.as_ref().storage).unwrap();
            state_before.lottery_counter = 2;
            store_state(deps.as_mut().storage, &state_before).unwrap();
            JACKPOT
                .save(
                    deps.as_mut().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                    &Uint128(1_000_000),
                )
                .unwrap();

            let addr2 = deps.api.addr_canonicalize(&"address2".to_string()).unwrap();
            let default_addr = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();

            // rank 1
            save_winner(&mut deps.storage, 1u64, addr2.clone(), 1).unwrap();
            save_winner(&mut deps.storage, 1u64, default_addr.clone(), 1).unwrap();

            // rank 5
            save_winner(&mut deps.storage, 1u64, addr2.clone(), 2).unwrap();
            save_winner(&mut deps.storage, 1u64, default_addr.clone(), 2).unwrap();
            save_winner(&mut deps.storage, 1u64, default_addr.clone(), 2).unwrap();

            let state = read_state(deps.as_ref().storage).unwrap();
            let mut env = mock_env();
            env.block.time = Timestamp::from_seconds(
                state_before
                    .block_time_play
                    .checked_sub(state_before.every_block_time_play / 2)
                    .unwrap(),
            );
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
            assert_eq!(res.messages.len(), 2);
            let amount_claimed = Uint128(248349);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    to_address: before_all.default_sender.clone(),
                    amount: vec![Coin {
                        denom: "ust".to_string(),
                        amount: amount_claimed.clone()
                    }]
                })
            );
            assert_eq!(
                res.messages[1],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .addr_humanize(&state_before.loterra_staking_contract_address)
                        .unwrap()
                        .to_string(),
                    msg: Binary::from(r#"{"update_global_index":{}}"#.as_bytes()),
                    send: vec![Coin {
                        denom: "ust".to_string(),
                        amount: Uint128(248349)
                    }]
                })
            );
            // Handle can't claim multiple times
            let msg = ExecuteMsg::Collect { address: None };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);

            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Already claimed"),
                _ => panic!("Unexpected error"),
            }

            let claimed_address = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();

            let claimed = PREFIXED_WINNER
                .load(
                    deps.as_ref().storage,
                    (&1u64.to_be_bytes(), claimed_address.as_slice()),
                )
                .unwrap();
            assert_eq!(claimed.claimed, true);

            let not_claimed = PREFIXED_WINNER
                .load(
                    deps.as_ref().storage,
                    (&1u64.to_be_bytes(), addr2.as_slice()),
                )
                .unwrap();
            assert_eq!(not_claimed.claimed, false);

            let state_after = read_state(deps.as_ref().storage).unwrap();
            let jackpot_before = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_before.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            let jackpot_after = JACKPOT
                .load(
                    deps.as_ref().storage,
                    &(state_after.lottery_counter - 1).to_be_bytes(),
                )
                .unwrap();
            assert_eq!(state_after, state_before);
        }
    }

    mod proposal {
        use super::*;
        // handle_proposal
        #[test]
        fn description_min_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let env = mock_env();
            let msg = ExecuteMsg::Poll {
                description: "This".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Description min length 6")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn description_max_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let env = mock_env();
            let msg = ExecuteMsg::Poll {
                description: "let env = mock_env(before_all.default_sender.clone(), &[]);\
                 let env = mock_env(before_all.default_sender.clone(), &[]); let env \
                 = mock_env(before_all.default_sender.clone(), &[]); let env = mock_env(before_all.default_sender.clone(), &[]);\
                 let env = mock_env(before_all.default_sender.clone(), &[]);let env = mock_env(before_all.default_sender.clone(), &[]);
                 ".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                recipient: None
            };
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Description max length 255")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let env = mock_env();
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let info = mock_info(
                before_all.default_sender.as_str().clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }

        fn msg_constructor_none(proposal: Proposal) -> ExecuteMsg {
            ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: None,
                recipient: None,
            }
        }
        fn msg_constructor_amount_out(proposal: Proposal) -> ExecuteMsg {
            ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: Option::from(Uint128(250)),
                prize_per_rank: None,
                recipient: None,
            }
        }

        fn msg_constructor_prize_len_out(proposal: Proposal) -> ExecuteMsg {
            ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: Option::from(vec![10, 20, 23, 23, 23, 23]),
                recipient: None,
            }
        }

        fn msg_constructor_prize_sum_out(proposal: Proposal) -> ExecuteMsg {
            ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal,
                amount: None,
                prize_per_rank: Option::from(vec![100, 20, 23, 23]),
                recipient: None,
            }
        }

        #[test]
        fn all_proposal_amount_error() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            let env = mock_env();

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
            let msg_staking_contract_migration =
                msg_constructor_none(Proposal::StakingContractMigration);

            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg_dao_funding);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount required"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_security_migration,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Migration address is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_staking_contract_migration,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Migration address is required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_lottery_every_block_time,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Amount block time required")
                }
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_drand_worker_fee_percentage,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount is required"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_jackpot_reward_percentage,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount is required"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_holder_fee_per_percentage,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount is required"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg_prize_per_rank);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Rank is required"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_amount_to_register,
            );
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount is required"),
                _ => panic!("Unexpected error"),
            }

            let msg_drand_worker_fee_percentage =
                msg_constructor_amount_out(Proposal::DrandWorkerFeePercentage);
            let msg_jackpot_reward_percentage =
                msg_constructor_amount_out(Proposal::JackpotRewardPercentage);
            let msg_holder_fee_per_percentage =
                msg_constructor_amount_out(Proposal::HolderFeePercentage);

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_drand_worker_fee_percentage,
            );
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount between 0 to 10"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_jackpot_reward_percentage,
            );
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount between 0 to 100"),
                _ => panic!("Unexpected error"),
            }

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_holder_fee_per_percentage,
            );
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Amount between 0 to 20"),
                _ => panic!("Unexpected error"),
            }

            let msg_prize_per_rank = msg_constructor_prize_len_out(Proposal::PrizePerRank);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_prize_per_rank.clone(),
            );
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(
                    msg,
                    "Ranks need to be in this format [0, 90, 10, 0] numbers between 0 to 100"
                ),
                _ => panic!("Unexpected error"),
            }
            let msg_prize_per_rank = msg_constructor_prize_sum_out(Proposal::PrizePerRank);
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg_prize_per_rank);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Numbers total sum need to be equal to 100")
                }
                _ => panic!("Unexpected error"),
            }
        }
        fn msg_constructor_success(
            proposal: Proposal,
            amount: Option<Uint128>,
            prize_per_rank: Option<Vec<u8>>,
            recipient: Option<String>,
        ) -> ExecuteMsg {
            ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal,
                amount,
                prize_per_rank,
                recipient,
            }
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            default_init(deps.as_mut());
            let state = read_state(deps.as_ref().storage).unwrap();
            assert_eq!(state.poll_count, 0);
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
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
                Option::from(Uint128(200_000)),
                None,
                None,
            );

            let msg_staking_contract_migration = msg_constructor_success(
                Proposal::StakingContractMigration,
                None,
                None,
                Option::from(before_all.default_sender_two.clone()),
            );

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_lottery_every_block_time,
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(
                poll_state.creator,
                deps.api
                    .addr_canonicalize(&before_all.default_sender)
                    .unwrap()
            );
            let state = read_state(deps.as_ref().storage).unwrap();
            assert_eq!(state.poll_count, 1);

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_amount_to_register,
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_holder_fee_percentage,
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg_prize_rank).unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_jackpot_reward_percentage,
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_drand_fee_worker,
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);

            // Admin create proposal migration
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_security_migration.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_staking_contract_migration.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_dao_funding.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);

            // Admin renounce so all can create proposal migration
            handle_renounce(deps.as_mut(), env.clone(), info).unwrap();
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_security_migration.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_staking_contract_migration.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                msg_dao_funding.clone(),
            )
            .unwrap();
            assert_eq!(res.attributes.len(), 3);
        }
    }

    mod vote {
        use super::*;
        // handle_vote
        fn create_poll(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let _res = execute(deps, mock_env(), mock_info("addr0000", &[]), msg).unwrap();
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(
                before_all.default_sender.as_str().clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with vote")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_deactivated() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            // Save to storage
            POLL.update(
                deps.as_mut().storage,
                &1u64.to_be_bytes(),
                |poll| -> StdResult<PollInfoState> {
                    match poll {
                        None => Err(StdError::generic_err("error")),
                        Some(poll_state) => {
                            let mut poll_data = poll_state;
                            // Update the status to passed
                            poll_data.status = PollStatus::RejectedByCreator;
                            Ok(poll_data)
                        }
                    }
                },
            )
            .unwrap();

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Proposal is deactivated"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Proposal expired"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn only_stakers_with_bonded_tokens_can_vote() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(0),
                Decimal::zero(),
                Decimal::zero(),
            );

            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: false,
            };
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Only stakers can vote"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(150_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str(), &[]);
            let poll_id: u64 = 1;
            let approve = false;
            let msg = ExecuteMsg::Vote { poll_id, approve };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            let poll_state = POLL
                .load(deps.as_ref().storage, &poll_id.to_be_bytes())
                .unwrap();
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(poll_state.no_vote, 1);
            assert_eq!(poll_state.yes_vote, 0);
            assert_eq!(poll_state.weight_yes_vote, Uint128::zero());
            assert_eq!(poll_state.weight_no_vote, Uint128(150_000));

            let sender_to_canonical = deps
                .api
                .addr_canonicalize(&before_all.default_sender)
                .unwrap();
            let vote_state = PREFIXED_POLL_VOTE
                .load(
                    deps.as_ref().storage,
                    (
                        &poll_id.to_be_bytes().clone(),
                        sender_to_canonical.as_slice(),
                    ),
                )
                .unwrap();
            assert_eq!(vote_state, approve);

            // Try to vote multiple times
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Already voted"),
                _ => panic!("Unexpected error"),
            }
        }
    }

    mod reject {
        use super::*;
        // handle_reject
        fn create_poll(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let _res = execute(deps, mock_env(), mock_info("addr0000", &[]), msg).unwrap();
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());
            let env = mock_env();
            let info = mock_info(
                before_all.default_sender.as_str().clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(1_000),
                }],
            );
            let msg = ExecuteMsg::RejectPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with reject proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());
            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            let msg = ExecuteMsg::RejectPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => assert_eq!(msg, "Proposal expired"),
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn only_creator_can_reject() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());
            let msg = ExecuteMsg::RejectPoll { poll_id: 1 };
            let env = mock_env();
            let info = mock_info(before_all.default_sender_two.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), env, info, msg);

            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Unauthorized");
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());
            let msg = ExecuteMsg::RejectPoll { poll_id: 1 };
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
            println!("{:?}", res);
            assert_eq!(res.messages.len(), 0);
            assert_eq!(res.attributes.len(), 2);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::RejectedByCreator);
        }
    }

    mod present {
        use super::*;
        use cosmwasm_std::CosmosMsg;

        // handle_present
        fn create_poll(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Some(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let _res = execute(deps, mock_env(), mock_info("addr0000", &[]), msg).unwrap();
        }
        fn create_poll_security_migration(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::SecurityMigration,
                amount: None,
                prize_per_rank: None,
                recipient: Some("newAddress".to_string()),
            };
            let _res = execute(deps, mock_env(), mock_info("addr0002", &[]), msg).unwrap();
            println!("{:?}", _res);
        }
        fn create_poll_dao_funding(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::DaoFunding,
                amount: Some(Uint128(22)),
                prize_per_rank: None,
                recipient: None,
            };
            let _res = execute(deps, mock_env(), mock_info("addr0002", &[]), msg).unwrap();
        }
        fn create_poll_statking_contract_migration(deps: DepsMut) {
            let msg = ExecuteMsg::Poll {
                description: "This is my first proposal".to_string(),
                proposal: Proposal::StakingContractMigration,
                amount: None,
                prize_per_rank: None,
                recipient: Some("newAddress".to_string()),
            };
            let _res = execute(deps, mock_env(), mock_info("addr0002", &[]), msg).unwrap();
            println!("{:?}", _res);
        }
        #[test]
        fn do_not_send_funds() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(
                before_all.default_sender.as_str().clone(),
                &[Coin {
                    denom: "ust".to_string(),
                    amount: Uint128(9_000_000),
                }],
            );
            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Do not send funds with present proposal")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_expired() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());
            // Save to storage

            POLL.update(
                deps.as_mut().storage,
                &1_u64.to_be_bytes(),
                |poll| -> StdResult<PollInfoState> {
                    match poll {
                        None => panic!("Unexpected error"),
                        Some(poll_state) => {
                            let mut poll_data = poll_state;
                            // Update the status to passed
                            poll_data.status = PollStatus::Rejected;
                            Ok(poll_data)
                        }
                    }
                },
            )
            .unwrap();
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Unauthorized")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn poll_still_in_progress() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            POLL.update(
                deps.as_mut().storage,
                &1_u64.to_be_bytes(),
                |poll| -> StdResult<PollInfoState> {
                    match poll {
                        None => panic!("Unexpected error"),
                        Some(poll_state) => {
                            let mut poll_data = poll_state;
                            // Update the status to passed
                            poll_data.end_height = env.block.height + 1;
                            Ok(poll_data)
                        }
                    }
                },
            )
            .unwrap();
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Proposal still in progress")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success_with_reject() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            default_init(deps.as_mut());
            create_poll(deps.as_mut());

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Rejected);
        }
        #[test]
        fn success_dao_funding() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(150_000),
                Decimal::zero(),
                Decimal::zero(),
            );

            default_init(deps.as_mut());
            // with admin renounce
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            let res = handle_renounce(deps.as_mut(), env, info);

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll_dao_funding(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            let state_before = read_state(deps.as_ref().storage).unwrap();

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            println!("{:?}", res);
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(res.messages.len(), 1);
            assert_eq!(
                res.messages[0],
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: deps
                        .api
                        .addr_humanize(&state_before.loterra_cw20_contract_address)
                        .unwrap()
                        .to_string(),
                    msg: Binary::from(
                        r#"{"transfer":{"recipient":"addr0002","amount":"22"}}"#.as_bytes()
                    ),
                    send: vec![]
                })
            );

            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);
        }
        #[test]
        fn success_staking_migration() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(150_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            create_poll_statking_contract_migration(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            let state_before = read_state(deps.as_ref().storage).unwrap();

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            println!("{:?}", res);
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);
            //let state = config(&mut deps);
            let state_after = read_state(deps.as_ref().storage).unwrap();
            assert_ne!(
                state_after.loterra_staking_contract_address,
                state_before.loterra_staking_contract_address
            );
            assert_eq!(
                deps.api
                    .addr_humanize(&state_after.loterra_staking_contract_address)
                    .unwrap()
                    .to_string(),
                "newAddress".to_string()
            );
        }
        #[test]
        fn success_security_migration() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(150_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            create_poll_security_migration(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;
            read_state(deps.as_ref().storage).unwrap();

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            println!("{:?}", res);
        }
        #[test]
        fn success_with_passed() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(150_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height + 1;

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll_security_migration(deps.as_mut());
            let msg = ExecuteMsg::Vote {
                poll_id: 2,
                approve: true,
            };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            println!("{:?}", res);
        }
        #[test]
        fn success_with_proposal_not_expired_yet_and_more_50_percent_weight_vote() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(15_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height - 1000;

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            assert_eq!(res.attributes.len(), 3);
            assert_eq!(res.messages.len(), 0);

            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            assert_eq!(poll_state.status, PollStatus::Passed);

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll_security_migration(deps.as_mut());
            let msg = ExecuteMsg::Vote {
                poll_id: 2,
                approve: true,
            };
            let res = execute(deps.as_mut(), env, info, msg).unwrap();
            println!("{:?}", res);
        }
        #[test]
        fn error_with_proposal_not_expired_yet_and_less_50_percent_weight_vote() {
            let before_all = before_all();
            let mut deps = mock_dependencies_custom(&[Coin {
                denom: "ust".to_string(),
                amount: Uint128(9_000_000),
            }]);
            deps.querier.with_token_balances(Uint128(200_000));
            deps.querier.with_holder(
                before_all.default_sender.clone(),
                Uint128(1_000),
                Decimal::zero(),
                Decimal::zero(),
            );
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            create_poll(deps.as_mut());

            let env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let msg = ExecuteMsg::Vote {
                poll_id: 1,
                approve: true,
            };

            let _res = execute(deps.as_mut(), env, info, msg);

            let mut env = mock_env();
            let info = mock_info(before_all.default_sender.as_str().clone(), &[]);
            let poll_state = POLL
                .load(deps.as_ref().storage, &1_u64.to_be_bytes())
                .unwrap();
            env.block.height = poll_state.end_height - 1000;

            let msg = ExecuteMsg::PresentPoll { poll_id: 1 };
            let res = execute(deps.as_mut(), env, info, msg);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Proposal still in progress")
                }
                _ => panic!("Unexpected error"),
            }
        }
    }

    mod safe_lock {
        use super::*;
        // handle_switch

        #[test]
        fn only_admin() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_two.as_str(), &[]);
            let res = handle_safe_lock(deps.as_mut(), info);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Unauthorized")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str(), &[]);
            // Switch to Off
            let res = handle_safe_lock(deps.as_mut(), info.clone()).unwrap();
            assert_eq!(res.messages.len(), 0);
            let state = read_state(deps.as_ref().storage).unwrap();
            assert!(state.safe_lock);
            // Switch to On
            let res = handle_safe_lock(deps.as_mut(), info.clone()).unwrap();
            println!("{:?}", res);
            let state = read_state(deps.as_ref().storage).unwrap();
            assert!(!state.safe_lock);
        }
    }

    mod renounce {
        use super::*;

        // handle_renounce
        #[test]
        fn only_admin() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_two.as_str(), &[]);
            let res = handle_renounce(deps.as_mut(), env, info);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Unauthorized")
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn safe_lock_on() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str(), &[]);
            let mut state = read_state(deps.as_ref().storage).unwrap();
            state.safe_lock = true;
            store_state(deps.as_mut().storage, &state).unwrap();

            let res = handle_renounce(deps.as_mut(), env, info);
            match res {
                Err(StdError::GenericErr { msg, .. }) => {
                    assert_eq!(msg, "Locked");
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn success() {
            let before_all = before_all();
            let mut deps = mock_dependencies(&[]);
            default_init(deps.as_mut());
            let env = mock_env();
            let info = mock_info(before_all.default_sender_owner.as_str().clone(), &[]);
            // Transfer power to admin
            let res = handle_renounce(deps.as_mut(), env.clone(), info).unwrap();
            assert_eq!(res.messages.len(), 0);
            let state = read_state(deps.as_ref().storage).unwrap();
            assert_ne!(
                state.admin,
                deps.api
                    .addr_canonicalize(&before_all.default_sender_owner)
                    .unwrap()
            );
            assert_eq!(
                state.admin,
                deps.api
                    .addr_canonicalize(&env.contract.address.as_str())
                    .unwrap()
            );
        }
    }
}
