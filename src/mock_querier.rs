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
use terra_cosmwasm::TaxCapResponse;

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
    holder_response: GetHolderResponse,
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
#[derive(Clone, Default, Serialize)]
pub struct GetAllBondedResponse {
    pub total_bonded: Uint128,
}

impl GetAllBondedResponse {
    pub fn new(total_bonded: Uint128) -> Self {
        GetAllBondedResponse { total_bonded }
    }
}
#[derive(Clone, Default, Serialize)]
pub struct GetHolderResponse {
    pub address: HumanAddr,
    pub bonded: Uint128,
    pub un_bonded: Uint128,
    pub available: Uint128,
    pub period: u64,
}

impl GetHolderResponse {
    pub fn new(
        address: HumanAddr,
        bonded: Uint128,
        un_bonded: Uint128,
        available: Uint128,
        period: u64,
    ) -> Self {
        GetHolderResponse {
            address,
            bonded,
            un_bonded,
            available,
            period,
        }
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
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                if contract_addr == &HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zloterracw20")
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
                            "OdRl+j6PHnN84dy12n4Oq1BrGktD73FW4SKPihxfB9I=".as_bytes(),
                        ),
                        worker: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker"),
                    };
                    return Ok(to_binary(&msg_terrand));
                } else if contract_addr
                    == &HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srloterrastaking")
                {
                    if msg == &Binary::from(r#"{"get_all_bonded":{}}"#.as_bytes()){
                        let msg_balance = GetAllBondedResponse {
                            total_bonded: self.lottery_balance_response.balance.clone(),
                        };
                        return Ok(to_binary(&msg_balance));
                    }
                    else if msg == &Binary::from(r#"{"get_holder":{"address":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007"}}"#.as_bytes()){
                        let msg_balance = GetHolderResponse {
                            address: HumanAddr::from("terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007"),
                            bonded: self.holder_response.bonded,
                            un_bonded: self.holder_response.un_bonded,
                            available: self.holder_response.available,
                            period: self.holder_response.period
                        };
                        return Ok(to_binary(&msg_balance));
                    }
                }
                panic!("DO NOT ENTER HERE")
            }
            QueryRequest::Custom(_e) => {
                let x = TaxCapResponse { cap: Uint128(1) };
                return Ok(to_binary(&x));
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
            holder_response: GetHolderResponse::default(),
        }
    }

    // configure the mint whitelist mock querier
    pub fn with_token_balances(&mut self, balances: Uint128) {
        self.lottery_balance_response = LotteraBalanceResponse::new(balances);
    }

    // configure the mint whitelist mock querier
    pub fn with_holder(
        &mut self,
        address: HumanAddr,
        bonded: Uint128,
        un_bonded: Uint128,
        available: Uint128,
        period: u64,
    ) {
        self.holder_response = GetHolderResponse::new(address, bonded, un_bonded, available, period)
    }
}
