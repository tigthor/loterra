use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, export_schema_with_title, remove_schemas, schema_for};

use loterra::msg::{
    AllCombinationResponse, AllWinnerResponse, ConfigResponse, GetPollResponse, HandleMsg, InitMsg,
    QueryMsg, RoundResponse,
};
use loterra::query::TerrandResponse;

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InitMsg), &out_dir);
    export_schema(&schema_for!(HandleMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema_with_title(&mut schema_for!(ConfigResponse), &out_dir, "ConfigResponse");
    export_schema_with_title(
        &mut schema_for!(AllCombinationResponse),
        &out_dir,
        "AllCombinationResponse",
    );
    export_schema_with_title(
        &mut schema_for!(AllWinnerResponse),
        &out_dir,
        "AllWinnerResponse",
    );
    export_schema_with_title(
        &mut schema_for!(GetPollResponse),
        &out_dir,
        "GetPollResponse",
    );
    export_schema_with_title(&mut schema_for!(RoundResponse), &out_dir, "RoundResponse");
    export_schema_with_title(&mut schema_for!(TerrandResponse), &out_dir, "TerrandResponse");
}
