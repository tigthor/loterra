use cosmwasm_std::{Binary, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct TerrandResponse {
    pub randomness: Binary,
    pub worker: HumanAddr,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct LoterraBalanceResponse {
    pub balance: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetHolderResponse {
    pub address: HumanAddr,
    pub bonded: Uint128,
    pub un_bonded: Uint128,
    pub available: Uint128,
    pub period: u64,
}
