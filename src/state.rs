use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, HumanAddr, Order, StdResult, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};

pub static CONFIG_KEY: &[u8] = b"config";
const COMBINATION_KEY: &[u8] = b"combination";
const WINNER_KEY: &[u8] = b"winner";
const POLL_KEY: &[u8] = b"poll";
const VOTE_KEY: &[u8] = b"user";

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
    pub poll_default_end_height: u64,
    pub price_per_ticket_to_register: Uint128,
    pub terrand_contract_address: CanonicalAddr,
    pub loterra_cw20_contract_address: CanonicalAddr,
    pub loterra_staking_contract_address: CanonicalAddr,
    pub safe_lock: bool,
    pub latest_winning_number: String,
    pub dao_funds: Uint128,
    pub lottery_counter: u64,
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

// index = COMBINATION_KEY | lottery_id | combination -> address
// TODO maybe implement index COMBINATION_KEY | lottery_id | combination | address -> true ?
pub fn combination_bucket<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
) -> Bucket<T, Vec<CanonicalAddr>> {
    Bucket::multilevel(&[COMBINATION_KEY, &lottery_id.to_be_bytes()], storage)
}

// index = COMBINATION_KEY | lottery_id | combination -> address
pub fn combination_bucket_read<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> ReadonlyBucket<T, Vec<CanonicalAddr>> {
    ReadonlyBucket::multilevel(&[COMBINATION_KEY, &lottery_id.to_be_bytes()], storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerRewardClaims {
    pub claimed: bool,
    pub ranks: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RewardClaim {
    pub rank: u8,
    pub claimed: bool,
}

// if an address won a lottery in this round, saved by rank
// index address -> winner claim
pub fn winner_storage<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
) -> Bucket<T, WinnerRewardClaims> {
    Bucket::multilevel(&[WINNER_KEY, &lottery_id.to_be_bytes()], storage)
}

// save winner
pub fn save_winner<T: Storage>(
    storage: &mut T,
    lottery_id: u64,
    addr: CanonicalAddr,
    rank: u8,
) -> StdResult<WinnerRewardClaims> {
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
    })
}

pub fn winner_storage_read<T: Storage>(
    storage: &T,
    lottery_id: u64,
) -> ReadonlyBucket<T, WinnerRewardClaims> {
    ReadonlyBucket::multilevel(&[WINNER_KEY, &lottery_id.to_be_bytes()], storage)
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
pub struct PollVoters {
    pub voter: CanonicalAddr,
    pub vote: bool,
    pub weight: Uint128,
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

pub fn poll_storage<T: Storage>(storage: &mut T) -> Bucket<T, PollInfoState> {
    bucket(POLL_KEY, storage)
}

pub fn poll_storage_read<T: Storage>(storage: &T) -> ReadonlyBucket<T, PollInfoState> {
    bucket_read(POLL_KEY, storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UserInfoState {
    pub voted: Vec<u64>,
}

// poll vote storage index = VOTE_KEY:poll_id:address -> bool
pub fn poll_vote_storage<T: Storage>(storage: &mut T, poll_id: u64) -> Bucket<T, bool> {
    Bucket::multilevel(&[VOTE_KEY, &poll_id.to_be_bytes()], storage)
}

pub fn poll_vote_storage_read<T: Storage>(storage: &T, poll_id: u64) -> ReadonlyBucket<T, bool> {
    ReadonlyBucket::multilevel(&[VOTE_KEY, &poll_id.to_be_bytes()], storage)
}

#[cfg(test)]
mod tests {
    use crate::msg::{HandleMsg, InitMsg};
    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use cosmwasm_std::StdError::GenericErr;
    use cosmwasm_std::{Api, CosmosMsg, HumanAddr, Storage, Uint128};

    mod combination {
        use super::*;
        use cosmwasm_std::Order;
        use cosmwasm_storage::to_length_prefixed_nested;

        /*
        #[test]
        fn test_save_combination() {
            let mut deps = mock_dependencies(20, &[]);
            let lottery_id = 1u64;
            save_combination(&mut deps.storage, lottery_id, "AB".to_string(), CanonicalAddr::from(vec![1u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "AB".to_string(), CanonicalAddr::from(vec![2u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "AB".to_string(), CanonicalAddr::from(vec![3u8])).unwrap();

            let res: StdResult<Vec<(Vec<u8>, bool)>> = ReadonlyBucket::multilevel(&[COMBINATION_KEY, &lottery_id.to_be_bytes(), "AB".as_bytes()], &deps.storage)
                .range(None, None, Order::Ascending)
                .collect();
            assert_eq!(res.unwrap(), vec![(vec![1u8], true), (vec![2u8], true), (vec![3u8], true)]);
        }

        #[test]
        fn test_combination_matches() {
            let mut deps = mock_dependencies(20, &[]);
            let lottery_id = 1u64;
            // winning combination AB11CC
            // second comb AB13CC
            // third comb  ABCC1C
            save_combination(&mut deps.storage, lottery_id, "AB11CC".to_string(), CanonicalAddr::from(vec![1u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "AB1".to_string(), CanonicalAddr::from(vec![2u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "AB2".to_string(), CanonicalAddr::from(vec![3u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "ABC".to_string(), CanonicalAddr::from(vec![3u8])).unwrap();
            save_combination(&mut deps.storage, lottery_id, "ABCCC".to_string(), CanonicalAddr::from(vec![3u8])).unwrap();

            let matches = combination_matches(&deps.storage, lottery_id, "AB".to_string()).unwrap();
            println!("{:?}", matches)
        }

         */
    }
}
