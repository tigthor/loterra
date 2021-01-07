use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::state::State;
use cosmwasm_std::{CanonicalAddr, Storage};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denom: String,
    pub denomDelegation: String,
    pub everyBlockHeight: u64,
    pub players: Vec<CanonicalAddr>,
    pub claimTicket: Vec<CanonicalAddr>,
    pub blockPlay: u64,
    pub blockClaim: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Subscribe to the lottery
    Register {},
    /// Owner can run the lottery
    Play {},
    Claim {},
    Ico {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
}

// We define a custom struct for each query response
pub type ConfigResponse = State;

