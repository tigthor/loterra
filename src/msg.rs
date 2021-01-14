use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::state::State;
use cosmwasm_std::{CanonicalAddr, Storage, Uint128, Binary};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denom: String,
    pub denomDelegation: String,
    pub denomShare: String,
    pub everyBlockHeight: u64,
    pub players: Vec<CanonicalAddr>,
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
    pub validatorMinAmountToAllowClaim: u64
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Registering to the lottery
    Register {},
    /// Run the lottery
    Play {
        round: u64,
        previous_signature: Binary,
        signature: Binary,
    },
    /// Claim 1 ticket every x block if you are a delegator
    Claim {},
    /// Buy the token holders with USCRT and get 1:1 ratio
    Ico {},
    /// Buy tickets with USCRT, 1 ticket is 1_000_000 USCRT (1SCRT)
    Buy {},
    /// Get reward
    Reward {}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
}

// We define a custom struct for each query response
pub type ConfigResponse = State;

