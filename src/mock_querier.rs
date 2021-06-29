use crate::query::{GetHoldersResponse, HoldersInfo};
use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_slice, to_binary, Binary, Coin, ContractResult, Decimal, OwnedDeps, Querier,
    QuerierResult, QueryRequest, SystemError, SystemResult, Uint128, WasmQuery,
};
use serde::Serialize;
use terra_cosmwasm::{TaxCapResponse, TaxRateResponse, TerraQuery, TerraQueryWrapper};

pub fn mock_dependencies_custom(
    contract_balance: &[Coin],
) -> OwnedDeps<MockStorage, MockApi, WasmMockQuerier> {
    let custom_querier =
        WasmMockQuerier::new(MockQuerier::new(&[(MOCK_CONTRACT_ADDR, contract_balance)]));
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
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
    pub worker: String,
}

impl TerrandResponse {
    pub fn new(randomness: Binary, worker: String) -> Self {
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
    pub address: String,
    pub balance: Uint128,
    pub index: Decimal,
    pub pending_rewards: Decimal,
}

impl GetHolderResponse {
    pub fn new(
        address: String,
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
        let request: QueryRequest<TerraQueryWrapper> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
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
                println!("{:?}", msg);
                if contract_addr == &"cw20".to_string() {
                    println!("{:?}", request);
                    let msg_balance = LotteraBalanceResponse {
                        balance: self.lottery_balance_response.balance,
                    };
                    return SystemResult::Ok(ContractResult::from(to_binary(&msg_balance)));
                } else if contract_addr == &"terrand".to_string() {
                    let msg_terrand = TerrandResponse {
                        randomness: Binary::from(
                            "OdRl+j6PHnN84dy12n4Oq1BrGktD73FW4SKPihxfB9I=".as_bytes(),
                        ),
                        worker: "terra1q88h7ewu6h3am4mxxeqhu3srxterrandworker".to_string(),
                    };
                    return SystemResult::Ok(ContractResult::from(to_binary(&msg_terrand)));
                } else if contract_addr == &"staking".to_string() {
                    if msg == &Binary::from(r#"{"get_all_bonded":{}}"#.as_bytes()) {
                        let msg_balance = GetAllBondedResponse {
                            total_bonded: self.lottery_balance_response.balance.clone(),
                        };
                        return SystemResult::Ok(ContractResult::from(to_binary(&msg_balance)));
                    } else if msg
                        == &Binary::from(r#"{"holder":{"address":"addr0000"}}"#.as_bytes())
                    {
                        let msg_balance = GetHolderResponse {
                            address: "addr0000".to_string(),
                            balance: self.holder_response.balance,
                            index: self.holder_response.index,
                            pending_rewards: self.holder_response.pending_rewards,
                        };
                        return SystemResult::Ok(ContractResult::from(to_binary(&msg_balance)));
                    } else if msg
                        == &Binary::from(
                            r#"{"holders":{"start_after":null,"limit":null}}"#.as_bytes(),
                        )
                    {
                        let msg_holders = GetHoldersResponse {
                            holders: vec![
                                HoldersInfo {
                                    address: "addr0000".to_string(),
                                    balance: Uint128(15_000),
                                    index: Decimal::zero(),
                                    pending_rewards: Decimal::zero(),
                                },
                                HoldersInfo {
                                    address: "addr0001".to_string(),
                                    balance: Uint128(10_000),
                                    index: Decimal::zero(),
                                    pending_rewards: Decimal::zero(),
                                },
                            ],
                        };
                        return SystemResult::Ok(ContractResult::from(to_binary(&msg_holders)));
                    } else if msg == &Binary::from(
                        r#"{"holder":{"address":"terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20qu3k"}}"#
                            .as_bytes(),
                    ) {
                        let msg_balance = GetHolderResponse {
                            address: "terra1q88h7ewu6h3am4mxxeqhu3srt7zw4z5s20q007".to_string(),
                            balance: self.holder_response.balance,
                            index: self.holder_response.index,
                            pending_rewards: self.holder_response.pending_rewards,
                        };
                        return SystemResult::Ok(ContractResult::from(to_binary(&msg_balance)));
                    }
                }
                panic!("DO NOT ENTER HERE")
            }
            QueryRequest::Custom(TerraQueryWrapper { route, query_data }) => match query_data {
                TerraQuery::TaxRate {} => {
                    let res = TaxRateResponse {
                        rate: Decimal::percent(1),
                    };
                    SystemResult::Ok(ContractResult::from(to_binary(&res)))
                }
                TerraQuery::TaxCap { denom: _ } => {
                    let cap = Uint128(1000000u128);
                    let res = TaxCapResponse { cap };
                    SystemResult::Ok(ContractResult::from(to_binary(&res)))
                }
                _ => panic!("DO NOT ENTER HERE"),
            },
            _ => self.base.handle_query(request),
        }
    }
}

impl WasmMockQuerier {
    pub fn new(base: MockQuerier<TerraQueryWrapper>) -> Self {
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
        address: String,
        balance: Uint128,
        index: Decimal,
        pending_rewards: Decimal,
    ) {
        self.holder_response = GetHolderResponse::new(address, balance, index, pending_rewards)
    }
}
