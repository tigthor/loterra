use crate::query::{GetHoldersResponse, HoldersInfo};
use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_slice, to_binary, Api, Binary, Coin, Decimal, Empty, Extern, HumanAddr, Querier,
    QuerierResult, QueryRequest, SystemError, Uint128, WasmQuery,
};
use serde::Serialize;
use terra_cosmwasm::{TaxCapResponse, TaxRateResponse, TerraQuery, TerraQueryWrapper, TerraRoute};

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
    base: MockQuerier<TerraQueryWrapper>,
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
    pub balance: Uint128,
    pub index: Decimal,
    pub pending_rewards: Decimal,
}

impl GetHolderResponse {
    pub fn new(
        address: HumanAddr,
        balance: Uint128,
        index: Decimal,
        pending_rewards: Decimal,
    ) -> Self {
        GetHolderResponse {
            address,
            balance,
            index,
            pending_rewards,
        }
    }
}

impl Querier for WasmMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        // MockQuerier doesn't support Custom, so we ignore it completely here
        let request: QueryRequest<TerraQueryWrapper> = match from_slice(bin_request) {
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
    pub fn handle_query(&self, request: &QueryRequest<TerraQueryWrapper>) -> QuerierResult {
        match &request {
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                println!("{:?} {}", request, msg);
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
                    if msg == &Binary::from(r#"{"get_all_bonded":{}}"#.as_bytes()) {
                        let msg_balance = GetAllBondedResponse {
                            total_bonded: self.lottery_balance_response.balance.clone(),
                        };
                        return Ok(to_binary(&msg_balance));
                    } else if msg == &Binary::from(
                        r#"{"holder":{"address":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007"}}"#
                            .as_bytes(),
                    ) {
                        let msg_balance = GetHolderResponse {
                            address: HumanAddr::from(
                                "terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007",
                            ),
                            balance: self.holder_response.balance,
                            index: self.holder_response.index,
                            pending_rewards: self.holder_response.pending_rewards,
                        };
                        return Ok(to_binary(&msg_balance));
                    } else if msg == &Binary::from(r#"{"holders":{}}"#.as_bytes()) {
                        let msg_holders = GetHoldersResponse {
                            holders: vec![
                                HoldersInfo {
                                    address: HumanAddr::default(),
                                    balance: Uint128(15_000),
                                    index: Decimal::zero(),
                                    pending_rewards: Decimal::zero(),
                                },
                                HoldersInfo {
                                    address: HumanAddr::default(),
                                    balance: Uint128(10_000),
                                    index: Decimal::zero(),
                                    pending_rewards: Decimal::zero(),
                                },
                            ],
                        };
                        return Ok(to_binary(&msg_holders));
                    } else if msg == &Binary::from(
                        r#"{"holder":{"address":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k"}}"#
                            .as_bytes(),
                    ) {
                        let msg_balance = GetHolderResponse {
                            address: HumanAddr::from(
                                "terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007",
                            ),
                            balance: self.holder_response.balance,
                            index: self.holder_response.index,
                            pending_rewards: self.holder_response.pending_rewards,
                        };
                        return Ok(to_binary(&msg_balance));
                    }
                }
                panic!("DO NOT ENTER HERE")
            }
            QueryRequest::Custom(TerraQueryWrapper { route, query_data }) => match query_data {
                TerraQuery::TaxRate {} => {
                    let res = TaxRateResponse {
                        rate: Decimal::percent(1),
                    };
                    Ok(to_binary(&res))
                }
                TerraQuery::TaxCap { denom: _ } => {
                    let cap = Uint128(1u128);
                    let res = TaxCapResponse { cap };
                    Ok(to_binary(&res))
                }
                _ => panic!("DO NOT ENTER HERE"),
            },
            _ => self.base.handle_query(request),
        }
    }
}
impl WasmMockQuerier {
    pub fn new<A: Api>(
        base: MockQuerier<TerraQueryWrapper>,
        _canonical_length: usize,
        _api: A,
    ) -> Self {
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
        balance: Uint128,
        index: Decimal,
        pending_rewards: Decimal,
    ) {
        self.holder_response = GetHolderResponse::new(address, balance, index, pending_rewards)
    }
}
