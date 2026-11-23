//! The server binary on a fixture, called the way a client calls it.
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

pub type Client = RunningService<RoleClient, ClientConfig>;

pub async fn connect(fixture: &str) -> Client {
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

/// The answer of `tool`, as the JSON a client gets. A tool error fails the test.
pub async fn call(client: &Client, tool: &'static str, arguments: Value) -> Value {
    let params = CallToolRequestParams::new(tool).with_arguments(object(arguments.clone()));
    let result = client.call_tool(params).await.expect("the tool answers");

    assert!(
        result.is_error != Some(true),
        "{tool} {arguments} failed: {}",
        json!(result.content)
    );

    result.structured_content.expect("the tool returns JSON")
}
