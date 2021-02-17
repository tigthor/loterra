use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, HumanAddr, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};

pub static CONFIG_KEY: &[u8] = b"config";
const COMBINATION_KEY: &[u8] = b"combination";
const WINNER_KEY: &[u8] = b"winner";
const POLL_KEY: &[u8] = b"poll";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub admin: CanonicalAddr,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub public_sale_end_block: u64,
    pub denom_stable: String,
    pub token_holder_supply: Uint128,
    pub combination_len: u8,
    pub jackpot_reward: Uint128,
    pub jackpot_percentage_reward: u8,
    pub token_holder_percentage_fee_reward: u8,
    pub fee_for_drand_worker_in_percentage: u8,
    pub prize_rank_winner_percentage: Vec<u8>,
    pub poll_count: u64,
    pub holders_max_percentage_reward: u8,
    pub worker_drand_max_percentage_reward: u8,
    pub poll_default_end_height: u64,
    pub price_per_ticket_to_register: Uint128,
    pub terrand_contract_address: CanonicalAddr,
    pub loterra_contract_address: CanonicalAddr,
    pub safe_lock: bool,
    pub last_winning_number: String,
}

pub fn config<S: Storage>(storage: &mut S) -> Singleton<S, State> {
    singleton(storage, CONFIG_KEY)
}
pub fn config_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, State> {
    singleton_read(storage, CONFIG_KEY)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Combination {
    pub addresses: Vec<CanonicalAddr>,
}

pub fn combination_storage<T: Storage>(storage: &mut T) -> Bucket<T, Combination> {
    bucket(COMBINATION_KEY, storage)
}

pub fn combination_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Combination> {
    bucket_read(COMBINATION_KEY, storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerInfoState {
    pub claimed: bool,
    pub address: CanonicalAddr,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Winner {
    pub winners: Vec<WinnerInfoState>,
}

pub fn winner_storage<T: Storage>(storage: &mut T) -> Bucket<T, Winner> {
    bucket(WINNER_KEY, storage)
}

pub fn winner_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Winner> {
    bucket_read(WINNER_KEY, storage)
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
    ClaimEveryBlock,
    AmountToRegister,
    SecurityMigration,
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
    pub yes_voters: Vec<CanonicalAddr>,
    pub no_voters: Vec<CanonicalAddr>,
    pub amount: Uint128,
    pub prize_rank: Vec<u8>,
    pub proposal: Proposal,
    pub migration_address: Option<HumanAddr>,
}

pub fn poll_storage<T: Storage>(storage: &mut T) -> Bucket<T, PollInfoState> {
    bucket(POLL_KEY, storage)
}

pub fn poll_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, PollInfoState> {
    bucket_read(POLL_KEY, storage)
}
