use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Send some coins to create an atomic swap")]
    EmptyBalance {},

    #[error("Send some funds")]
    NoFunds {},

    #[error("Must send '{0}' to buy lottery tickets")]
    MissingDenom(String),

    #[error("Sent unsupported denoms, must send '{0}' to buy lottery tickets")]
    ExtraDenom(String),

    #[error("You need to delegate to the lottery validator first")]
    NoDelegations{},

    #[error("Need players to play the lottery")]
    NoPlayers{},

    #[error("Sent extra delegation")]
    ExtraDelegation{},

    #[error("You have already claimed your reward for today")]
    AlreadyClaimed{},

    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
