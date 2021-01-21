use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, Storage, Uint128, Binary, Order};
use cosmwasm_storage::{singleton, singleton_read, ReadonlySingleton, Singleton, PrefixedStorage, prefixed, ReadonlyPrefixedStorage, prefixed_read, Bucket, ReadonlyBucket, bucket_read, bucket};
use crate::error::ContractError;


pub static CONFIG_KEY: &[u8] = b"config";
const BEACONS_KEY: &[u8] = b"beacons";
const COMBINATION_KEY: &[u8] = b"combination";
const WINNER_KEY: &[u8] = b"winner";
const POLL_KEY: &[u8] = b"poll";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub owner: CanonicalAddr,
    pub blockPlay: u64,
    pub blockTimePlay: u64,
    pub everyBlockTimePlay: u64,
    pub blockClaim: u64,
    pub blockIcoTimeframe: u64,
    pub everyBlockHeight: u64,
    pub denomTicket: String,
    pub denomDelegation: String,
    pub denomDelegationDecimal: Uint128,
    pub denomShare: String,
    pub claimTicket: Vec<CanonicalAddr>,
    pub claimReward: Vec<CanonicalAddr>,
    pub holdersRewards: Uint128,
    pub tokenHolderSupply: Uint128,
    pub drandPublicKey: Binary,
    pub drandPeriod: u64,
    pub drandGenesisTime: u64,
    pub validatorMinAmountToAllowClaim: u64,
    pub delegatorMinAmountInDelegation: Uint128,
    pub combinationLen: u8,
    pub jackpotReward: Uint128,
    pub jackpotPercentageReward: u64,
    pub tokenHolderPercentageFeeReward: u64,
    pub feeForDrandWorkerInPercentage: u64,
    pub prizeRankWinnerPercentage: Vec<u64>,
    pub pollCount: u64,

}

pub fn config(storage: &mut dyn Storage) -> Singleton<State> {
    singleton(storage, CONFIG_KEY)
}

pub fn config_read(storage: &dyn Storage) -> ReadonlySingleton<State> {
    singleton_read(storage, CONFIG_KEY)
}

pub fn beacons_storage(storage: &mut dyn Storage) -> PrefixedStorage{
    prefixed(storage, BEACONS_KEY)
}
pub fn beacons_storage_read(storage: &dyn Storage) -> ReadonlyPrefixedStorage{
    prefixed_read(storage, BEACONS_KEY)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Combination {
    pub addresses: Vec<CanonicalAddr>,
}
pub fn combination_storage(storage: &mut dyn Storage) -> Bucket<Combination>{
    bucket(storage, COMBINATION_KEY)
}

pub fn combination_storage_read(storage: &dyn Storage) -> ReadonlyBucket<Combination>{
    bucket_read(storage, COMBINATION_KEY)
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

pub fn winner_storage(storage: &mut dyn Storage) -> Bucket<Winner>{
    bucket(storage, WINNER_KEY)
}

pub fn winner_storage_read(storage: &dyn Storage) -> ReadonlyBucket<Winner>{
    bucket_read(storage, WINNER_KEY)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub enum  PollStatus {
    InProgress,
    Passed,
    Rejected,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct PollInfoState {
    pub creator: CanonicalAddr,
    pub status: PollStatus,
    pub end_height: u64,
    pub start_height: u64,
    pub description: String,
    pub yes_voters: Vec<CanonicalAddr>,
    pub no_voters: Vec<CanonicalAddr>,
    pub amount: Option<Uint128>,
    pub prizeRank: Option<Vec<u64>>
}

pub fn poll_storage(storage: &mut dyn Storage) -> Bucket<PollInfoState>{
    bucket(storage, POLL_KEY)
}

pub fn poll_storage_read(storage: &dyn Storage) -> ReadonlyBucket<PollInfoState>{
    bucket_read(storage, POLL_KEY)
}
/*
pub fn combination_storage(storage: &mut dyn Storage) -> PrefixedStorage{
    prefixed(storage, COMBINATION_KEY)
}

pub fn combination_storage_read(storage: &dyn Storage) -> ReadonlyPrefixedStorage{
    prefixed_read(storage, COMBINATION_KEY)
}*/

