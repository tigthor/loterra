use crate::msg::{QueryMsg, HandleMsg};
use crate::query::{GetHolderResponse, LoterraBalanceResponse, TerrandResponse};
use crate::state::State;
use cosmwasm_std::{
    to_binary, Api, CanonicalAddr, Coin, CosmosMsg, Empty, Extern, HumanAddr, Querier,
    QueryRequest, StdResult, Storage, Uint128, WasmMsg, WasmQuery,
};
use moneymarket::market::EpochStateResponse;

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

pub fn encode_msg_execute_anchor<HandleMsg>(
    msg: HandleMsg,
    address: HumanAddr,
    coin: Vec<Coin>,
) -> StdResult<CosmosMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: address,
        msg: to_binary(&msg)?,
        send: coin,
    }
        .into())
}

pub fn encode_msg_query_anchor(msg: moneymarket::market::QueryMsg, address: HumanAddr) -> StdResult<QueryRequest<Empty>> {
    Ok(WasmQuery::Smart {
        contract_addr: address,
        msg: to_binary(&msg)?,
    }
        .into())
}


pub fn encode_msg_execute(
    msg: QueryMsg,
    address: HumanAddr,
    coin: Vec<Coin>,
) -> StdResult<CosmosMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: address,
        msg: to_binary(&msg)?,
        send: coin,
    }
    .into())
}
pub fn encode_msg_query(msg: QueryMsg, address: HumanAddr) -> StdResult<QueryRequest<Empty>> {
    Ok(WasmQuery::Smart {
        contract_addr: address,
        msg: to_binary(&msg)?,
    }
    .into())
}

pub fn wrapper_msg_terrand<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<TerrandResponse> {
    let res: TerrandResponse = deps.querier.query(&query)?;
    Ok(res)
}

pub fn wrapper_msg_loterra<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<LoterraBalanceResponse> {
    let res: LoterraBalanceResponse = deps.querier.query(&query)?;

    Ok(res)
}

pub fn wrapper_msg_loterra_staking<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<GetHolderResponse> {
    let res: GetHolderResponse = deps.querier.query(&query)?;

    Ok(res)
}

pub fn user_total_weight<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    state: &State,
    address: &CanonicalAddr,
) -> Uint128 {
    let mut weight = Uint128::zero();
    let human_address = deps.api.human_address(&address).unwrap();

    // Ensure sender have some reward tokens
    let msg = QueryMsg::Holder {
        address: human_address,
    };
    let loterra_human = deps
        .api
        .human_address(&state.loterra_staking_contract_address.clone())
        .unwrap();
    let res = encode_msg_query(msg, loterra_human).unwrap();
    let loterra_balance = wrapper_msg_loterra_staking(&deps, res).unwrap();

    if !loterra_balance.balance.is_zero() {
        weight += loterra_balance.balance;
    }

    weight
}

pub fn wrapper_msg_anchor<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    query: QueryRequest<Empty>,
) -> StdResult<EpochStateResponse> {
    let res: EpochStateResponse = deps.querier.query(&query)?;
    Ok(res)
}
