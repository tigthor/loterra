use cosmwasm_std::{
    attr, to_binary, BankMsg, Binary, CanonicalAddr, Coin, Decimal, Deps, DepsMut, Env,
    HandleResponse, InitResponse, MessageInfo, Order, StdError, StdResult, Uint128,
};

use crate::error::ContractError;
use crate::msg::{
    AllCombinationResponse, AllWinnerResponse, CombinationInfo, ConfigResponse, GetPollResponse,
    GetResponse, HandleMsg, InitMsg, LatestResponse, QueryMsg, RoundResponse, WinnerInfo,
};
use crate::state::{
    beacons_storage, beacons_storage_read, combination_storage, combination_storage_read, config,
    config_read, poll_storage, poll_storage_read, winner_storage, winner_storage_read, Combination,
    PollInfoState, PollStatus, Proposal, State, Winner, WinnerInfoState,
};

use drand_verify::{g1_from_variable, verify};
use hex;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::ops::{Mul, Sub};

const MIN_DESC_LEN: u64 = 6;
const MAX_DESC_LEN: u64 = 64;

// Note, you can use StdResult in some functions where you do not
// make use of the custom errors
// #[serde(rename_all = "snake_case")]
pub fn init(deps: DepsMut, _env: Env, info: MessageInfo, msg: InitMsg) -> StdResult<InitResponse> {
    let state = State {
        owner: deps.api.canonical_address(&info.sender)?,
        blockTimePlay: msg.blockTimePlay,
        everyBlockTimePlay: msg.everyBlockTimePlay,
        blockClaim: msg.blockClaim,
        blockIcoTimeframe: msg.blockIcoTimeframe,
        everyBlockHeight: msg.everyBlockHeight,
        denomTicket: msg.denomTicket,
        denomDelegation: msg.denomDelegation,
        denomDelegationDecimal: msg.denomDelegationDecimal,
        denomShare: msg.denomShare,
        claimTicket: msg.claimTicket,
        claimReward: msg.claimReward,
        holdersRewards: msg.holdersRewards,
        tokenHolderSupply: msg.tokenHolderSupply,
        drandPublicKey: msg.drandPublicKey,
        drandPeriod: msg.drandPeriod,
        drandGenesisTime: msg.drandGenesisTime,
        validatorMinAmountToAllowClaim: msg.validatorMinAmountToAllowClaim,
        delegatorMinAmountInDelegation: msg.delegatorMinAmountInDelegation,
        combinationLen: msg.combinationLen,
        jackpotReward: msg.jackpotReward,
        jackpotPercentageReward: msg.jackpotPercentageReward,
        tokenHolderPercentageFeeReward: msg.tokenHolderPercentageFeeReward,
        feeForDrandWorkerInPercentage: msg.feeForDrandWorkerInPercentage,
        prizeRankWinnerPercentage: msg.prizeRankWinnerPercentage,
        pollCount: 0,
        holdersMaxPercentageReward: 20,
        workerDrandMaxPercentageReward: 10,
        pollEndHeight: msg.pollEndHeight,
    };
    config(deps.storage).save(&state)?;
    Ok(InitResponse::default())
}

// And declare a custom Error variant for the ones where you will want to make use of it
pub fn handle(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: HandleMsg,
) -> Result<HandleResponse, ContractError> {
    match msg {
        HandleMsg::Register { combination } => handle_register(deps, _env, info, combination),
        HandleMsg::Play {
            round,
            previous_signature,
            signature,
        } => handle_play(deps, _env, info, round, previous_signature, signature),
        HandleMsg::Ticket {} => handle_ticket(deps, _env, info),
        HandleMsg::Ico {} => handle_ico(deps, _env, info),
        HandleMsg::Buy {} => handle_buy(deps, _env, info),
        HandleMsg::Reward {} => handle_reward(deps, _env, info),
        HandleMsg::Jackpot {} => handle_jackpot(deps, _env, info),
        HandleMsg::Proposal {
            description,
            proposal,
            amount,
            prizePerRank,
        } => handle_proposal(
            deps,
            _env,
            info,
            description,
            proposal,
            amount,
            prizePerRank,
        ),
        HandleMsg::Vote { pollId, approve } => handle_vote(deps, _env, info, pollId, approve),
        HandleMsg::PresentProposal { pollId } => handle_present_proposal(deps, _env, info, pollId),
        HandleMsg::RejectProposal { pollId } => handle_reject_proposal(deps, _env, info, pollId),
    }
}

pub fn handle_register(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    combination: String,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let state = config(deps.storage).load()?;

    // Regex to check if the combination is allowed
    let regexBuild = format!(r"\b[a-f0-9]{{{}}}\b", state.combinationLen);
    let re = Regex::new(regexBuild.as_str()).unwrap();
    if !re.is_match(&combination) {
        return Err(ContractError::CombinationNotAuthorized(
            state.combinationLen.to_string(),
        ));
    }

    // Check if some funds are sent
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomTicket {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(
                    state.denomTicket.clone().to_string(),
                ))
            }
        }
        _ => Err(ContractError::ExtraDenom(
            state.denomTicket.clone().to_string(),
        )),
    }?;
    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    }
    /*
       TODO: Check if sent is 1 ticket
    */

    // Check if the lottery is about to play and cancel new ticket to enter until play
    if _env.block.time >= state.blockTimePlay {
        return Err(ContractError::LotteryAboutToStart {});
    }

    // Save combination and addresses to the bucket
    let keyExist = combination_storage(deps.storage).load(&combination.as_bytes());
    if keyExist.is_ok() {
        let mut combinationStorage = keyExist.unwrap();
        combinationStorage
            .addresses
            .push(deps.api.canonical_address(&info.sender)?);
        combination_storage(deps.storage).save(&combination.as_bytes(), &combinationStorage)?;
    } else {
        combination_storage(deps.storage).save(
            &combination.as_bytes(),
            &Combination {
                addresses: vec![deps.api.canonical_address(&info.sender)?],
            },
        )?;
    }

    Ok(HandleResponse {
        messages: vec![],
        attributes: vec![attr("action", "register")],
        data: None,
    })
}

pub fn handle_ticket(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // convert the sender to canonical address
    let sender = deps.api.canonical_address(&info.sender).unwrap();
    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Claim".to_string()));
    }

    if _env.block.height > state.blockClaim {
        state.claimTicket = vec![];
        state.blockClaim = _env.block.height + state.everyBlockHeight;
    }
    // Ensure sender is a delegator
    let mut delegator = deps.querier.query_all_delegations(&info.sender)?;
    if delegator.is_empty() {
        return Err(ContractError::NoDelegations {});
    }
    delegator.sort_by(|a, b| b.amount.amount.cmp(&a.amount.amount));

    // Ensure validator are owning 10000 upot min and the user stake the majority of his funds to this validator
    let validatorBalance = deps
        .querier
        .query_balance(&delegator[0].validator, &state.denomShare)
        .unwrap();
    if validatorBalance.amount.u128() < state.validatorMinAmountToAllowClaim.u128() {
        return Err(ContractError::ValidatorNotAuthorized(
            state.validatorMinAmountToAllowClaim.to_string(),
        ));
    }
    // Ensure is delegating the right denom
    if delegator[0].amount.denom != state.denomDelegation {
        return Err(ContractError::NoDelegations {});
    }
    // Ensure delegating the minimum admitted
    if delegator[0].amount.amount < state.delegatorMinAmountInDelegation {
        return Err(ContractError::DelegationTooLow(
            state.delegatorMinAmountInDelegation.to_string(),
        ));
    }
    // Ensure sender only can claim one time every x blocks
    if state
        .claimTicket
        .iter()
        .any(|address| deps.api.human_address(address).unwrap() == info.sender)
    {
        return Err(ContractError::AlreadyClaimed {});
    }
    // Add the sender to claimed state
    state.claimTicket.push(sender.clone());

    // Get the contract balance
    let balance = deps
        .querier
        .query_balance(_env.contract.address.clone(), &state.denomTicket)?;
    // Cancel if no amount in the contract
    if balance.amount.is_zero() {
        return Err(ContractError::EmptyBalance {});
    }

    // Save the new state
    config(deps.storage).save(&state)?;

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin {
            denom: state.denomTicket.clone(),
            amount: Uint128(1),
        }],
    };
    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "claim"), attr("to", &sender)],
        data: None,
    })
}

// Derives a 32 byte randomness from the beacon's signature
fn derive_randomness(signature: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(signature);
    hasher.finalize().into()
}

