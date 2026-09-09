//! The command line: the one surface of this server that lives in a user's config
//! file, so it changes rarely and deliberately.

use clap::Parser;
use subxt_utils_fetchmetadata::Url;

/// Read-only index of a Substrate runtime's metadata.
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    /// Node RPC endpoint to fetch metadata from (ws, wss, http, https).
    #[arg(long, value_name = "URL", value_parser = endpoint)]
    pub url: Url,
}

/// Parse the process arguments, or exit with clap's own message and status.
///
/// Keeping this here rather than calling `Cli::parse()` from `main` is what confines
/// the argument parser to this module: nothing else in the crate names clap.
pub fn parse() -> Cli {
    Cli::parse()
}

const SUPPORTED_SCHEMES: [&str; 4] = ["ws", "wss", "http", "https"];

fn endpoint(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|err| err.to_string())?;

    if !SUPPORTED_SCHEMES.contains(&url.scheme()) {
        return Err(format!(
            "unsupported scheme {:?} (expected one of: {})",
            url.scheme(),
            SUPPORTED_SCHEMES.join(", ")
        ));
    }

    Ok(url)
}
