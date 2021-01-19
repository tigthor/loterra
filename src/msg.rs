use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::state::{State, WinnerInfoState};

use cosmwasm_std::{CanonicalAddr, Storage, Uint128, Binary};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denomTicket: String,
    pub denomDelegation: String,
    pub denomDelegationDecimal: Uint128,
    pub denomShare: String,
    pub everyBlockHeight: u64,
    pub claimTicket: Vec<CanonicalAddr>,
    pub claimReward: Vec<CanonicalAddr>,
    pub blockPlay: u64,
    pub blockClaim: u64,
    pub blockIcoTimeframe: u64,
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Registering to the lottery
    Register {
        combination: String,
    },
    /// Run the lottery
    Play {
        round: u64,
        previous_signature: Binary,
        signature: Binary,
    },
    /// Claim 1 ticket every x block if you are a delegator
    Ticket {},
    /// Buy the token holders with USCRT and get 1:1 ratio
    Ico {},
    /// Buy tickets with USCRT, 1 ticket is 1_000_000 USCRT (1SCRT)
    Buy {},
    /// Claim holder reward
    Reward {},
    /// Claim jackpot
    Jackpot {}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Get the config state
    Config {},
    /// Get the last randomness
    LatestDrand {},
    /// Get a specific randomness
    GetRandomness {
        round: u64
    },
    /// Combination lottery numbers and address
    Combination {},
    /// Winner lottery rank and address
    Winner {}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetResponse {
    pub randomness: Binary
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LatestResponse {
    pub round: u64,
    pub randomness: Binary
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CombinationInfo {
    pub key: String,
    pub addresses: Vec<CanonicalAddr>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllCombinationResponse {
    pub combination: Vec<CombinationInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerInfo {
    pub rank: u8,
    pub winners: Vec<WinnerInfoState>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllWinnerResponse {
    pub winner: Vec<WinnerInfo>,
}

// We define a custom struct for each query response
pub type ConfigResponse = State;

