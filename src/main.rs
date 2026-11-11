mod cli;
mod metadata;
mod server;
mod tools;

use std::process::ExitCode;

use rmcp::{ServiceExt, transport::stdio};

use crate::server::MetadataServer;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err:#}", env!("CARGO_PKG_NAME"));
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let server = MetadataServer::new(cli::parse().source());
    server.prepare().await?;

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