pub fn handle_play(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    round: u64,
    previous_signature: Binary,
    signature: Binary,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // reset holders reward
    state.holdersRewards = Uint128(0);
    // Load combinations
    let store = query_all_combination(deps.as_ref()).unwrap();

    /*
        Empty previous winner
    */
    // Get all keys in the bucket winner
    let keys = winner_storage(deps.storage)
        .range(None, None, Order::Ascending)
        .flat_map(|item| item.and_then(|(key, _)| Ok(key)))
        .collect::<Vec<Vec<u8>>>();
    // Empty winner for the next play
    for x in keys {
        winner_storage(deps.storage).remove(x.as_ref())
    }

    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Play".to_string()));
    }
    // Make the contract callable for everyone every x blocks
    if _env.block.time > state.blockTimePlay {
        // Update the state
        state.claimReward = vec![];
        state.blockTimePlay = _env.block.time + state.everyBlockTimePlay;
    } else {
        return Err(ContractError::Unauthorized {});
    }
    // Get the current round and check if it is a valid round.
    let fromGenesis = state.blockTimePlay - state.drandGenesisTime;
    let nextRound = (fromGenesis as f64 / state.drandPeriod as f64) + 1.0;

    if round != nextRound as u64 {
        return Err(ContractError::InvalidRound {});
    }
    let pk = g1_from_variable(&state.drandPublicKey).unwrap();
    let valid = verify(&pk, round, &previous_signature, &signature).unwrap_or(false);

    if !valid {
        return Err(ContractError::InvalidSignature {});
    }
    let randomness = derive_randomness(&signature);

    //save beacon for other users usage
    beacons_storage(deps.storage).set(&round.to_be_bytes(), &randomness);
    let randomnessHash = hex::encode(randomness);

    let n = randomnessHash
        .char_indices()
        .rev()
        .nth(state.combinationLen as usize - 1)
        .map(|(i, _)| i)
        .unwrap();
    let winningCombination = &randomnessHash[n..];

    // Set jackpot amount
    let balance = deps
        .querier
        .query_balance(&_env.contract.address, &state.denomDelegation)
        .unwrap();
    // Max amount winners can claim
    let jackpot = balance
        .amount
        .mul(Decimal::percent(state.jackpotPercentageReward as u64));
    // Drand worker fee
    let feeForDrandWorker =
        jackpot.mul(Decimal::percent(state.feeForDrandWorkerInPercentage as u64));
    // Amount token holders can claim of the reward is a fee
    let tokenHolderFeeReward = jackpot.mul(Decimal::percent(
        state.tokenHolderPercentageFeeReward as u64,
    ));
    // Total fees if winner of the jackpot
    let totalFee = jackpot.mul(Decimal::percent(
        (state.feeForDrandWorkerInPercentage as u64)
            + (state.tokenHolderPercentageFeeReward as u64),
    ));
    // The jackpot after worker fee applied
    let mut jackpotAfter = (jackpot - feeForDrandWorker).unwrap();

    if !store.combination.is_empty() {
        let mut count = 0;
        for combination in store.combination {
            for x in 0..winningCombination.len() {
                if combination.key.chars().nth(x).unwrap()
                    == winningCombination.chars().nth(x).unwrap()
                {
                    count += 1;
                }
            }

            if count == winningCombination.len() {
                // Set the reward for token holders
                state.holdersRewards = tokenHolderFeeReward;
                // Set the new jackpot after fee
                jackpotAfter = (jackpot - totalFee).unwrap();

                let mut dataWinner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    dataWinner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !dataWinner.is_empty() {
                    winner_storage(deps.storage).save(
                        &1_u8.to_be_bytes(),
                        &Winner {
                            winners: dataWinner,
                        },
                    )?;
                }
            } else if count == winningCombination.len() - 1 {
                let mut dataWinner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    dataWinner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !dataWinner.is_empty() {
                    winner_storage(deps.storage).save(
                        &2_u8.to_be_bytes(),
                        &Winner {
                            winners: dataWinner,
                        },
                    )?;
                }
            } else if count == winningCombination.len() - 2 {
                let mut dataWinner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    dataWinner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !dataWinner.is_empty() {
                    winner_storage(deps.storage).save(
                        &3_u8.to_be_bytes(),
                        &Winner {
                            winners: dataWinner,
                        },
                    )?;
                }
            } else if count == winningCombination.len() - 3 {
                let mut dataWinner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    dataWinner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !dataWinner.is_empty() {
                    winner_storage(deps.storage).save(
                        &4_u8.to_be_bytes(),
                        &Winner {
                            winners: dataWinner,
                        },
                    )?;
                }
            } else if count == winningCombination.len() - 4 {
                let mut dataWinner: Vec<WinnerInfoState> = vec![];
                for winnerAddress in combination.addresses {
                    dataWinner.push(WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    });
                }
                if !dataWinner.is_empty() {
                    winner_storage(deps.storage).save(
                        &5_u8.to_be_bytes(),
                        &Winner {
                            winners: dataWinner,
                        },
                    )?;
                }
            }
            // Re init the counter for the next players
            count = 0;
        }
    }

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin {
            denom: state.denomDelegation.clone(),
            amount: feeForDrandWorker,
        }],
    };
    // Update the state
    state.jackpotReward = jackpotAfter;

    // Get all keys in the bucket combination
    let keys = combination_storage(deps.storage)
        .range(None, None, Order::Ascending)
        .flat_map(|item| item.and_then(|(key, _)| Ok(key)))
        .collect::<Vec<Vec<u8>>>();
    // Empty combination for the next play
    for x in keys {
        combination_storage(deps.storage).remove(x.as_ref())
    }

    // Save the new state
    config(deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "reward"), attr("to", &info.sender)],
        data: None,
    })
}

pub fn handle_ico(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // Ico expire after blocktime
    if state.blockIcoTimeframe < _env.block.height {
        return Err(ContractError::TheIcoIsEnded {});
    }
    // Check if some funds are sent
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomDelegation {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denomDelegation.clone()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denomDelegation.clone())),
    }?;

    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    };
    // Get the contract balance prepare the tx
    let balance = deps
        .querier
        .query_balance(&_env.contract.address, &state.denomShare)
        .unwrap();
    if balance.amount.is_zero() {
        return Err(ContractError::EmptyBalance {});
    }

    if balance.amount.u128() < sent.u128() {
        return Err(ContractError::EmptyBalance {});
    }

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin {
            denom: state.denomShare.clone(),
            amount: sent,
        }],
    };

    state.tokenHolderSupply += sent;
    // Save the new state
    config(deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "ico"), attr("to", &info.sender)],
        data: None,
    })
}

pub fn handle_buy(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let state = config(deps.storage).load()?;

    // Get the funds
    let sent = match info.sent_funds.len() {
        0 => Err(ContractError::NoFunds {}),
        1 => {
            if info.sent_funds[0].denom == state.denomDelegation {
                Ok(info.sent_funds[0].amount)
            } else {
                Err(ContractError::MissingDenom(state.denomDelegation.clone()))
            }
        }
        _ => Err(ContractError::ExtraDenom(state.denomDelegation.clone())),
    }?;

    if sent.is_zero() {
        return Err(ContractError::NoFunds {});
    };

    let balance = deps
        .querier
        .query_balance(&_env.contract.address, &state.denomTicket)
        .unwrap();
    if balance.amount.is_zero() {
        return Err(ContractError::EmptyBalance {});
    }

    if balance.amount.u128() < sent.u128() {
        return Err(ContractError::EmptyBalance {});
    }

    let amountToSend = sent.u128() / state.denomDelegationDecimal.u128();

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: info.sender.clone(),
        amount: vec![Coin {
            denom: state.denomTicket.clone(),
            amount: Uint128(amountToSend),
        }],
    };

    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "ticket"), attr("to", &info.sender)],
        data: None,
    })
}

pub fn handle_reward(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<HandleResponse, ContractError> {
    // Load the state
    let mut state = config(deps.storage).load()?;
    // convert the sender to canonical address
    let sender = deps.api.canonical_address(&info.sender).unwrap();
    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Reward".to_string()));
    }
    if state.tokenHolderSupply.is_zero() {
        return Err(ContractError::Unauthorized {});
    }

    // Ensure sender have some reward tokens
    let balanceSender = deps
        .querier
        .query_balance(info.sender.clone(), &state.denomShare)
        .unwrap();
    if balanceSender.amount.is_zero() {
        return Err(ContractError::Unauthorized {});
    }

    // Ensure sender only can claim one time every x blocks
    if state
        .claimReward
        .iter()
        .any(|address| deps.api.human_address(address).unwrap() == info.sender.clone())
    {
        return Err(ContractError::AlreadyClaimed {});
    }
    // Add the sender to claimed state
    state.claimReward.push(sender.clone());

    // Get the contract balance
    let balanceContract = deps
        .querier
        .query_balance(_env.contract.address.clone(), &state.denomDelegation)?;
    // Cancel if no amount in the contract
    if balanceContract.amount.is_zero() {
        return Err(ContractError::EmptyBalance {});
    }
    // Get the percentage of shareholder
    let shareHolderPercentage =
        balanceSender.amount.u128() as u64 * 100 / state.tokenHolderSupply.u128() as u64;
    if shareHolderPercentage == 0 {
        return Err(ContractError::SharesTooLow {});
    }
    // Calculate the reward
    let reward = state
        .holdersRewards
        .mul(Decimal::percent(shareHolderPercentage));
    // Update the holdersReward
    state.holdersRewards = state.holdersRewards.sub(reward).unwrap();
    // Save the new state
    config(deps.storage).save(&state)?;

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps.api.human_address(&sender).unwrap(),
        amount: vec![Coin {
            denom: state.denomDelegation.clone(),
            amount: reward,
        }],
    };
    // Send the claimed tickets
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![attr("action", "reward"), attr("to", &sender)],
        data: None,
    })
}

fn remove_from_storage(
    deps: &DepsMut,
    info: &MessageInfo,
    winner: &WinnerInfo,
) -> Vec<WinnerInfoState> {
    // Update to claimed
    let mut newAddress = winner.winners.clone();
    for x in 0..newAddress.len() {
        if newAddress[x].address == deps.api.canonical_address(&info.sender).unwrap() {
            newAddress[x].claimed = true;
        }
    }

    return newAddress;
}

