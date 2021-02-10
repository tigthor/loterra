use cosmwasm_std::{Binary, HumanAddr};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct TerrandResponse {
    pub randomness: Binary,
    pub worker: HumanAddr,
}
