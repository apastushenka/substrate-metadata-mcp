//! Search on real metadata, through the server binary, the way a client talks to it.
//!
//! The fixtures are the metadata of Polkadot and of Polkadot Asset Hub, both at spec 2005003.
//! They stay as they are, so that a runtime upgrade cannot move these tests.
//!
//! The package leaves tests/ out (`exclude` in Cargo.toml): the fixtures outweigh the crate
//! several times over, and a test inside src/ would ship without them and fail there.

use rmcp::{
    model::{CallToolRequestParams, ClientConfig, object},
    serde_json::{Value, json},
    serve_client,
    transport::TokioChildProcess,
};
use tokio::process::Command;

/// How far down the hits a target may sit.
const TOP: usize = 5;

/// A query an agent sent in a real session, with the `kinds` it gave, and the items it read
/// next.
struct Case {
    query: &'static str,
    kinds: &'static [&'static str],
    /// Items as `kind namespace::name`, or `kind name` for a pallet. Every group has to be among
    /// the hits; any one item of a group will do.
    targets: &'static [&'static [&'static str]],
}

#[tokio::test]
async fn real_queries_on_polkadot_find_targets_in_top_five() {
    assert_targets_found(
        "polkadot.scale",
        &[
            Case {
                query: "claims",
                kinds: &[],
                targets: &[&["pallet Claims"]],
            },
            Case {
                query: "bounties",
                kinds: &["pallet"],
                targets: &[&["pallet Bounties"]],
            },
            Case {
                query: "transfer allow death",
                kinds: &["call"],
                targets: &[&["call Balances::transfer_allow_death"]],
            },
        ],
    )
    .await;
}

// These queries were sent to another chain. Where a query or its target named a pallet or a
// runtime API of that chain, it names the matching one here.
#[tokio::test]
async fn real_queries_on_asset_hub_find_targets_in_top_five() {
    assert_targets_found(
        "polkadot-asset-hub.scale",
        &[
            Case {
                query: "swap",
                kinds: &["call"],
                targets: &[&["call AssetConversion::swap_exact_tokens_for_tokens"]],
            },
            Case {
                query: "AssetTxPayment",
                kinds: &[],
                targets: &[&["pallet AssetTxPayment"]],
            },
            Case {
                query: "AssetConversion",
                kinds: &["pallet"],
                targets: &[&["pallet AssetConversion"]],
            },
            Case {
                query: "SlashDeferDuration",
                kinds: &["constant"],
                targets: &[&["constant Staking::SlashDeferDuration"]],
            },
            Case {
                query: "AssetAccountDeposit",
                kinds: &["constant"],
                targets: &[&["constant Assets::AssetAccountDeposit"]],
            },
            Case {
                query: "Proxy",
                kinds: &[],
                targets: &[&["pallet Proxy"]],
            },
            Case {
                query: "quote",
                kinds: &["call", "view_function", "runtime_api_method"],
                targets: &[&[
                    "runtime_api_method AssetConversionApi::quote_price_exact_tokens_for_tokens",
                    "runtime_api_method AssetConversionApi::quote_price_tokens_for_exact_tokens",
                ]],
            },
        ],
    )
    .await;
}

async fn assert_targets_found(fixture: &str, cases: &[Case]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_substrate-metadata-mcp"));
    command.arg("--file").arg(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ));

    let transport = TokioChildProcess::new(command).expect("server starts");
    let client = serve_client(ClientConfig::default(), transport)
        .await
        .expect("server initializes");
    let mut misses = Vec::new();

    for case in cases {
        let arguments = json!({ "query": case.query, "kinds": case.kinds, "limit": TOP });
        let params = CallToolRequestParams::new("search").with_arguments(object(arguments));
        let result = client.call_tool(params).await.expect("search answers");

        assert!(
            result.is_error != Some(true),
            "search {:?} failed: {}",
            case.query,
            json!(result.content)
        );

        let content = result.structured_content.expect("search returns JSON");
        let hits: Vec<String> = content["hits"]
            .as_array()
            .expect("search returns hits")
            .iter()
            .map(address)
            .collect();

        for group in case.targets {
            if !hits.iter().any(|hit| group.contains(&hit.as_str())) {
                misses.push(format!("{:?}: none of {group:?} in {hits:?}", case.query));
            }
        }
    }

    client.cancel().await.expect("server stops");

    assert!(
        misses.is_empty(),
        "targets missing from the first {TOP} hits:\n{}",
        misses.join("\n")
    );
}

fn address(hit: &Value) -> String {
    let kind = hit["kind"].as_str().expect("hit has a kind");
    let name = hit["name"].as_str().expect("hit has a name");

    match hit["namespace"].as_str() {
        Some(namespace) => format!("{kind} {namespace}::{name}"),
        None => format!("{kind} {name}"),
    }
}
