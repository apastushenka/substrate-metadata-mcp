mod cli;
mod metadata;
mod server;

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
    let cli = cli::parse();
    let server = MetadataServer::new(cli.url);

    // Preload metadata
    tokio::spawn({
        let server = server.clone();

        async move {
            if let Err(err) = server.metadata().await {
                eprintln!("{}: {err:#}", env!("CARGO_PKG_NAME"));
            }
        }
    });

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
