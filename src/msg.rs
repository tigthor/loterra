use crate::state::{PollStatus, Proposal, State, WinnerInfoState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Binary, CanonicalAddr, HumanAddr, Uint128};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denomTicket: String,
    pub denomDelegation: String,
    pub denomStable: String,
    pub denomStableDecimal: Uint128,
    pub denomShare: String,
    pub everyBlockHeight: u64,
    pub blockTimePlay: u64,
    pub everyBlockTimePlay: u64,
    pub blockClaim: u64,
    pub publicSaleEndBlock: u64,
    pub pollEndHeight: u64,
    pub tokenHolderSupply: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Registering to the lottery
    Register { combination: String },
    /// Run the lottery
    Play {
        round: u64,
        previous_signature: Binary,
        signature: Binary,
    },
    /// Claim 1 ticket every x block if you are a delegator
    Ticket {},
    /// Public sale buy the token holders with 1:1 ratio
    PublicSale {},
    /// Buy tickets at 1:1 ratio only integer, float number will be rounded down
    Buy {},
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
        prizePerRank: Option<Vec<u8>>,
    },
    /// Vote the proposal
    Vote { pollId: u64, approve: bool },
    /// Valid a proposal
    PresentProposal { pollId: u64 },
    /// Reject a proposal
    RejectProposal { pollId: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Get the config state
    Config {},
    /// Get the last randomness
    LatestDrand {},
    /// Get a specific randomness
    GetRandomness { round: u64 },
    /// Combination lottery numbers and address
    Combination {},
    /// Winner lottery rank and address
    Winner {},
    /// Get specific poll
    GetPoll { pollId: u64 },
    /// Get the specific round to query from Drand to play the lottery
    GetRound {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GetResponse {
    pub randomness: Binary,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LatestResponse {
    pub round: u64,
    pub randomness: Binary,
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
    pub prizePerRank: Vec<u8>,
}

// We define a custom struct for each query response
pub type ConfigResponse = State;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Round {
    pub nextRound: u64,
}

pub type RoundResponse = Round;
