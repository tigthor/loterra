use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, Deps, Order, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};
use std::ops::Add;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub admin: CanonicalAddr,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub denom_stable: String,
    pub combination_len: u8,
    pub jackpot_percentage_reward: u8,
    pub token_holder_percentage_fee_reward: u8,
    pub fee_for_drand_worker_in_percentage: u8,
    pub prize_rank_winner_percentage: Vec<u8>,
    pub poll_count: u64,
    pub poll_default_end_height: u64,
    pub price_per_ticket_to_register: Uint128,
    pub terrand_contract_address: CanonicalAddr,
    pub loterra_cw20_contract_address: CanonicalAddr,
    pub loterra_staking_contract_address: CanonicalAddr,
    pub safe_lock: bool,
    pub lottery_counter: u64,
    pub holders_bonus_block_time_end: u64,
}
pub const STATE: Item<State> = Item::new("state");
pub fn store_state(storage: &mut dyn Storage, state: &State) -> StdResult<()> {
    STATE.save(storage, state)
}

pub fn read_state(storage: &dyn Storage) -> StdResult<State> {
    STATE.load(storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum PollStatus {
    InProgress,
    Passed,
    Rejected,
    RejectedByCreator,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum Proposal {
    LotteryEveryBlockTime,
    HolderFeePercentage,
    DrandWorkerFeePercentage,
    PrizePerRank,
    JackpotRewardPercentage,
    AmountToRegister,
    SecurityMigration,
    DaoFunding,
    StakingContractMigration,
    PollSurvey,
    // test purpose
    NotExist,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PollInfoState {
    pub creator: CanonicalAddr,
    pub status: PollStatus,
    pub end_height: u64,
    pub start_height: u64,
    pub description: String,
    pub weight_yes_vote: Uint128,
    pub weight_no_vote: Uint128,
    pub yes_vote: u64,
    pub no_vote: u64,
    pub amount: Uint128,
    pub prize_rank: Vec<u8>,
    pub proposal: Proposal,
    pub migration_address: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerRewardClaims {
    pub claimed: bool,
    pub ranks: Vec<u8>,
}

pub const PREFIXED_USER_COMBINATION: Map<(&[u8], &[u8]), Vec<String>> = Map::new("holders");
pub const PREFIXED_WINNER: Map<(&[u8], &[u8]), WinnerRewardClaims> = Map::new("winner");
pub const PREFIXED_RANK: Map<(&[u8], &[u8]), Uint128> = Map::new("rank");
pub const PREFIXED_POLL_VOTE: Map<(&[u8], &[u8]), bool> = Map::new("vote");
pub const ALL_USER_COMBINATION: Map<&[u8], Vec<CanonicalAddr>> = Map::new("players");
pub const COUNT_PLAYERS: Map<&[u8], Uint128> = Map::new("count");
pub const COUNT_TICKETS: Map<&[u8], Uint128> = Map::new("tickets");
pub const POLL: Map<&[u8], PollInfoState> = Map::new("poll");
pub const WINNING_COMBINATION: Map<&[u8], String> = Map::new("winning");
pub const JACKPOT: Map<&[u8], Uint128> = Map::new("jackpot");

pub fn combination_save(
    storage: &mut dyn Storage,
    lottery_id: u64,
    address: CanonicalAddr,
    combination: Vec<String>,
) -> StdResult<()> {
    let mut exist = true;
    // Save combination by senders
    PREFIXED_USER_COMBINATION.update(
        storage,
        (&lottery_id.to_be_bytes(), address.as_slice()),
        |exists| -> StdResult<Vec<String>> {
            match exists {
                Some(combinations) => {
                    let mut modified = combinations;
                    modified.extend(combination.clone());
                    Ok(modified)
                }
                None => {
                    exist = false;
                    Ok(combination.clone())
                }
            }
        },
    )?;
    if !exist {
        ALL_USER_COMBINATION.update(
            storage,
            &lottery_id.to_be_bytes(),
            |exist| -> StdResult<Vec<CanonicalAddr>> {
                match exist {
                    None => Ok(vec![address]),
                    Some(players) => {
                        let mut data = players;
                        data.push(address);
                        Ok(data)
                    }
                }
            },
        )?;
        COUNT_PLAYERS
            .update(
                storage,
                &lottery_id.to_be_bytes(),
                |exists| -> StdResult<Uint128> {
                    match exists {
                        None => Ok(Uint128(1)),
                        Some(p) => Ok(p.add(Uint128(1))),
                    }
                },
            )
            .map(|_| ())?
    }
    COUNT_TICKETS
        .update(
            storage,
            &lottery_id.to_be_bytes(),
            |exists| -> StdResult<Uint128> {
                match exists {
                    None => Ok(Uint128(combination.len() as u128)),
                    Some(p) => Ok(p.add(Uint128(combination.len() as u128))),
                }
            },
        )
        .map(|_| ())
}

// save winner
pub fn save_winner(
    storage: &mut dyn Storage,
    lottery_id: u64,
    addr: CanonicalAddr,
    rank: u8,
) -> StdResult<()> {
    PREFIXED_WINNER.update(
        storage,
        (&lottery_id.to_be_bytes(), addr.as_slice()),
        |exists| -> StdResult<WinnerRewardClaims> {
            match exists {
                None => Ok(WinnerRewardClaims {
                    claimed: false,
                    ranks: vec![rank],
                }),
                Some(claims) => {
                    let mut ranks = claims.ranks;
                    ranks.push(rank);
                    Ok(WinnerRewardClaims {
                        claimed: false,
                        ranks,
                    })
                }
            }
        },
    )?;

    PREFIXED_RANK
        .update(
            storage,
            (&lottery_id.to_be_bytes(), &rank.to_be_bytes()),
            |exists| -> StdResult<Uint128> {
                match exists {
                    None => Ok(Uint128(1)),
                    Some(r) => Ok(r.add(Uint128(1))),
                }
            },
        )
        .map(|_| ())
}

pub fn all_winners(
    deps: &Deps,
    lottery_id: u64,
) -> StdResult<Vec<(CanonicalAddr, WinnerRewardClaims)>> {
    PREFIXED_WINNER
        .prefix(&lottery_id.to_be_bytes())
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, claim) = item?;
            Ok((CanonicalAddr::from(addr), claim))
        })
        .collect()
}
