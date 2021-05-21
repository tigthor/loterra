use crate::state::{PollStatus, Proposal, State, WinnerRewardClaims};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{HumanAddr, Uint128};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub denom_stable: String,
    pub block_time_play: u64,
    pub every_block_time_play: u64,
    pub public_sale_end_block_time: u64,
    pub poll_default_end_height: u64,
    pub token_holder_supply: Uint128,
    pub terrand_contract_address: HumanAddr,
    pub loterra_cw20_contract_address: HumanAddr,
    pub loterra_staking_contract_address: HumanAddr,
    pub dao_funds: Uint128,
    pub aterra_contract_address: HumanAddr,
    pub market_contract_address: HumanAddr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Registering to the lottery
    Register {
        address: Option<HumanAddr>,
        combination: Vec<String>,
    },
    /// Run the lottery
    Play {},
    /// Public sale buy the token holders with 1:1 ratio
    PublicSale {},
    /// Claim jackpot
    Claim { addresses: Option<Vec<HumanAddr>> },
    /// Collect jackpot
    Collect { address: Option<HumanAddr> },
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
    Combination { lottery_id: u64, address: HumanAddr },
    /// Winner lottery rank and address
    Winner { lottery_id: u64 },
    /// Get specific poll
    GetPoll { poll_id: u64 },
    /// Count players by lottery id
    CountPlayer { lottery_id: u64 },
    /// Count ticket sold by lottery id
    CountTicket { lottery_id: u64 },
    /// Count winner by rank and lottery id
    CountWinnerRank { lottery_id: u64, rank: u8 },
    /// Get winning combination by lottery id
    WinningCombination { lottery_id: u64 },
    /// Get the needed round for workers adding randomness to Terrand
    GetRound {},
    /// Query Terrand smart contract to get the needed randomness to play the lottery
    GetRandomness { round: u64 },
    /// Not used to be called directly
    /// Query Loterra smart contract to get the balance
    Balance { address: HumanAddr },
    /// Get specific holder, address and balance from loterra staking contract
    Holder { address: HumanAddr },
    /// Query Loterra send
    Transfer {
        recipient: HumanAddr,
        amount: Uint128,
    },
    /// Update balance of the staking contract with rewards
    UpdateGlobalIndex {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllCombinationResponse {
    pub combination: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WinnerResponse {
    pub address: HumanAddr,
    pub claims: WinnerRewardClaims,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllWinnersResponse {
    pub winners: Vec<WinnerResponse>,
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
