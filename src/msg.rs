use crate::state::{PollStatus, Proposal, State, WinnerInfoState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Binary, CanonicalAddr, HumanAddr, Uint128};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denom_stable: String,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub public_sale_end_block: u64,
    pub poll_default_end_height: u64,
    pub token_holder_supply: Uint128,
    pub terrand_contract_address: HumanAddr,
    pub loterra_contract_address: HumanAddr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Registering to the lottery
    Register { combination: String },
    /// Run the lottery
    Play {},
    /// Public sale buy the token holders with 1:1 ratio
    PublicSale {},
    /// Claim holder reward
    Reward {},
    /// Claim jackpot
    Jackpot {},
    /// DAO
    /// Make a proposal
    Proposal {
        description: String,
        proposal: Proposal,
        amount: Option<Uint128>,
        prize_per_rank: Option<Vec<u8>>,
        contract_migration_address: Option<HumanAddr>,
    },
    /// Vote the proposal
    Vote { poll_id: u64, approve: bool },
    /// Valid a proposal
    PresentProposal { poll_id: u64 },
    /// Reject a proposal
    RejectProposal { poll_id: u64 },
    /// Admin
    /// Security owner can switch on off to prevent exploit
    SafeLock {},
    /// Admin renounce and restore contract address to admin for full decentralization
    Renounce {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Get the config state
    Config {},
    /// Combination lottery numbers and address
    Combination {},
    /// Winner lottery rank and address
    Winner {},
    /// Get specific poll
    GetPoll { poll_id: u64 },
    /// Get the needed round for workers adding randomness to Terrand
    GetRound {},
    /// Query Terrand smart contract to get the needed randomness to play the lottery
    GetRandomness { round: u64 },
    /// Query Loterra smart contract to get the balance
    Balance { address: HumanAddr },
    /// Query Loterra send
    Transfer {
        recipient: HumanAddr,
        amount: Uint128,
    },
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetPollResponse {
    pub creator: HumanAddr,
    pub status: PollStatus,
    pub end_height: u64,
    pub start_height: u64,
    pub description: String,
    pub amount: Uint128,
    pub prize_per_rank: Vec<u8>,
    pub migration_address: Option<HumanAddr>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Round {
    pub next_round: u64,
}

pub type RoundResponse = Round;

// We define a custom struct for each query response
pub type ConfigResponse = State;
