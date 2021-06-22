use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Do not send funds")]
    DoNotSendFunds {},

    #[error("Poll is expired")]
    PollExpired {},

    #[error("Poll vote closed")]
    PollClosed {},

    #[error("Already voted")]
    AlreadyVoted {},

    #[error("Only stakers can vote")]
    OnlyStakers {},

    #[error("Contract is locked")]
    Locked {},

    #[error("Contract is deactivated")]
    Deactivated {},

    #[error("Lottery is about to start wait until the end before register")]
    LotteryStart {},

    #[error("Not authorized use combination of [a-f] and [0-9] with length {0}")]
    CombinationFormatError(a),

    #[error("you need to send {0} {1} per combination in order to register")]
    RegisteredAmount(a,b),

    #[error("Only send {0} to register")]
    RegisterRequireAmount(a),
    #[error("Send {0} {1}")]
    SendRequiredAmount(a, b)
    // "you need to send {}{} per combination in order to register
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
