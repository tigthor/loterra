use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Binary, CanonicalAddr, Storage, Uint128, HumanAddr};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket,
    ReadonlyBucket, ReadonlySingleton, Singleton,
};

pub static CONFIG_KEY: &[u8] = b"config";
const COMBINATION_KEY: &[u8] = b"combination";
const WINNER_KEY: &[u8] = b"winner";
const POLL_KEY: &[u8] = b"poll";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub blockTimePlay: u64,
    pub everyBlockTimePlay: u64,
    pub publicSaleEndBlock: u64,
    pub denomStable: String,
    pub denomStableDecimal: Uint128,
    pub denomShare: String,
    pub claimReward: Vec<CanonicalAddr>,
    pub holdersRewards: Uint128,
    pub tokenHolderSupply: Uint128,
    pub combinationLen: u8,
    pub jackpotReward: Uint128,
    pub jackpotPercentageReward: u8,
    pub tokenHolderPercentageFeeReward: u8,
    pub feeForDrandWorkerInPercentage: u8,
    pub prizeRankWinnerPercentage: Vec<u8>,
    pub pollCount: u64,
    pub holdersMaxPercentageReward: u8,
    pub workerDrandMaxPercentageReward: u8,
    pub pollEndHeight: u64,
    pub pricePerTicketToRegister: Uint128,
    pub terrandContractAddress: HumanAddr,
}

pub fn config<S: Storage>(storage: &mut S) -> Singleton<S, State> {
    singleton(storage, CONFIG_KEY)
}
pub fn config_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, State>  {
    singleton_read(storage, CONFIG_KEY)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Combination {
    pub addresses: Vec<CanonicalAddr>,
}

pub fn combination_storage<T: Storage>(storage: &mut T) -> Bucket<T, Combination> {
    bucket(COMBINATION_KEY, storage )
}

pub fn combination_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Combination> {
    bucket_read(COMBINATION_KEY, storage )
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
    bucket(WINNER_KEY, storage )
}


pub fn winner_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, Winner> {
    bucket_read(WINNER_KEY , storage)
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
    pub prizeRank: Vec<u8>,
    pub proposal: Proposal,
}


pub fn poll_storage<T: Storage>(storage: &mut T) -> Bucket<T, PollInfoState>{
    bucket(POLL_KEY, storage)
}

pub fn poll_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, PollInfoState> {
    bucket_read(POLL_KEY, storage)
}
