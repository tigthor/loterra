use crate::state::{PollStatus, State, POLL};
use cosmwasm_std::{
    attr, to_binary, CanonicalAddr, DepsMut, Response, StdError, StdResult, Storage, Uint128,
    WasmQuery,
};
use loterra_staking_contract::msg::QueryMsg::{Holder, Holders};
use loterra_staking_contract::msg::{HolderResponse, HoldersResponse};

pub fn count_match(x: &str, y: &str) -> usize {
    let mut count = 0;
    for i in 0..y.len() {
        if x.chars().nth(i).unwrap() == y.chars().nth(i).unwrap() {
            count += 1;
        }
    }
    count
}
// There is probably some built-in function for this, but this is a simple way to do it
pub fn is_lower_hex(combination: &str, len: u8) -> bool {
    if combination.len() != (len as usize) {
        return false;
    }
    if !combination
        .chars()
        .all(|c| ('a'..='f').contains(&c) || ('0'..='9').contains(&c))
    {
        return false;
    }
    true
}

pub fn user_total_weight(deps: &DepsMut, state: &State, address: &CanonicalAddr) -> Uint128 {
    let mut weight = Uint128::zero();
    let human_address = deps.api.addr_humanize(&address).unwrap();

    // Ensure sender have some reward tokens
    let msg = Holder {
        address: human_address.to_string(),
    };

    let loterra_human = deps
        .api
        .addr_humanize(&state.loterra_staking_contract_address.clone())
        .unwrap();

    let query = WasmQuery::Smart {
        contract_addr: loterra_human.to_string(),
        msg: to_binary(&msg).unwrap(),
    }
    .into();
    let loterra_balance: HolderResponse = deps.querier.query(&query).unwrap();

    if !loterra_balance.balance.is_zero() {
        weight += loterra_balance.balance;
    }

    weight
}

pub fn total_weight(deps: &DepsMut, state: &State) -> Uint128 {
    let mut weight = Uint128::zero();

    // Ensure sender have some reward tokens
    let msg = Holders {
        start_after: None,
        limit: None,
    };

    let loterra_human = deps
        .api
        .addr_humanize(&state.loterra_staking_contract_address.clone())
        .unwrap();
    let query = WasmQuery::Smart {
        contract_addr: loterra_human.to_string(),
        msg: to_binary(&msg).unwrap(),
    }
    .into();
    let loterra_balance: HoldersResponse = deps.querier.query(&query).unwrap();

    for holder in loterra_balance.holders {
        if !holder.balance.is_zero() {
            weight += holder.balance;
        }
    }

    weight
}

pub fn reject_proposal(storage: &mut dyn Storage, poll_id: u64) -> StdResult<Response> {
    POLL.update(storage, &poll_id.to_be_bytes(), |poll| match poll {
        None => Err(StdError::generic_err("Proposal still in progress")),
        Some(poll_info) => {
            let mut poll = poll_info;
            poll.status = PollStatus::Rejected;
            Ok(poll)
        }
    })?;

    Ok(Response {
        submessages: vec![],
        messages: vec![],
        data: None,
        attributes: vec![
            attr("action", "present poll"),
            attr("poll_id", poll_id),
            attr("poll_result", "rejected"),
        ],
    })
}
