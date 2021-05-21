use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, HumanAddr, Order, StdResult, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};
use std::ops::Add;

pub static CONFIG_KEY: &[u8] = b"config";
const COMBINATION_KEY: &[u8] = b"combination";
const WINNER_KEY: &[u8] = b"winner";
const WINNER_RANK_KEY: &[u8] = b"rank";
const POLL_KEY: &[u8] = b"poll";
const VOTE_KEY: &[u8] = b"user";
const WINNING_COMBINATION_KEY: &[u8] = b"winning";
const PLAYER_COUNT_KEY: &[u8] = b"player";
const TICKET_COUNT_KEY: &[u8] = b"ticket";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub admin: CanonicalAddr,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub public_sale_end_block_time: u64,
    pub denom_stable: String,
    pub token_holder_supply: Uint128,
    pub combination_len: u8,
    pub jackpot_reward: Uint128,
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
    pub latest_winning_number: String,
    pub dao_funds: Uint128,
    pub lottery_counter: u64,
    pub aterra_contract_address: CanonicalAddr,
    pub market_contract_address: CanonicalAddr,
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
    pub migration_address: Option<HumanAddr>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerRewardClaims {
    pub claimed: bool,
    pub ranks: Vec<u8>,
}

pub fn config<S: Storage>(storage: &mut S) -> Singleton<S, State> {
    singleton(storage, CONFIG_KEY)
}
pub fn config_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, State> {
    singleton_read(storage, CONFIG_KEY)
}

pub fn combination_save<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
    address: CanonicalAddr,
    combination: Vec<String>,
) -> StdResult<()> {
    let mut exist = true;
    // Save combination by senders
    user_combination_bucket(storage, lottery_id).update(
        address.as_slice(),
        |exists| match exists {
            Some(combinations) => {
                let mut modified = combinations;
                modified.extend(combination.clone());
                Ok(modified)
            }
            None => {
                exist = false;
                Ok(combination.clone())
            }
        },
    )?;
    if !exist {
        count_player_by_lottery(storage)
            .update(&lottery_id.to_be_bytes(), |exists| match exists {
                None => Ok(Uint128(1)),
                Some(p) => Ok(p.add(Uint128(1))),
            })
            .map(|_| ())?
    }
    count_total_ticket_by_lottery(storage)
        .update(&lottery_id.to_be_bytes(), |exists| match exists {
            None => Ok(Uint128(combination.len() as u128)),
            Some(p) => Ok(p.add(Uint128(combination.len() as u128))),
        })
        .map(|_| ())
}

pub fn user_combination_bucket<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
) -> Bucket<T, Vec<String>> {
    Bucket::multilevel(&[COMBINATION_KEY, &lottery_id.to_be_bytes()], storage)
}

pub fn user_combination_bucket_read<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> ReadonlyBucket<T, Vec<String>> {
    ReadonlyBucket::multilevel(&[COMBINATION_KEY, &lottery_id.to_be_bytes()], storage)
}

// index: lottery_id | count
pub fn count_player_by_lottery<T: Storage>(storage: &mut T) -> Bucket<T, Uint128> {
    bucket(PLAYER_COUNT_KEY, storage)
}
// index: lottery_id | count
pub fn count_player_by_lottery_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Uint128> {
    bucket_read(PLAYER_COUNT_KEY, storage)
}

// index: lottery_id | count
pub fn count_total_ticket_by_lottery<T: Storage>(storage: &mut T) -> Bucket<T, Uint128> {
    bucket(TICKET_COUNT_KEY, storage)
}
// index: lottery_id | count
pub fn count_total_ticket_by_lottery_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Uint128> {
    bucket_read(TICKET_COUNT_KEY, storage)
}

// if an address won a lottery in this round, saved by rank
// index address -> winner claim
pub fn winner_storage<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
) -> Bucket<T, WinnerRewardClaims> {
    Bucket::multilevel(&[WINNER_KEY, &lottery_id.to_be_bytes()], storage)
}

pub fn winner_storage_read<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> ReadonlyBucket<T, WinnerRewardClaims> {
    ReadonlyBucket::multilevel(&[WINNER_KEY, &lottery_id.to_be_bytes()], storage)
}

// save winner
pub fn save_winner<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
    addr: CanonicalAddr,
    rank: u8,
) -> StdResult<()> {
    winner_storage(storage, lottery_id).update(addr.as_slice(), |exists| match exists {
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
    })?;
    winner_count_by_rank(storage, lottery_id)
        .update(&rank.to_be_bytes(), |exists| match exists {
            None => Ok(Uint128(1)),
            Some(r) => Ok(r.add(Uint128(1))),
        })
        .map(|_| ())
}

pub fn all_winners<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> StdResult<Vec<(CanonicalAddr, WinnerRewardClaims)>> {
    winner_storage_read(storage, lottery_id)
        .range(None, None, Order::Ascending)
        .map(|item| {
            let (addr, claim) = item?;
            Ok((CanonicalAddr::from(addr), claim))
        })
        .collect()
}

// index: lottery_id | rank -> count
pub fn winner_count_by_rank_read<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> ReadonlyBucket<T, Uint128> {
    ReadonlyBucket::multilevel(&[WINNER_RANK_KEY, &lottery_id.to_be_bytes()], storage)
}

// index: lottery_id | rank -> count
pub fn winner_count_by_rank<T: Storage>(storage: &mut T, lottery_id: u64) -> Bucket<T, Uint128> {
    Bucket::multilevel(&[WINNER_RANK_KEY, &lottery_id.to_be_bytes()], storage)
}

pub fn poll_storage<T: Storage>(storage: &mut T) -> Bucket<T, PollInfoState> {
    bucket(POLL_KEY, storage)
}

pub fn poll_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, PollInfoState> {
    bucket_read(POLL_KEY, storage)
}

// poll vote storage index = VOTE_KEY:poll_id:address -> bool
pub fn poll_vote_storage<T: Storage>(storage: &mut T, poll_id: u64) -> Bucket<T, bool> {
    Bucket::multilevel(&[VOTE_KEY, &poll_id.to_be_bytes()], storage)
}

pub fn poll_vote_storage_read<T: Storage>(storage: &T, poll_id: u64) -> ReadonlyBucket<T, bool> {
    ReadonlyBucket::multilevel(&[VOTE_KEY, &poll_id.to_be_bytes()], storage)
}

pub fn lottery_winning_combination_storage<T: Storage>(storage: &mut T) -> Bucket<T, String> {
    bucket(WINNING_COMBINATION_KEY, storage)
}

pub fn lottery_winning_combination_storage_read<T: Storage>(
    storage: &T,
) -> ReadonlyBucket<T, String> {
    bucket_read(WINNING_COMBINATION_KEY, storage)
}