// Players claim the jackpot
pub fn handle_jackpot(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<HandleResponse, ContractError> {
    // Load state
    let state = config(deps.storage).load()?;
    // Load winners
    let store = query_all_winner(deps.as_ref()).unwrap();
    // Ensure the sender is not sending funds
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Jackpot".to_string()));
    }
    // Ensure there is jackpot reward to claim
    if state.jackpotReward.is_zero() {
        return Err(ContractError::Unauthorized {});
    }
    // Ensure there is some winner
    if store.winner.is_empty() {
        return Err(ContractError::NoWinners {});
    }

    let mut jackpotAmount: Uint128 = Uint128(0);
    let mut ticketWinning: Uint128 = Uint128(0);
    for winner in store.winner.clone() {
        for winnerInfo in winner.winners.clone() {
            if winnerInfo.address == deps.api.canonical_address(&info.sender).unwrap() {
                if winnerInfo.claimed {
                    return Err(ContractError::AlreadyClaimed {});
                }

                match winner.rank {
                    1 => {
                        // Prizes first rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[0] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpotAmount += Uint128(prize);
                        // Remove the address from the array and save
                        let newAddresses = remove_from_storage(&deps, &info, &winner);
                        winner_storage(deps.storage).save(
                            &1_u8.to_be_bytes(),
                            &Winner {
                                winners: newAddresses,
                            },
                        )?;
                    }
                    2 => {
                        // Prizes second rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[1] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpotAmount += Uint128(prize);
                        // Remove the address from the array and save
                        let newAddresses = remove_from_storage(&deps, &info, &winner);
                        winner_storage(deps.storage).save(
                            &2_u8.to_be_bytes(),
                            &Winner {
                                winners: newAddresses,
                            },
                        )?;
                    }
                    3 => {
                        // Prizes third rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[2] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpotAmount += Uint128(prize);
                        // Remove the address from the array and save
                        let newAddresses = remove_from_storage(&deps, &info, &winner);
                        winner_storage(deps.storage).save(
                            &3_u8.to_be_bytes(),
                            &Winner {
                                winners: newAddresses,
                            },
                        )?;
                    }
                    4 => {
                        // Prizes four rank
                        let prize = state
                            .jackpotReward
                            .mul(Decimal::percent(state.prizeRankWinnerPercentage[3] as u64))
                            .u128()
                            / winner.winners.clone().len() as u128;
                        jackpotAmount += Uint128(prize);
                        // Remove the address from the array and save
                        let newAddresses = remove_from_storage(&deps, &info, &winner);
                        winner_storage(deps.storage).save(
                            &4_u8.to_be_bytes(),
                            &Winner {
                                winners: newAddresses,
                            },
                        )?;
                    }
                    5 => {
                        // Prizes five rank
                        ticketWinning += Uint128(1);
                        // Remove the address from the array and save
                        let newAddresses = remove_from_storage(&deps, &info, &winner);
                        winner_storage(deps.storage).save(
                            &5_u8.to_be_bytes(),
                            &Winner {
                                winners: newAddresses,
                            },
                        )?;
                    }
                    _ => (),
                }
            }
        }
    }

    // Build the amount transaction
    let mut amountToSend: Vec<Coin> = vec![];

    if !jackpotAmount.is_zero() {
        // Get the contract balance
        let balance = deps
            .querier
            .query_balance(&_env.contract.address, &state.denomDelegation)
            .unwrap();
        // Ensure the contract have the balance
        if balance.amount.is_zero() {
            return Err(ContractError::EmptyBalance {});
        }
        // Ensure the contract have sufficient balance to handle the transaction
        if balance.amount < jackpotAmount {
            return Err(ContractError::NoFunds {});
        }
        amountToSend.push(Coin {
            denom: state.denomDelegation.clone(),
            amount: jackpotAmount,
        });
    }

    if !ticketWinning.is_zero() {
        // Get the contract ticket balance
        let ticketBalance = deps
            .querier
            .query_balance(&_env.contract.address, &state.denomTicket)
            .unwrap();
        // Ensure the contract have the balance
        if !ticketBalance.amount.is_zero() || ticketBalance.amount > ticketWinning {
            amountToSend.push(Coin {
                denom: state.denomTicket.clone(),
                amount: ticketWinning,
            });
        }
    }

    // Check if no amount to send return ok
    if amountToSend.is_empty() {
        return Ok(HandleResponse {
            messages: vec![],
            attributes: vec![
                attr("action", "jackpot reward"),
                attr("to", &info.sender),
                attr("jackpot_prize", "no"),
            ],
            data: None,
        });
    }

    let msg = BankMsg::Send {
        from_address: _env.contract.address.clone(),
        to_address: deps
            .api
            .human_address(&deps.api.canonical_address(&info.sender).unwrap())
            .unwrap(),
        amount: amountToSend,
    };

    // Send the jackpot
    Ok(HandleResponse {
        messages: vec![msg.into()],
        attributes: vec![
            attr("action", "jackpot reward"),
            attr("to", &info.sender),
            attr("jackpot_prize", "yes"),
        ],
        data: None,
    })
}

pub fn handle_proposal(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    description: String,
    proposal: Proposal,
    amount: Option<Uint128>,
    prizePerRank: Option<Vec<u8>>,
) -> Result<HandleResponse, ContractError> {
    let mut state = config(deps.storage).load().unwrap();
    // Increment and get the new poll id for bucket key
    let pollId = state.pollCount + 1;
    // Set the new counter
    state.pollCount = pollId;

    //Handle sender is not sending funds
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Proposal".to_string()));
    }

    // Handle the description is respecting length
    if (description.len() as u64) < MIN_DESC_LEN {
        return Err(ContractError::DescriptionTooShort(MIN_DESC_LEN.to_string()));
    } else if (description.len() as u64) > MAX_DESC_LEN {
        return Err(ContractError::DescriptionTooLong(MAX_DESC_LEN.to_string()));
    }

    let mut proposalAmount: Uint128 = Uint128::zero();
    let mut proposalPrizeRank: Vec<u8> = vec![];

    let proposalType = if let Proposal::HolderFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.holdersMaxPercentageReward {
                    return Err(ContractError::ParamRequiredForThisProposal(
                        "HolderFeePercentage amount between 0 to 100".to_string(),
                    ));
                }
                proposalAmount = percentage;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "HolderFeePercentage amount".to_string(),
                ));
            }
        }

        Proposal::HolderFeePercentage
    } else if let Proposal::DrandWorkerFeePercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > state.workerDrandMaxPercentageReward {
                    return Err(ContractError::ParamRequiredForThisProposal(
                        "DrandWorkerFeePercentage amount between 0 to 100".to_string(),
                    ));
                }
                proposalAmount = percentage;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "DrandWorkerFeePercentage amount".to_string(),
                ));
            }
        }

        Proposal::DrandWorkerFeePercentage
    } else if let Proposal::JackpotRewardPercentage = proposal {
        match amount {
            Some(percentage) => {
                if percentage.u128() as u8 > 100 {
                    return Err(ContractError::ParamRequiredForThisProposal(
                        "JackpotRewardPercentage amount between 0 to 100".to_string(),
                    ));
                }
                proposalAmount = percentage;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "JackpotRewardPercentage amount".to_string(),
                ));
            }
        }

        Proposal::JackpotRewardPercentage
    } else if let Proposal::LotteryEveryBlockTime = proposal {
        match amount {
            Some(blockTime) => {
                proposalAmount = blockTime;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "LotteryEveryBlockTime amount".to_string(),
                ));
            }
        }

        Proposal::LotteryEveryBlockTime
    } else if let Proposal::MinAmountDelegator = proposal {
        match amount {
            Some(minAmount) => {
                proposalAmount = minAmount;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "MinAmountDelegator amount".to_string(),
                ));
            }
        }

        Proposal::MinAmountDelegator
    } else if let Proposal::MinAmountValidator = proposal {
        match amount {
            Some(minAmount) => {
                proposalAmount = minAmount;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "MinAmountValidator amount".to_string(),
                ));
            }
        }
        Proposal::MinAmountValidator
    } else if let Proposal::PrizePerRank = proposal {
        match prizePerRank {
            Some(ranks) => {
                if ranks.len() != 4 {
                    return Err(ContractError::ParamRequiredForThisProposal(
                        "PrizePerRank 4 separated numbers between 0 to 100".to_string(),
                    ));
                }
                let mut totalPercentage = 0;
                for rank in ranks.clone() {
                    if (rank as u8) > 100 {
                        return Err(ContractError::ParamRequiredForThisProposal(
                            "PrizePerRank numbers between 0 to 100".to_string(),
                        ));
                    }
                    totalPercentage += rank;
                }
                // Ensure the repartition sum is 100%
                if totalPercentage != 100 {
                    return Err(ContractError::ParamRequiredForThisProposal(
                        "PrizePerRank numbers sum need to be 100".to_string(),
                    ));
                }

                proposalPrizeRank = ranks;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "PrizePerRank".to_string(),
                ));
            }
        }
        Proposal::PrizePerRank
    } else if let Proposal::ClaimEveryBlock = proposal {
        match amount {
            Some(blockTime) => {
                proposalAmount = blockTime;
            }
            None => {
                return Err(ContractError::ParamRequiredForThisProposal(
                    "ClaimEveryBlock amount".to_string(),
                ));
            }
        }
        Proposal::ClaimEveryBlock
    } else {
        return Err(ContractError::ProposalNotFound {});
    };

    let senderToCanonical = deps.api.canonical_address(&info.sender).unwrap();

    let newPoll = PollInfoState {
        creator: senderToCanonical,
        status: PollStatus::InProgress,
        end_height: _env.block.height + state.pollEndHeight,
        start_height: _env.block.height,
        description,
        yes_voters: vec![],
        no_voters: vec![],
        amount: proposalAmount,
        prizeRank: proposalPrizeRank,
        proposal: proposalType,
    };

    // Save poll
    poll_storage(deps.storage).save(&state.pollCount.to_be_bytes(), &newPoll)?;

    // Save state
    config(deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![],
        attributes: vec![
            attr("action", "create a proposal"),
            attr("proposal_id", pollId.to_string()),
            attr("proposal_creator", &info.sender.to_string()),
            attr("proposal_creation_result", "success"),
        ],
        data: None,
    })
}

pub fn handle_vote(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    pollId: u64,
    approve: bool,
) -> Result<HandleResponse, ContractError> {
    let store = poll_storage(deps.storage).load(&pollId.to_be_bytes())?;
    let sender = deps.api.canonical_address(&info.sender).unwrap();

    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("Vote".to_string()));
    }
    // Ensure the poll is still valid
    if _env.block.height > store.end_height {
        return Err(ContractError::ProposalExpired {});
    }
    // Ensure the voter can't vote more times
    if store.yes_voters.contains(&sender) || store.no_voters.contains(&sender) {
        return Err(ContractError::AlreadyVoted {});
    }

    match approve {
        true => {
            poll_storage(deps.storage).update::<_, StdError>(&pollId.to_be_bytes(), |poll| {
                let mut pollData = poll.unwrap();
                pollData.yes_voters.push(sender.clone());
                Ok(pollData)
            })?;
        }
        false => {
            poll_storage(deps.storage).update::<_, StdError>(&pollId.to_be_bytes(), |poll| {
                let mut pollData = poll.unwrap();
                pollData.no_voters.push(sender.clone());
                Ok(pollData)
            })?;
        }
    }

    Ok(HandleResponse {
        messages: vec![],
        attributes: vec![
            attr("action", "vote"),
            attr("proposalId", pollId.to_string()),
            attr("voting_result", "success"),
        ],
        data: None,
    })
}

