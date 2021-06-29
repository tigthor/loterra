use crate::state::{PollStatus, Proposal, State, WinnerRewardClaims};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom_stable: String,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub poll_default_end_height: u64,
    pub terrand_contract_address: String,
    pub loterra_cw20_contract_address: String,
    pub loterra_staking_contract_address: String,
    pub holders_bonus_block_time_end: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Registering to the lottery
    Register {
        address: Option<String>,
        combination: Vec<String>,
    },
    /// Run the lottery
    Play {},
    /// Claim jackpot
    Claim { addresses: Option<Vec<String>> },
    /// Collect jackpot
    Collect { address: Option<String> },
    /// DAO
    /// Make a proposal
    Poll {
        description: String,
        proposal: Proposal,
        amount: Option<Uint128>,
        prize_per_rank: Option<Vec<u8>>,
        recipient: Option<String>,
    },
    /// Vote the proposal
    Vote { poll_id: u64, approve: bool },
    /// Valid a proposal
    PresentPoll { poll_id: u64 },
    /// Reject a proposal
    RejectPoll { poll_id: u64 },
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
    Combination { lottery_id: u64, address: String },
    /// Winner lottery rank and address
    Winner { lottery_id: u64 },
    /// Get specific poll
    GetPoll { poll_id: u64 },
    /// Count players by lottery id
    CountPlayer { lottery_id: u64 },
    /// Count ticket sold by lottery id
    CountTicket { lottery_id: u64 },
    /// Count winner by rank and lottery id
    CountWinner { lottery_id: u64, rank: u8 },
    /// Get winning combination by lottery id
    WinningCombination { lottery_id: u64 },
    /// Get the jackpot by lottery id
    Jackpot { lottery_id: u64 },
    /// Get all players by lottery id
    Players { lottery_id: u64 },
    /// Get the needed round for workers adding randomness to Terrand
    GetRound {},
    /// Query Terrand smart contract to get the needed randomness to play the lottery
    GetRandomness { round: u64 },
    /// Not used to be called directly
    /// Get specific holder, address and balance from loterra staking contract
    Holder { address: String },
    /// Get all holders from loterra staking contract
    Holders {},
    /// Update balance of the staking contract with rewards
    UpdateGlobalIndex {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllCombinationResponse {
    pub combination: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerResponse {
    pub address: String,
    pub claims: WinnerRewardClaims,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllWinnersResponse {
    pub winners: Vec<WinnerResponse>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetPollResponse {
    pub creator: String,
    pub status: PollStatus,
    pub end_height: u64,
    pub start_height: u64,
    pub description: String,
    pub amount: Uint128,
    pub prize_per_rank: Vec<u8>,
    pub migration_address: Option<String>,
    pub weight_yes_vote: Uint128,
    pub weight_no_vote: Uint128,
    pub yes_vote: u64,
    pub no_vote: u64,
    pub proposal: Proposal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Round {
    pub next_round: u64,
}

pub type RoundResponse = Round;

// We define a custom struct for each query response
pub type ConfigResponse = State;
