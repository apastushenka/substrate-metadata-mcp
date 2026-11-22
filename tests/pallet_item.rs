//! Constant values and storage defaults on real metadata, through the server binary, as the JSON
//! a client gets.

use rmcp::serde_json::{Value, json};

mod common;

use self::common::{Client, call, connect};

#[tokio::test]
async fn constants_are_json() {
    let client = connect("polkadot.scale").await;

    assert_eq!(
        item(&client, "Balances", "constant", "ExistentialDeposit").await["value"],
        json!("10000000000")
    );
    // A `PalletId` wraps `[u8; 8]`: `py/trsry` in hex.
    assert_eq!(
        item(&client, "Treasury", "constant", "PalletId").await["value"],
        json!("0x70792f7472737279")
    );

    let version = item(&client, "System", "constant", "Version").await;

    assert_eq!(version["value"]["spec_name"], json!("polkadot"));
    assert_eq!(
        version["value"]["apis"][0],
        json!(["0xdf6acb689907609b", 5])
    );

    let weights = item(&client, "System", "constant", "BlockWeights").await;
    let classes = &weights["value"]["per_class"];

    assert_eq!(classes["mandatory"]["max_extrinsic"], json!("None"));
    assert_eq!(
        classes["normal"]["max_extrinsic"],
        json!({ "Some": { "ref_time": "1479873955000", "proof_size": "13650590614545068195" } })
    );

    // An XCM location: an enum around a one-element array, a variant without fields, bytes.
    assert_eq!(
        item(&client, "Coretime", "constant", "BrokerPotLocation").await["value"],
        json!({ "X1": [{ "AccountId32": {
            "network": "None",
            "id": "0x6d6f646c70792f62726f6b650000000000000000000000000000000000000000"
        } }] })
    );

    client.cancel().await.expect("server stops");
}

#[tokio::test]
async fn storage_defaults_are_json() {
    let client = connect("polkadot.scale").await;

    assert_eq!(
        item(&client, "System", "storage", "Account").await["default"],
        json!({
            "nonce": 0,
            "consumers": 0,
            "providers": 0,
            "sufficients": 0,
            "data": {
                "free": "0",
                "reserved": "0",
                "frozen": "0",
                "flags": "170141183460469231731687303715884105728"
            }
        })
    );
    // Compared as text, since keys keep the order the runtime declares them in.
    assert_eq!(
        item(&client, "Staking", "storage", "Validators").await["default"].to_string(),
        r#"{"commission":0,"blocked":false}"#
    );
    assert_eq!(
        item(&client, "Staking", "storage", "ForceEra").await["default"],
        json!("NotForcing")
    );

    client.cancel().await.expect("server stops");
}

#[tokio::test]
async fn revive_base_fee_is_decimal_string() {
    let client = connect("polkadot-asset-hub.scale").await;

    let builder = item(&client, "Revive", "storage", "EthBlockBuilderIR").await;

    // A `primitive_types::U256`, kept as four `u64` limbs.
    assert_eq!(
        builder["default"]["base_fee_per_gas"],
        json!("800000000000")
    );

    client.cancel().await.expect("server stops");
}

#[tokio::test]
async fn every_constant_and_default_decodes() {
    // The counts show that the walk read every value, not only that none of them failed.
    assert_eq!(read_every_value("polkadot.scale").await, 296);
    assert_eq!(read_every_value("polkadot-asset-hub.scale").await, 380);
}

/// What `get_pallet_item` answers for one item of a pallet.
async fn item(client: &Client, pallet: &str, kind: &str, name: &str) -> Value {
    let arguments = json!({ "pallet": pallet, "kind": kind, "name": name });

    call(client, "get_pallet_item", arguments).await
}

/// Reads every constant and storage entry of `fixture` and returns how many values it read.
async fn read_every_value(fixture: &str) -> usize {
    let client = connect(fixture).await;
    let pallets = call(&client, "list_pallets", json!({})).await;
    let mut values = 0;

    for pallet in pallets["pallets"].as_array().expect("pallets are listed") {
        let pallet = pallet["name"].as_str().expect("a pallet has a name");
        let mut offset = 0;

        loop {
            let arguments =
                json!({ "pallet": pallet, "kinds": ["constant", "storage"], "offset": offset });
            let listing = call(&client, "list_pallet_items", arguments).await;
            let items = listing["items"].as_array().expect("items are listed");

            for listed in items {
                let kind = listed["kind"].as_str().expect("an item has a kind");
                let name = listed["name"].as_str().expect("an item has a name");
                let answer = item(&client, pallet, kind, name).await;

                // A constant always has its `value`, a storage entry has a `default` exactly
                // when it is not optional.
                let (field, present) = match kind {
                    "constant" => ("value", true),
                    _ => ("default", answer["optional"] == json!(false)),
                };

                assert_eq!(
                    answer.get(field).is_some(),
                    present,
                    "{pallet}::{name}: `{field}` {}",
                    if present {
                        "is missing"
                    } else {
                        "should be absent"
                    }
                );

                values += usize::from(present);
            }

            offset += items.len();

            if items.is_empty() || listing["total"] == json!(offset) {
                break;
            }
        }
    }

    client.cancel().await.expect("server stops");

    values
}