pub fn handle_reject_proposal(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    pollId: u64,
) -> Result<HandleResponse, ContractError> {
    let store = poll_storage_read(deps.storage).load(&pollId.to_be_bytes())?;
    let sender = deps.api.canonical_address(&info.sender).unwrap();

    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("RejectProposal".to_string()));
    }
    // Ensure end proposal height is not expired
    if store.end_height < _env.block.height {
        return Err(ContractError::ProposalExpired {});
    }
    // Ensure only the creator can reject a proposal OR the status of the proposal is still in progress
    if store.creator != sender || store.status != PollStatus::InProgress {
        return Err(ContractError::Unauthorized {});
    }

    poll_storage(deps.storage).update::<_, StdError>(&pollId.to_be_bytes(), |poll| {
        let mut pollData = poll.unwrap();
        // Update the status to rejected by the creator
        pollData.status = PollStatus::RejectedByCreator;
        // Update the end eight to now
        pollData.end_height = _env.block.height;
        Ok(pollData)
    })?;

    Ok(HandleResponse {
        messages: vec![],
        attributes: vec![
            attr("action", "creator reject the proposal"),
            attr("proposalId", pollId.to_string()),
        ],
        data: None,
    })
}

fn total_weight(deps: &DepsMut, state: &State, addresses: &Vec<CanonicalAddr>) -> Uint128 {
    let mut weight = Uint128::zero();
    for address in addresses {
        let humanAddress = deps.api.human_address(&address).unwrap();

        let balance = deps
            .querier
            .query_balance(humanAddress, &state.denomShare)
            .unwrap();

        if !balance.amount.is_zero() {
            weight += balance.amount;
        }
    }
    weight
}

pub fn handle_present_proposal(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    pollId: u64,
) -> Result<HandleResponse, ContractError> {
    // Load storage
    let mut state = config(deps.storage).load().unwrap();
    let store = poll_storage_read(deps.storage)
        .load(&pollId.to_be_bytes())
        .unwrap();

    // Ensure the sender not sending funds accidentally
    if !info.sent_funds.is_empty() {
        return Err(ContractError::DoNotSendFunds("PresentProposal".to_string()));
    }
    // Ensure the proposal is still in Progress
    if store.status != PollStatus::InProgress {
        return Err(ContractError::Unauthorized {});
    }
    // Ensure the proposal is ended
    if store.end_height > _env.block.height {
        return Err(ContractError::ProposalNotExpired {});
    }
    println!("{:?}", store);
    // Calculating the weight
    let yesWeight = total_weight(&deps, &state, &store.yes_voters);
    // let noWeight = total_weight(&deps, &state, &store.no_voters);
    println!("{}", yesWeight.u128());

    // Get the amount
    let mut finalVoteWeightInPercentage: u128 = 0;
    if !yesWeight.is_zero() {
        let yesWeightByHundred = yesWeight.u128() * 100;
        finalVoteWeightInPercentage = yesWeightByHundred / state.tokenHolderSupply.u128();
    }

    // Reject the proposal
    if
    /*noWeight.u128() >= yesWeight.u128() ||*/
    finalVoteWeightInPercentage < 60 {
        poll_storage(deps.storage).update::<_, StdError>(&pollId.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the status to rejected
            pollData.status = PollStatus::Rejected;
            Ok(pollData)
        })?;
        return Ok(HandleResponse {
            messages: vec![],
            attributes: vec![
                attr("action", "present the proposal"),
                attr("proposal_id", pollId.to_string()),
                attr("proposal_result", "rejected"),
            ],
            data: None,
        });
    };

    // Valid the proposal
    match store.proposal {
        Proposal::ClaimEveryBlock => {
            state.blockClaim = store.amount.u128() as u64;
        }
        Proposal::LotteryEveryBlockTime => {
            state.everyBlockTimePlay = store.amount.u128() as u64;
        }
        Proposal::DrandWorkerFeePercentage => {
            state.feeForDrandWorkerInPercentage = store.amount.u128() as u8;
        }
        Proposal::JackpotRewardPercentage => {
            state.jackpotPercentageReward = store.amount.u128() as u8;
        }
        Proposal::MinAmountDelegator => {
            state.delegatorMinAmountInDelegation = store.amount.clone();
        }
        Proposal::PrizePerRank => {
            state.prizeRankWinnerPercentage = store.prizeRank;
        }
        Proposal::MinAmountValidator => {
            state.validatorMinAmountToAllowClaim = store.amount.clone();
        }
        Proposal::HolderFeePercentage => {
            state.holdersMaxPercentageReward = store.amount.u128() as u8
        }
        _ => {
            return Err(ContractError::ProposalNotFound {});
        }
    }

    // Save to storage
    poll_storage(deps.storage).update::<_, StdError>(&pollId.to_be_bytes(), |poll| {
        let mut pollData = poll.unwrap();
        // Update the status to passed
        pollData.status = PollStatus::Passed;
        Ok(pollData)
    })?;

    config(deps.storage).save(&state)?;

    Ok(HandleResponse {
        messages: vec![],
        attributes: vec![
            attr("action", "present the proposal"),
            attr("proposal_id", pollId.to_string()),
            attr("proposal_result", "approved"),
        ],
        data: None,
    })
}

pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    let response = match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?)?,
        QueryMsg::LatestDrand {} => to_binary(&query_latest(deps)?)?,
        QueryMsg::GetRandomness { round } => to_binary(&query_get(deps, round)?)?,
        QueryMsg::Combination {} => to_binary(&query_all_combination(deps)?)?,
        QueryMsg::Winner {} => to_binary(&query_all_winner(deps)?)?,
        QueryMsg::GetPoll { pollId } => to_binary(&query_poll(deps, pollId)?)?,
        QueryMsg::GetRound {} => to_binary(&query_round(deps)?)?,
    };
    Ok(response)
}

fn query_config(deps: Deps) -> Result<ConfigResponse, ContractError> {
    let state = config_read(deps.storage).load()?;
    Ok(state)
}
// Query beacon by round
fn query_get(deps: Deps, round: u64) -> Result<GetResponse, ContractError> {
    let beacons = beacons_storage_read(deps.storage);
    let randomness = beacons.get(&round.to_be_bytes()).unwrap_or_default();
    Ok(GetResponse {
        randomness: randomness.into(),
    })
}
// Query latest beacon
fn query_latest(deps: Deps) -> Result<LatestResponse, ContractError> {
    let store = beacons_storage_read(deps.storage);
    let mut iter = store.range(None, None, Order::Descending);
    let (key, value) = iter.next().ok_or(ContractError::NoBeacon {})?;
    Ok(LatestResponse {
        round: u64::from_be_bytes(Binary(key).to_array()?),
        randomness: value.into(),
    })
}
fn query_all_combination(deps: Deps) -> Result<AllCombinationResponse, ContractError> {
    let combinations = combination_storage_read(deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item| {
            item.and_then(|(k, combination)| {
                Ok(CombinationInfo {
                    key: String::from_utf8(k)?,
                    addresses: combination.addresses,
                })
            })
        })
        .collect();

    Ok(AllCombinationResponse {
        combination: combinations,
    })
}

fn query_all_winner(deps: Deps) -> Result<AllWinnerResponse, ContractError> {
    let winners = winner_storage_read(deps.storage)
        .range(None, None, Order::Descending)
        .flat_map(|item| {
            item.and_then(|(k, winner)| {
                Ok(WinnerInfo {
                    rank: u8::from_be_bytes(Binary(k).to_array()?),
                    winners: winner.winners,
                })
            })
        })
        .collect();
    Ok(AllWinnerResponse { winner: winners })
}

fn query_poll(deps: Deps, pollId: u64) -> Result<GetPollResponse, ContractError> {
    let store = poll_storage_read(deps.storage);

    let poll = match store.may_load(&pollId.to_be_bytes())? {
        Some(poll) => Some(poll),
        None => return Err(ContractError::ProposalNotFound {}),
    }
    .unwrap();

    Ok(GetPollResponse {
        creator: deps.api.human_address(&poll.creator).unwrap(),
        status: poll.status,
        end_height: poll.end_height,
        start_height: poll.start_height,
        description: poll.description,
        amount: poll.amount,
        prizePerRank: poll.prizeRank,
    })
}

