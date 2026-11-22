//! Search on real metadata, through the server binary, the way a client talks to it.
//!
//! The fixtures are the metadata of Polkadot and of Polkadot Asset Hub, both at spec 2005003.
//! They stay as they are, so that a runtime upgrade cannot move these tests.
//!
//! The package leaves tests/ out (`exclude` in Cargo.toml): the fixtures outweigh the crate
//! several times over, and a test inside src/ would ship without them and fail there.

use rmcp::{
    RoleClient,
    model::{CallToolRequestParams, ClientConfig, object},
    serde_json::{Value, json},
    serve_client,
    service::RunningService,
    transport::TokioChildProcess,
};
use tokio::process::Command;

type Client = RunningService<RoleClient, ClientConfig>;

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
                query: "Staking BondingDuration SessionsPerEra",
                kinds: &["constant"],
                targets: &[
                    &["constant Staking::BondingDuration"],
                    &["constant Staking::SessionsPerEra"],
                ],
            },
            Case {
                query: "ErasStakers ErasValidatorPrefs exposure commission",
                kinds: &[],
                targets: &[
                    &[
                        "storage Staking::ErasStakersOverview",
                        "storage Staking::ErasStakersPaged",
                    ],
                    &["storage Staking::ErasValidatorPrefs"],
                ],
            },
            Case {
                query: "Rewarded payout DOT staking",
                kinds: &[],
                targets: &[&["event Staking::Rewarded", "call Staking::payout_stakers"]],
            },
            Case {
                query: "swap_exact_tokens_for_tokens keep_alive",
                kinds: &[],
                targets: &[&["call AssetConversion::swap_exact_tokens_for_tokens"]],
            },
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

// A hit need not have every word of the query, and `missing` names the words it lacks.
#[tokio::test]
async fn partial_hits_list_missing_words() {
    let client = connect("polkadot.scale").await;
    let content = search(&client, "transfer allow death", &["call"]).await;

    client.cancel().await.expect("server stops");

    let missing_of = |target: &str| {
        content["hits"]
            .as_array()
            .expect("search returns hits")
            .iter()
            .find(|hit| address(hit) == target)
            .map(|hit| hit["missing"].clone())
    };

    assert_eq!(
        missing_of("call Balances::transfer_allow_death"),
        Some(json!([]))
    );
    assert_eq!(
        missing_of("call OnDemand::place_order_allow_death"),
        Some(json!(["transfer"]))
    );
}

async fn assert_targets_found(fixture: &str, cases: &[Case]) {
    let client = connect(fixture).await;
    let mut misses = Vec::new();

    for case in cases {
        let content = search(&client, case.query, case.kinds).await;
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

async fn connect(fixture: &str) -> Client {
    let mut command = Command::new(env!("CARGO_BIN_EXE_substrate-metadata-mcp"));
    command.arg("--file").arg(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ));

    let transport = TokioChildProcess::new(command).expect("server starts");

    serve_client(ClientConfig::default(), transport)
        .await
        .expect("server initializes")
}

/// The first `TOP` hits, as the JSON a client gets.
async fn search(client: &Client, query: &str, kinds: &[&str]) -> Value {
    let arguments = json!({ "query": query, "kinds": kinds, "limit": TOP });
    let params = CallToolRequestParams::new("search").with_arguments(object(arguments));
    let result = client.call_tool(params).await.expect("search answers");

    assert!(
        result.is_error != Some(true),
        "search {query:?} failed: {}",
        json!(result.content)
    );

    result.structured_content.expect("search returns JSON")
}

fn address(hit: &Value) -> String {
    let kind = hit["kind"].as_str().expect("hit has a kind");
    let name = hit["name"].as_str().expect("hit has a name");

    match hit["namespace"].as_str() {
        Some(namespace) => format!("{kind} {namespace}::{name}"),
        None => format!("{kind} {name}"),
    }
}
