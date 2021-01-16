use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, Storage, Uint128, Binary, Order};
use cosmwasm_storage::{singleton, singleton_read, ReadonlySingleton, Singleton, PrefixedStorage, prefixed, ReadonlyPrefixedStorage, prefixed_read, Bucket, ReadonlyBucket, bucket_read, bucket};
use crate::error::ContractError;


pub static CONFIG_KEY: &[u8] = b"config";
const BEACONS_KEY: &[u8] = b"beacons";
const COMBINATION_KEY: &[u8] = b"combination";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub owner: CanonicalAddr,
    pub players: Vec<CanonicalAddr>,
    pub blockPlay: u64,
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
/*
pub fn combination_storage(storage: &mut dyn Storage) -> PrefixedStorage{
    prefixed(storage, COMBINATION_KEY)
}

pub fn combination_storage_read(storage: &dyn Storage) -> ReadonlyPrefixedStorage{
    prefixed_read(storage, COMBINATION_KEY)
}*/

