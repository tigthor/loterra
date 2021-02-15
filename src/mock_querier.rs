use crate::msg::QueryMsg;
use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_slice, to_binary, Api, Binary, CanonicalAddr, Coin, Empty, Extern, HumanAddr, Querier,
    QuerierResult, QueryRequest, StdResult, SystemError, Uint128, WasmQuery,
};
use cosmwasm_storage::to_length_prefixed;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn mock_dependencies_custom(
    canonical_length: usize,
    contract_balance: &[Coin],
) -> Extern<MockStorage, MockApi, WasmMockQuerier> {
    let contract_addr = HumanAddr::from(MOCK_CONTRACT_ADDR);
    let custom_querier: WasmMockQuerier = WasmMockQuerier::new(
        MockQuerier::new(&[(&contract_addr, contract_balance)]),
        canonical_length,
        MockApi::new(canonical_length),
    );

    Extern {
        storage: MockStorage::default(),
        api: MockApi::new(canonical_length),
        querier: custom_querier,
    }
}

pub struct WasmMockQuerier {
    base: MockQuerier<Empty>,
    terrand_response: TerrandResponse,
    lottery_balance_response: LotteraBalanceResponse,
}

#[derive(Clone, Default, Serialize)]
pub struct TerrandResponse {
    pub randomness: Binary,
    pub worker: HumanAddr,
}

impl TerrandResponse {
    pub fn new(randomness: Binary, worker: HumanAddr) -> Self {
        TerrandResponse { randomness, worker }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct LotteraBalanceResponse {
    pub balance: Uint128,
}

impl LotteraBalanceResponse {
    pub fn new(balance: Uint128) -> Self {
        LotteraBalanceResponse { balance }
    }
}

impl Querier for WasmMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        // MockQuerier doesn't support Custom, so we ignore it completely here
        let request: QueryRequest<Empty> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {}", e),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}
impl WasmMockQuerier {
    pub fn handle_query(&self, request: &QueryRequest<Empty>) -> QuerierResult {
        match &request {
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, .. }) => {
                if contract_addr == &HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5loterra")
                {
                    println!("{:?}", request);
                    let msg_balance = LotteraBalanceResponse {
                        balance: self.lottery_balance_response.balance,
                    };
                    return Ok(to_binary(&msg_balance));
                } else if contract_addr
                    == &HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5terrand")
                {
                    let msg_terrand = TerrandResponse {
                        randomness: Binary::from(
                            "/f8IwVIaGXfcKw4V7MfBzz2mlgZ3hNIB+ppPoZ67Xks=".as_bytes(),
                        ),
                        worker: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5terrand"),
                    };
                    return Ok(to_binary(&msg_terrand));
                }
                panic!("DO NOT ENTER HERE")
            }
            _ => self.base.handle_query(request),
        }
    }
}
impl WasmMockQuerier {
    pub fn new<A: Api>(base: MockQuerier<Empty>, _canonical_length: usize, _api: A) -> Self {
        WasmMockQuerier {
            base,
            terrand_response: TerrandResponse::default(),
            lottery_balance_response: LotteraBalanceResponse::default(),
        }
    }

    // configure the mint whitelist mock querier
    pub fn with_token_balances(&mut self, balances: Uint128) {
        self.lottery_balance_response = LotteraBalanceResponse::new(balances);
    }
}