fn query_round(deps: Deps) -> Result<RoundResponse, ContractError> {
    let state = config_read(deps.storage).load()?;
    let fromGenesis = state.blockTimePlay - state.drandGenesisTime;
    let nextRound = (fromGenesis as f64 / state.drandPeriod as f64) + 1.0;
    let nextRoundFormat = nextRound as u64;

    Ok(RoundResponse {
        nextRound: nextRoundFormat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{HandleMsg, InitMsg, QueryMsg};
    use cosmwasm_std::testing::{
        mock_dependencies, mock_env, mock_info, BankQuerier, MockApi, MockQuerier, MockStorage,
        MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Api, Attribute, Binary, CanonicalAddr, CosmosMsg, Decimal,
        FullDelegation, HumanAddr, MessageInfo, OwnedDeps, QuerierResult, StdError::NotFound,
        Storage, Uint128, Validator,
    };
    use cosmwasm_storage::{bucket, bucket_read, singleton, singleton_read};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use std::borrow::Borrow;
    use std::collections::HashMap;
    // DRAND
    use crate::error::ContractError::Std;
    use drand_verify::{g1_from_fixed, g1_from_variable, verify};
    use hex_literal::hex;
    use sha2::{Digest, Sha256};

    fn default_init(deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>) {
        const DENOM_TICKET: &str = "ujack";
        const DENOM_DELEGATION: &str = "uscrt";
        const DENOM_DELEGATION_DECIMAL: Uint128 = Uint128(1_000_000);
        const DENOM_SHARE: &str = "upot";
        const EVERY_BLOCK_EIGHT: u64 = 100;
        const CLAIM_TICKET: Vec<CanonicalAddr> = vec![];
        const CLAIM_REWARD: Vec<CanonicalAddr> = vec![];
        const BLOCK_TIME_PLAY: u64 = 1610566920;
        const EVERY_BLOCK_TIME_PLAY: u64 = 50000;
        const BLOCK_CLAIM: u64 = 0;
        const EVERY_BLOCK_CLAIM: u64 = 50000;
        const BLOCK_ICO_TIME_FRAME: u64 = 1000000000;
        const HOLDERS_REWARDS: Uint128 = Uint128(5_221);
        const TOKEN_HOLDER_SUPPLY: Uint128 = Uint128(10_000_000);
        let PUBLIC_KEY: Binary = vec![
            134, 143, 0, 94, 184, 230, 228, 202, 10, 71, 200, 167, 124, 234, 165, 48, 154, 71, 151,
            138, 124, 113, 188, 92, 206, 150, 54, 107, 93, 122, 86, 153, 55, 197, 41, 238, 218,
            102, 199, 41, 55, 132, 169, 64, 40, 1, 175, 49,
        ]
        .into(); //Binary::from(hex!("868f005eb8e6e4ca0a47c8a77ceaa5309a47978a7c71bc5cce96366b5d7a569937c529eeda66c7293784a9402801af31"));
        const GENESIS_TIME: u64 = 1595431050;
        const PERIOD: u64 = 30;
        const VALIDATOR_MIN_AMOUNT_TO_ALLOW_CLAIM: Uint128 = Uint128(10_000);
        const DELEGATOR_MIN_AMOUNT_IN_DELEGATION: Uint128 = Uint128(10_000);
        const COMBINATION_LEN: u8 = 6;
        const JACKPOT_REWARD: Uint128 = Uint128(8_000_000);
        const JACKPOT_PERCENTAGE_REWARD: u8 = 80;
        const TOKEN_HOLDER_PERCENTAGE_FEE_REWARD: u8 = 10;
        const FEE_FOR_DRAND_WORKER_IN_PERCENTAGE: u8 = 1;
        let PRIZE_RANK_WINNER_PERCENTAGE: Vec<u8> = vec![84, 10, 5, 1];
        const POLL_END_HEIGHT: u64 = 40_000;

        fn pubkey() -> Binary {
            vec![
                134, 143, 0, 94, 184, 230, 228, 202, 10, 71, 200, 167, 124, 234, 165, 48, 154, 71,
                151, 138, 124, 113, 188, 92, 206, 150, 54, 107, 93, 122, 86, 153, 55, 197, 41, 238,
                218, 102, 199, 41, 55, 132, 169, 64, 40, 1, 175, 49,
            ]
            .into()
        }

        let init_msg = InitMsg {
            denomTicket: DENOM_TICKET.to_string(),
            denomDelegation: DENOM_DELEGATION.to_string(),
            denomDelegationDecimal: DENOM_DELEGATION_DECIMAL,
            denomShare: DENOM_SHARE.to_string(),
            everyBlockHeight: EVERY_BLOCK_EIGHT,
            claimTicket: CLAIM_TICKET,
            claimReward: CLAIM_REWARD,
            blockTimePlay: BLOCK_TIME_PLAY,
            everyBlockTimePlay: EVERY_BLOCK_TIME_PLAY,
            blockClaim: EVERY_BLOCK_CLAIM,
            blockIcoTimeframe: BLOCK_ICO_TIME_FRAME,
            holdersRewards: HOLDERS_REWARDS,
            tokenHolderSupply: TOKEN_HOLDER_SUPPLY,
            drandPublicKey: PUBLIC_KEY,
            drandPeriod: PERIOD,
            drandGenesisTime: GENESIS_TIME,
            validatorMinAmountToAllowClaim: VALIDATOR_MIN_AMOUNT_TO_ALLOW_CLAIM,
            delegatorMinAmountInDelegation: DELEGATOR_MIN_AMOUNT_IN_DELEGATION,
            combinationLen: COMBINATION_LEN,
            jackpotReward: JACKPOT_REWARD,
            jackpotPercentageReward: JACKPOT_PERCENTAGE_REWARD,
            tokenHolderPercentageFeeReward: TOKEN_HOLDER_PERCENTAGE_FEE_REWARD,
            feeForDrandWorkerInPercentage: FEE_FOR_DRAND_WORKER_IN_PERCENTAGE,
            prizeRankWinnerPercentage: PRIZE_RANK_WINNER_PERCENTAGE,
            pollEndHeight: POLL_END_HEIGHT,
        };
        let info = mock_info(HumanAddr::from("owner"), &[]);
        init(deps.as_mut(), mock_env(), info, init_msg).unwrap();
    }
    /// Derives a 32 byte randomness from the beacon's signature
    fn derive_randomness(signature: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(signature);
        hasher.finalize().into()
    }

    #[test]
    fn proper_init() {
        let mut deps = mock_dependencies(&[Coin {
            denom: "uscrt".to_string(),
            amount: Uint128(100_000_000),
        }]);
        default_init(&mut deps);
        println!("{:?}", mock_env())
    }
    #[test]
    fn get_round_play() {
        let mut deps = mock_dependencies(&[Coin {
            denom: "uscrt".to_string(),
            amount: Uint128(100_000_000),
        }]);
        default_init(&mut deps);
        let res = query_round(deps.as_ref()).unwrap();
        println!("{:?}", res.nextRound);
    }
    #[test]
    fn testing_saved_address_winner() {
        let mut deps = mock_dependencies(&[Coin {
            denom: "uscrt".to_string(),
            amount: Uint128(100_000_000),
        }]);
        default_init(&mut deps);

        let winnerAddress = deps
            .api
            .canonical_address(&HumanAddr::from("address".to_string()))
            .unwrap();
        let winnerAddress2 = deps
            .api
            .canonical_address(&HumanAddr::from("address2".to_string()))
            .unwrap();
        winner_storage(deps.as_mut().storage).save(
            &2_u8.to_be_bytes(),
            &Winner {
                winners: vec![
                    WinnerInfoState {
                        claimed: false,
                        address: winnerAddress,
                    },
                    WinnerInfoState {
                        claimed: true,
                        address: winnerAddress2,
                    },
                ],
            },
        );
        let res = query_all_winner(deps.as_ref()).unwrap();
        println!("{:?}", res);
    }
    /*#[test]
    fn random (){
        //println!("{}", 1432439234 % 100);
        let mut deps = mock_dependencies(&[Coin{ denom: "uscrt".to_string(), amount: Uint128(100_000_000)}]);
        default_init(&mut deps);

        let res = query_config(deps.as_ref()).unwrap();

        // DRAND
        let PK_LEO_MAINNET: [u8; 48] = hex!("868f005eb8e6e4ca0a47c8a77ceaa5309a47978a7c71bc5cce96366b5d7a569937c529eeda66c7293784a9402801af31");
        const GENESIS_TIME: u64 = 1595431050;
        const PERIOD: f64 = 30.0;

        //let pk = g1_from_fixed(PK_LEO_MAINNET).unwrap();
        let pk = g1_from_variable(&res.drandPublicKey).unwrap();
        let previous_signature = hex::decode("9491284489e453531c2429245dc6a9c4667f4cc2ab36295e7652de566d8ea2e16617690472d99b7d4604ecb8a8a249190e3a9c224bda3a9ea6c367c8ab6432c8f177838c20429e51fedcb8dacd5d9c7dc08b5147d6abbfc3db4b59d832290be2").unwrap();
        let signature = hex::decode("b1af60ff60d52b38ef13f8597df977c950997b562ec8bf31b765dedf3e138801a6582b53737b654d1df047c1786acd94143c9a02c173185dcea2fa2801223180d34130bf8c6566d26773296cdc9666fdbf095417bfce6ba90bb83929081abca3").unwrap();
        let round: u64 = 501672;
        let result = verify(&pk, round, &previous_signature, &signature).unwrap();
        let mut env = mock_env();
        env.block.time = 1610566920;
        let now = env.block.time;
        let fromGenesis = env.block.time - GENESIS_TIME;
        // let time = 1610566920 - 1595431050;
        let round = (fromGenesis as f64 / PERIOD) + 1.0;

        assert_eq!(result, true);
        println!("{:?}", pk);
        println!("{:?}", hex::encode(derive_randomness(&signature)));
        println!("{:?}", env);
        // println!("{}", time);
        println!("{}", round.floor());
        println!("{}", fromGenesis);
    }*/
    mod register {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn register_success() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);

            // Test if this succeed
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(1),
                }],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();
            assert_eq!(0, res.messages.len());
            // check if we can add multiple players
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator12"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(1),
                }],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();
            // Load all combination
            let res = query_all_combination(deps.as_ref()).unwrap();
            // Test if the address is saved success
            assert_eq!(2, res.combination[0].addresses.len());
        }
        #[test]
        fn try_register_without_ticket_result_error() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);
            // Test if this succeed
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(0),
                }],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
            match res {
                Err(ContractError::NoFunds {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sending_wrong_denom_ticket() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);
            // Test if this succeed
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(2),
                }],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
            match res {
                Err(ContractError::MissingDenom(msg)) => {
                    assert_eq!(msg, "ujack".to_string())
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn only_send_ticket_denom() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);
            // Test if this succeed
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[
                    Coin {
                        denom: "ujack".to_string(),
                        amount: Uint128(3),
                    },
                    Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(2),
                    },
                ],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
            match res {
                Err(ContractError::ExtraDenom(msg)) => {
                    assert_eq!(msg, "ujack".to_string())
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn ensure_cannot_play_lottery_when_about_to_start() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);
            // Test if this succeed
            let msg = HandleMsg::Register {
                combination: "1e3fab".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(2),
                }],
            );
            let mut env = mock_env();
            // If block time is superior to block time to play the lottery is about to start and we can't
            // admit new register until the lottery play cause the result can be leaked at every moment.
            env.block.time = 1610566930;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::LotteryAboutToStart {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn combination_not_authorized() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            default_init(&mut deps);
            // Test if we get error if combination is not authorized
            let msg = HandleMsg::Register {
                combination: "1e3fabgvcc".to_string(),
            };
            let info = mock_info(
                HumanAddr::from("delegator12"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(3),
                }],
            );
            let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
            match res {
                Err(ContractError::CombinationNotAuthorized(msg)) => {
                    assert_eq!("6", msg);
                }
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod claim_ticket {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        fn init_balance_and_staking_default(deps: &mut MockQuerier, balance: Uint128) {
            deps.update_balance(
                "validator2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: balance,
                }],
            );
            deps.update_staking(
                "uscrt",
                &[Validator {
                    address: HumanAddr::from("validator1"),
                    commission: Decimal::percent(10),
                    max_commission: Decimal::percent(100),
                    max_change_rate: Decimal::percent(100),
                }],
                &[
                    FullDelegation {
                        delegator: HumanAddr::from("delegator1"),
                        validator: HumanAddr::from("validator1"),
                        amount: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(1_000_000),
                        },
                        can_redelegate: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        },
                        accumulated_rewards: vec![Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        }],
                    },
                    FullDelegation {
                        delegator: HumanAddr::from("delegator1"),
                        validator: HumanAddr::from("validator2"),
                        amount: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(15_050_030),
                        },
                        can_redelegate: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        },
                        accumulated_rewards: vec![Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        }],
                    },
                    FullDelegation {
                        delegator: HumanAddr::from("delegator1"),
                        validator: HumanAddr::from("validator3"),
                        amount: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(2_004_000),
                        },
                        can_redelegate: Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        },
                        accumulated_rewards: vec![Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(0),
                        }],
                    },
                ],
            );
        }
        fn init_balance_and_staking_insufficiently(deps: &mut MockQuerier, balance: Uint128) {
            deps.update_balance(
                "validator2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: balance,
                }],
            );
            deps.update_staking(
                "uscrt",
                &[Validator {
                    address: HumanAddr::from("validator1"),
                    commission: Decimal::percent(10),
                    max_commission: Decimal::percent(100),
                    max_change_rate: Decimal::percent(100),
                }],
                &[FullDelegation {
                    delegator: HumanAddr::from("delegator1"),
                    validator: HumanAddr::from("validator2"),
                    amount: Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(1_050),
                    },
                    can_redelegate: Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(0),
                    },
                    accumulated_rewards: vec![Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(0),
                    }],
                }],
            );
        }

        fn init_balance_and_staking_the_incorrect_denom(deps: &mut MockQuerier, balance: Uint128) {
            deps.update_balance(
                "validator2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: balance,
                }],
            );
            deps.update_staking(
                "uscrt",
                &[Validator {
                    address: HumanAddr::from("validator1"),
                    commission: Decimal::percent(10),
                    max_commission: Decimal::percent(100),
                    max_change_rate: Decimal::percent(100),
                }],
                &[FullDelegation {
                    delegator: HumanAddr::from("delegator1"),
                    validator: HumanAddr::from("validator2"),
                    amount: Coin {
                        denom: "ucoin".to_string(),
                        amount: Uint128(10_050_000),
                    },
                    can_redelegate: Coin {
                        denom: "ucoin".to_string(),
                        amount: Uint128(0),
                    },
                    accumulated_rewards: vec![Coin {
                        denom: "ucoin".to_string(),
                        amount: Uint128(0),
                    }],
                }],
            );
        }

        #[test]
        fn handle_dot_not_send_funds() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            init_balance_and_staking_default(&mut deps.querier, Uint128(10_000));
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(3),
                }],
            );
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!(msg, "Claim".to_string())
                }
                _ => panic!("Unexpected error"),
            };
        }
        #[test]
        fn not_allowed_to_claim_because_not_staking() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::NoDelegations {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn error_no_ticket_funds_in_the_contract() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(100_000_000),
            }]);
            init_balance_and_staking_default(&mut deps.querier, Uint128(10_000));
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_validator_need_to_stake_min_amount() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            init_balance_and_staking_default(&mut deps.querier, Uint128(0));
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::ValidatorNotAuthorized(msg)) => {
                    assert_eq!(msg, "10000")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_delegator_need_to_stake_min_amount() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            init_balance_and_staking_insufficiently(&mut deps.querier, Uint128(10_000));
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::DelegationTooLow(msg)) => {
                    assert_eq!(msg, "10000")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_delegator_need_to_stake() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            init_balance_and_staking_the_incorrect_denom(&mut deps.querier, Uint128(10_000));
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::NoDelegations {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn claim_the_ticket() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(100_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(100_000_000),
                },
            ]);
            init_balance_and_staking_default(&mut deps.querier, Uint128(10_000));
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("delegator1"), &[]);
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("delegator1"),
                    amount: vec![Coin {
                        denom: "ujack".to_string(),
                        amount: Uint128(1)
                    }]
                })
            );
            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            let claimer = deps
                .api
                .canonical_address(&HumanAddr::from("delegator1"))
                .unwrap();
            // Test we added the claimer to the claimed vector
            assert!(res.claimTicket.len() > 0);
            // Test we added correct claimer to the claimed vector
            assert!(res.claimTicket.contains(&claimer));
            // Test error to claim two times
            let res = handle_ticket(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::AlreadyClaimed {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod public_sale {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn balance_contract_empty() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "upot".to_string(),
                amount: Uint128(0),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_insufficient_balance() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "upot".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(2_000_000),
                }],
            );
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sender_wrong_denom() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "upot".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "other".to_string(),
                    amount: Uint128(2_000_000),
                }],
            );
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::MissingDenom(msg)) => {
                    assert_eq!(msg, "uscrt")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn handle_sending_more_than_one_denom() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "upot".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[
                    Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(2_000_000),
                    },
                    Coin {
                        denom: "other".to_string(),
                        amount: Uint128(2_000_000),
                    },
                ],
            );
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::ExtraDenom(msg)) => {
                    assert_eq!(msg, "uscrt")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "upot".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(2_000_000),
                }],
            );
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("delegator1"),
                    amount: vec![Coin {
                        denom: "upot".to_string(),
                        amount: Uint128(2_000_000)
                    }]
                })
            );
            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            assert_ne!(0, res.tokenHolderSupply.u128());
            assert_eq!(12_000_000, res.tokenHolderSupply.u128());

            // Test if state have changed correctly
            let res = handle_ico(deps.as_mut(), mock_env(), info.clone()).unwrap();
            let res = query_config(deps.as_ref()).unwrap();
            // Test if tokenHolderSupply incremented correctly after multiple buys
            assert_eq!(14_000_000, res.tokenHolderSupply.u128());
        }
    }

    mod buy_ticket {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn contract_balance_empty() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "ujack".to_string(),
                amount: Uint128(0),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_insufficient_balance() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "ujack".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(2_000_000),
                }],
            );
            let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sender_wrong_denom() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "ujack".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "other".to_string(),
                    amount: Uint128(5_561_532),
                }],
            );
            let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::MissingDenom(msg)) => {
                    assert_eq!(msg, "uscrt")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn sender_multiple_denom() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "ujack".to_string(),
                amount: Uint128(1_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[
                    Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(2_000_000),
                    },
                    Coin {
                        denom: "other".to_string(),
                        amount: Uint128(2_000_000),
                    },
                ],
            );
            let res = handle_buy(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::ExtraDenom(msg)) => {
                    assert_eq!(msg, "uscrt")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "ujack".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("delegator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(5_961_532),
                }],
            );
            let res = handle_buy(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("delegator1"),
                    amount: vec![Coin {
                        denom: "ujack".to_string(),
                        amount: Uint128(5)
                    }]
                })
            );
        }
    }

    mod play {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        fn init_combination(deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>) {
            // Init the bucket with players to storage
            let combination = "950a51";
            let combination2 = "950a52";
            let combination3 = "950a90";
            let combination4 = "950f92";
            let combination5 = "954f92";
            let addresses1 = vec![
                deps.api
                    .canonical_address(&HumanAddr("address1".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap(),
            ];
            let addresses2 = vec![
                deps.api
                    .canonical_address(&HumanAddr("address2".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address3".to_string()))
                    .unwrap(),
                deps.api
                    .canonical_address(&HumanAddr("address3".to_string()))
                    .unwrap(),
            ];
            let addresses3 = vec![deps
                .api
                .canonical_address(&HumanAddr("address2".to_string()))
                .unwrap()];
            combination_storage(&mut deps.storage).save(
                &combination.as_bytes(),
                &Combination {
                    addresses: addresses1.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination2.as_bytes(),
                &Combination {
                    addresses: addresses2.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination3.as_bytes(),
                &Combination {
                    addresses: addresses3.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination4.as_bytes(),
                &Combination {
                    addresses: addresses2.clone(),
                },
            );
            combination_storage(&mut deps.storage).save(
                &combination5.as_bytes(),
                &Combination {
                    addresses: addresses1.clone(),
                },
            );
        }
        #[test]
        fn success() {
            let signature  = hex::decode("99afb21f4194b282b5025cad5855b01e7f562612233aecd49ed76a32987ca7e1d3abe043ba280efd2a97467fba2639060d9bd4608e0ab5fa10754b025e011d658a73dac6ae265325a0d8c4d1dd45f0b488150e89567f807ce50cd8ba58684dde").unwrap().into();
            let previous_signature  = hex::decode("a04b9a86fb2bcaaac6a595c6405bf5bb1af657c078bf7a88f8ec74b1d363ae17e97e7f3fe466e2fc699a656312f646aa009a8b0f5e5d6b2d603a40bec29132827e59fc8f85c971482a8100bd3301c0c5c42e7f03ddaa87ba15134faa1a629027").unwrap().into();
            let round: u64 = 519530;
            let msg = HandleMsg::Play {
                round: round,
                previous_signature: previous_signature,
                signature: signature,
            };
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            init_combination(&mut deps);

            // Test if combination have been added correctly
            let res = query_all_combination(deps.as_ref()).unwrap();
            assert_eq!(5, res.combination.len());

            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(80_000)
                    }]
                })
            );
            // Test if the five combination winners have been added correctly
            let res = query_all_winner(deps.as_ref()).unwrap();
            assert_eq!(5, res.winner.len());
            // Test if combination have been removed correctly
            let res = query_all_combination(deps.as_ref()).unwrap();
            assert_eq!(0, res.combination.len());

            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            println!("{}", res.holdersRewards.u128());
            // Test fees is now superior to 0
            assert_ne!(0, res.holdersRewards.u128());
            // Test block to play superior block height
            assert!(res.blockTimePlay > env.block.time);
            // Test if round was saved
            let res = query_latest(deps.as_ref()).unwrap();
            println!("{:?}", res);
            assert_eq!(519530, res.round);

            // Test if winners have been added at rank 1
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert_eq!(2, res.winners.len());
            // Test if winners have been added at rank 2
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&2_u8.to_be_bytes())
                .unwrap();
            assert_eq!(3, res.winners.len());
            // Test if winners have been added at rank 3
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&3_u8.to_be_bytes())
                .unwrap();
            assert_eq!(1, res.winners.len());
            // Test if winners have been added at rank 4
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&4_u8.to_be_bytes())
                .unwrap();
            assert_eq!(3, res.winners.len());
            println!("{:?}", res);
            // Test if winners have been added at rank 5
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&5_u8.to_be_bytes())
                .unwrap();
            assert_eq!(2, res.winners.len());

            // PLay second time before block time end error
            init_combination(&mut deps);
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn do_not_send_funds() {
            let signature  = hex::decode("99afb21f4194b282b5025cad5855b01e7f562612233aecd49ed76a32987ca7e1d3abe043ba280efd2a97467fba2639060d9bd4608e0ab5fa10754b025e011d658a73dac6ae265325a0d8c4d1dd45f0b488150e89567f807ce50cd8ba58684dde").unwrap().into();
            let previous_signature  = hex::decode("a04b9a86fb2bcaaac6a595c6405bf5bb1af657c078bf7a88f8ec74b1d363ae17e97e7f3fe466e2fc699a656312f646aa009a8b0f5e5d6b2d603a40bec29132827e59fc8f85c971482a8100bd3301c0c5c42e7f03ddaa87ba15134faa1a629027").unwrap().into();
            let round: u64 = 519530;
            let msg = HandleMsg::Play {
                round: round,
                previous_signature: previous_signature,
                signature: signature,
            };
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);
            init_combination(&mut deps);

            let info = mock_info(
                HumanAddr::from("validator1"),
                &[Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                }],
            );
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!(msg, "Play")
                }
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn no_players_combination_empty() {
            let signature  = hex::decode("99afb21f4194b282b5025cad5855b01e7f562612233aecd49ed76a32987ca7e1d3abe043ba280efd2a97467fba2639060d9bd4608e0ab5fa10754b025e011d658a73dac6ae265325a0d8c4d1dd45f0b488150e89567f807ce50cd8ba58684dde").unwrap().into();
            let previous_signature  = hex::decode("a04b9a86fb2bcaaac6a595c6405bf5bb1af657c078bf7a88f8ec74b1d363ae17e97e7f3fe466e2fc699a656312f646aa009a8b0f5e5d6b2d603a40bec29132827e59fc8f85c971482a8100bd3301c0c5c42e7f03ddaa87ba15134faa1a629027").unwrap().into();
            let round: u64 = 519530;
            let msg = HandleMsg::Play {
                round: round,
                previous_signature: previous_signature,
                signature: signature,
            };
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            default_init(&mut deps);

            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let mut env = mock_env();
            // Set block time superior to the block play so the lottery can start
            env.block.time = 1610966920;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(80_000)
                    }]
                })
            );

            let res = winner_storage_read(deps.as_ref().storage).load(&[]);
            let winners = res.and_then(|i| Ok(i));
            match winners {
                Err(StdError::NotFound { kind, .. }) => assert_eq!(kind, "lottery::state::Winner"),
                _ => panic!("Unexpected error"),
            }
        }
    }
    mod holder_claim_reward {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn contract_balance_empty() {
            let mut deps = mock_dependencies(&[]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn not_allowed_to_claim() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(4_532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn do_not_send_funds() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(
                HumanAddr::from("validator1"),
                &[Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532_004),
                }],
            );
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!(msg, "Reward")
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn shares_to_low() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::SharesTooLow {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success() {
            let mut deps = mock_dependencies(&[Coin {
                denom: "uscrt".to_string(),
                amount: Uint128(10_000_000),
            }]);
            deps.querier.update_balance(
                HumanAddr::from("validator1"),
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(4_532_004),
                }],
            );
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("validator1"), &[]);
            let env = mock_env();
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone()).unwrap();
            assert_eq!(1, res.messages.len());
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("validator1"),
                    amount: vec![Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(2349)
                    }]
                })
            );
            // Test if state have changed correctly
            let res = query_config(deps.as_ref()).unwrap();
            // Test if claimReward is not empty and the right address was added correctly
            assert_ne!(0, res.claimReward.len());
            assert!(res.claimReward.contains(
                &deps
                    .api
                    .canonical_address(&HumanAddr::from("validator1"))
                    .unwrap()
            ));
            // Test reward amount was updated with success
            assert_ne!(5_221, res.holdersRewards.u128());

            // Error do not claim multiple times
            let res = handle_reward(deps.as_mut(), env.clone(), info.clone());
            match res {
                Err(ContractError::AlreadyClaimed {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }

    mod winner_claim_jackpot {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        #[test]
        fn do_not_send_funds() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            // Test sender is not sending funds
            let info = mock_info(
                HumanAddr::from("address2"),
                &[Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                }],
            );
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!("Jackpot", msg);
                }
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_reward() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            let mut state = config_read(deps.as_mut().storage).load().unwrap();
            state.jackpotReward = Uint128(0);
            config(deps.as_mut().storage).save(&state);
            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn no_winner() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::NoWinners {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn some_winners_with_contract_ticket_prize_balance_empty() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(20),
                },
            ]);
            default_init(&mut deps);
            // Test with some winners and empty balance ticket
            deps.querier.update_balance(
                MOCK_CONTRACT_ADDR,
                vec![Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                }],
            );
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );
            // Test transaction succeed if not ticket ujack to send
            // in this case the winner only have won tickets but since the balance is empty he will get nothing
            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(0, res.messages.len());
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "jackpot reward"),
                    attr("to", "address2"),
                    attr("jackpot_prize", "no")
                ]
            )
        }
        #[test]
        fn prioritize_coin_prize_rather_than_ticket_prize() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(0),
                },
            ]);
            default_init(&mut deps);
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address1"),
                    amount: vec![Coin {
                        denom: "uscrt".to_string(),
                        amount: Uint128(6720000)
                    }]
                })
            );
        }
        #[test]
        fn handle_contract_balance_is_empty_ticket() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(0),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(0),
                },
            ]);
            default_init(&mut deps);
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            // Test error since the winner won uscrt and is mor important prize than ujack ticket
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn contract_balance_is_empty() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::EmptyBalance {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
        #[test]
        fn success_claim_only_ticket() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(0),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(10),
                },
            ]);
            default_init(&mut deps);
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            let info = mock_info(HumanAddr::from("address2"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address2"),
                    amount: vec![Coin {
                        denom: "ujack".to_string(),
                        amount: Uint128(1)
                    }]
                })
            );
        }

        #[test]
        fn success_claim() {
            let mut deps = mock_dependencies(&[
                Coin {
                    denom: "uscrt".to_string(),
                    amount: Uint128(10_000_000_000),
                },
                Coin {
                    denom: "ujack".to_string(),
                    amount: Uint128(10),
                },
            ]);
            default_init(&mut deps);
            // Init winner storage for test
            let key1: u8 = 1;
            let key2: u8 = 5;
            winner_storage(&mut deps.storage).save(
                &key1.to_be_bytes(),
                &Winner {
                    winners: vec![WinnerInfoState {
                        claimed: false,
                        address: deps
                            .api
                            .canonical_address(&HumanAddr("address1".to_string()))
                            .unwrap(),
                    }],
                },
            );
            winner_storage(&mut deps.storage).save(
                &key2.to_be_bytes(),
                &Winner {
                    winners: vec![
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address2".to_string()))
                                .unwrap(),
                        },
                        WinnerInfoState {
                            claimed: false,
                            address: deps
                                .api
                                .canonical_address(&HumanAddr("address1".to_string()))
                                .unwrap(),
                        },
                    ],
                },
            );

            // Test success address3 claim jackpot and get 3 ticket since he won 3 times
            let info = mock_info(HumanAddr::from("address1"), &[]);
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone()).unwrap();
            assert_eq!(
                res.messages[0],
                CosmosMsg::Bank(BankMsg::Send {
                    from_address: HumanAddr::from(MOCK_CONTRACT_ADDR),
                    to_address: HumanAddr::from("address1"),
                    amount: vec![
                        Coin {
                            denom: "uscrt".to_string(),
                            amount: Uint128(6720000)
                        },
                        Coin {
                            denom: "ujack".to_string(),
                            amount: Uint128(1)
                        }
                    ]
                })
            );
            // test if sender is claimed true
            let res = winner_storage_read(deps.as_ref().storage)
                .load(&1_u8.to_be_bytes())
                .unwrap();
            assert!(res.winners[0].claimed);

            // Test sender can't claim jackpot anymore since claimed is true
            let res = handle_jackpot(deps.as_mut(), mock_env(), info.clone());
            match res {
                Err(ContractError::AlreadyClaimed {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }

    #[test]
    fn proposal() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        // Test success proposal HolderFeePercentage
        let info = mock_info(HumanAddr::from("address2"), &[]);
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to a more expensive".to_string(),
            proposal: Proposal::HolderFeePercentage,
            amount: Option::from(Uint128(15)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::HolderFeePercentage, storage.proposal);
        assert_eq!(Uint128(15), storage.amount);

        // Test success proposal MinAmountValidator
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to a more expensive".to_string(),
            proposal: Proposal::MinAmountValidator,
            amount: Option::from(Uint128(15_000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&2_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::MinAmountValidator, storage.proposal);
        assert_eq!(Uint128(15000), storage.amount);

        // Test success proposal PrizePerRank
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new prize rank".to_string(),
            proposal: Proposal::PrizePerRank,
            amount: None,
            prizePerRank: Option::from(vec![60, 20, 10, 10]),
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&3_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::PrizePerRank, storage.proposal);
        assert_eq!(vec![60, 20, 10, 10], storage.prizeRank);

        // Test success proposal MinAmountDelegator
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new amount delegator".to_string(),
            proposal: Proposal::MinAmountDelegator,
            amount: Option::from(Uint128(10_000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&4_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::MinAmountDelegator, storage.proposal);
        assert_eq!(Uint128(10000), storage.amount);

        // Test success proposal JackpotRewardPercentage
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new jackpot percentage".to_string(),
            proposal: Proposal::JackpotRewardPercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&5_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::JackpotRewardPercentage, storage.proposal);
        assert_eq!(Uint128(10), storage.amount);

        // Test success proposal DrandWorkerFeePercentage
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new worker percentage".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&6_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::DrandWorkerFeePercentage, storage.proposal);
        assert_eq!(Uint128(10), storage.amount);

        // Test success proposal LotteryEveryBlockTime
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&7_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::LotteryEveryBlockTime, storage.proposal);
        assert_eq!(Uint128(100000), storage.amount);

        // Test success proposal ClaimEveryBlock
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::ClaimEveryBlock,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&8_u64.to_be_bytes())
            .unwrap();
        assert_eq!(Proposal::ClaimEveryBlock, storage.proposal);
        assert_eq!(Uint128(100000), storage.amount);

        // Test saved correctly the state
        let res = config_read(deps.as_ref().storage).load().unwrap();
        assert_eq!(8, res.pollCount);

        // Test error description too short
        let msg = HandleMsg::Proposal {
            description: "I".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DescriptionTooShort(msg)) => {
                assert_eq!("6", msg);
            }
            _ => panic!("Unexpected error"),
        }

        // Test error description too long
        let msg = HandleMsg::Proposal {
            description: "I erweoi oweijr w oweijr woerwe oirjewoi rewoirj ewoirjewr weiorjewoirjwerieworijewewriewo werioj ew".to_string(),
            proposal: Proposal::DrandWorkerFeePercentage,
            amount: Option::from(Uint128(10)),
            prizePerRank: None
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DescriptionTooLong(msg)) => {
                assert_eq!("64", msg);
            }
            _ => panic!("Unexpected error"),
        }

        // Test error description too long
        let msg = HandleMsg::Proposal {
            description: "Default".to_string(),
            proposal: Proposal::NotExist,
            amount: Option::from(Uint128(10)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalNotFound {}) => {}
            _ => panic!("Unexpected error"),
        }

        // Test error sending funds with proposal
        let info = mock_info(
            HumanAddr::from("address2"),
            &[Coin {
                denom: "kii".to_string(),
                amount: Uint128(500),
            }],
        );
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DoNotSendFunds(msg)) => {
                assert_eq!("Proposal", msg);
            }
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn vote() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("address2"), &[]);

        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: false,
        };
        // Error not found storage key
        let errMsg = "lottery::state::PollInfoState".to_string();
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(Std(NotFound { kind: errMsg, .. })) => {}
            _ => panic!("Unexpected error"),
        }

        // Init proposal
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());

        // Test success proposal HolderFeePercentage
        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: false,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        // Test success added to the no voter array
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert!(storage.no_voters.contains(
            &deps
                .api
                .canonical_address(&HumanAddr::from("address2"))
                .unwrap()
        ));
        // Test only added 1 time
        assert_eq!(1, storage.no_voters.len());

        // Test only can vote one time per proposal
        let msg = HandleMsg::Vote {
            pollId: 1,
            approve: true,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::AlreadyVoted {}) => {}
            _ => panic!("Unexpected error"),
        }
        // Test sender is not added to vote cause multiple times votes
        let storage = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert!(!storage.yes_voters.contains(
            &deps
                .api
                .canonical_address(&HumanAddr::from("address2"))
                .unwrap()
        ));

        // Test proposal is expired
        let env = mock_env();
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the status to rejected by the creator
            pollData.status = PollStatus::RejectedByCreator;
            // Update the end eight to now
            pollData.end_height = env.block.height - 1;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalExpired {}) => {}
            _ => panic!("Unexpected error"),
        }
    }

    #[test]
    fn creator_reject_proposal() {
        let mut deps = mock_dependencies(&[]);
        default_init(&mut deps);
        let info = mock_info(HumanAddr::from("creator"), &[]);
        // Init proposal
        let msg = HandleMsg::Proposal {
            description: "I think we need to up to new block time".to_string(),
            proposal: Proposal::LotteryEveryBlockTime,
            amount: Option::from(Uint128(100000)),
            prizePerRank: None,
        };
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());

        // Init reject proposal
        let msg = HandleMsg::RejectProposal { pollId: 1 };
        let env = mock_env();

        // test error do not send funds with reject proposal
        let info = mock_info(
            HumanAddr::from("creator"),
            &[Coin {
                denom: "xcoin".to_string(),
                amount: Uint128(19_000),
            }],
        );
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        match res {
            Err(ContractError::DoNotSendFunds(msg)) => {
                assert_eq!("RejectProposal", msg)
            }
            _ => panic!("Unexpected error"),
        }

        // Success reject the proposal
        let info = mock_info(HumanAddr::from("creator"), &[]);
        let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        assert_eq!(0, res.messages.len());

        let store = poll_storage_read(deps.as_ref().storage)
            .load(&1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(env.block.height, store.end_height);
        assert_eq!(PollStatus::RejectedByCreator, store.status);

        // Test error since the sender is not the creator
        let info = mock_info(HumanAddr::from("otherUser"), &[]);
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Unexpected error"),
        }

        // Test error the proposal is not inProgress
        let info = mock_info(HumanAddr::from("creator"), &[]);
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the status to rejected by the creator
            pollData.status = PollStatus::RejectedByCreator;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Unexpected error"),
        }
        // Test error the proposal already expired
        let info = mock_info(HumanAddr::from("creator"), &[]);
        poll_storage(deps.as_mut().storage).update::<_, StdError>(&1_u64.to_be_bytes(), |poll| {
            let mut pollData = poll.unwrap();
            // Update the end eight to now
            pollData.end_height = env.block.height - 1;
            Ok(pollData)
        });
        let res = handle(deps.as_mut(), mock_env(), info.clone(), msg.clone());
        match res {
            Err(ContractError::ProposalExpired {}) => {}
            _ => panic!("Unexpected error"),
        }
    }

    mod present_proposal {
        use super::*;
        use crate::error::ContractError;
        use cosmwasm_std::attr;

        fn init_proposal(deps: DepsMut, env: &Env) {
            let info = mock_info(HumanAddr::from("creator"), &[]);
            let msg = HandleMsg::Proposal {
                description: "I think we need to up to new block time".to_string(),
                proposal: Proposal::LotteryEveryBlockTime,
                amount: Option::from(Uint128(200_000)),
                prizePerRank: None,
            };
            handle(deps, env.clone(), info.clone(), msg.clone());
        }

        fn init_voter(address: String, deps: DepsMut, vote: bool, env: &Env) {
            let info = mock_info(HumanAddr::from(address.clone()), &[]);
            let msg = HandleMsg::Vote {
                pollId: 1,
                approve: vote,
            };
            handle(deps, env.clone(), info.clone(), msg.clone());
        }

        #[test]
        fn present_proposal_with_empty_voters() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "rejected")
                ]
            );
        }

        #[test]
        fn present_proposal_with_voters_NO_WEIGHT() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let stateBefore = config_read(deps.as_ref().storage).load().unwrap();
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), false, &env);
            init_voter("address2".to_string(), deps.as_mut(), false, &env);

            // Init present proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "rejected")
                ]
            );
            assert_eq!(0, res.messages.len());
            // check if state remain the same since the vote was rejected
            let stateAfter = config_read(deps.as_ref().storage).load().unwrap();
            assert_eq!(
                stateBefore.everyBlockTimePlay,
                stateAfter.everyBlockTimePlay
            );
        }
        #[test]
        fn present_proposal_with_voters_YES_WEIGHT() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), true, &env);
            init_voter("address2".to_string(), deps.as_mut(), true, &env);

            // Init present proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
            assert_eq!(
                res.attributes,
                vec![
                    attr("action", "present the proposal"),
                    attr("proposal_id", "1"),
                    attr("proposal_result", "approved")
                ]
            );
            assert_eq!(0, res.messages.len());
            // check if state the vote was rejected
            let state = config_read(deps.as_ref().storage).load().unwrap();
            assert_eq!(200000, state.everyBlockTimePlay);

            // Test can't REpresent the proposal
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::Unauthorized {}) => {}
                _ => panic!("Unexpected error"),
            }
        }

        #[test]
        fn error_present_proposal() {
            let mut deps = mock_dependencies(&[]);
            default_init(&mut deps);
            let mut env = mock_env();
            env.block.height = 10_000;
            // Init proposal
            init_proposal(deps.as_mut(), &env);

            // Init two votes with approval false
            deps.querier.update_balance(
                "address1",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(1_000_000),
                }],
            );
            deps.querier.update_balance(
                "address2",
                vec![Coin {
                    denom: "upot".to_string(),
                    amount: Uint128(7_000_000),
                }],
            );
            init_voter("address1".to_string(), deps.as_mut(), true, &env);
            init_voter("address2".to_string(), deps.as_mut(), true, &env);

            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let info = mock_info(
                HumanAddr::from("sender"),
                &[Coin {
                    denom: "funds".to_string(),
                    amount: Uint128(324),
                }],
            );
            let mut env = mock_env();
            // expire the proposal to allow presentation
            env.block.height = 100_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::DoNotSendFunds(msg)) => {
                    assert_eq!("PresentProposal", msg)
                }
                _ => panic!("Unexpected error"),
            }

            // proposal not expired
            let msg = HandleMsg::PresentProposal { pollId: 1 };
            let info = mock_info(HumanAddr::from("sender"), &[]);
            let mut env = mock_env();
            env.block.height = 10_000;
            let res = handle(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            match res {
                Err(ContractError::ProposalNotExpired {}) => {}
                _ => panic!("Unexpected error"),
            }
        }
    }
}
