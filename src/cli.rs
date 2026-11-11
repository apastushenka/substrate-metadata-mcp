//! The command line: the one surface of this server that lives in a user's config
//! file, so it changes rarely and deliberately.

use std::path::PathBuf;

use clap::{Args, Parser};
use subxt_utils_fetchmetadata::Url;

use crate::metadata::Source;

/// Read-only index of a Substrate runtime's metadata.
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(flatten)]
    source: SourceArgs,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct SourceArgs {
    /// Node RPC endpoint to fetch metadata from (ws, wss, http, https).
    #[arg(long, value_name = "URL", value_parser = endpoint)]
    url: Option<Url>,

    /// File to read metadata from, in the SCALE format `subxt metadata` writes by default.
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
}

impl Cli {
    pub fn source(self) -> Source {
        match (self.source.url, self.source.file) {
            (Some(url), None) => Source::Url(url),
            (None, Some(path)) => Source::File(path),
            // The group on `SourceArgs` admits exactly one of the two.
            _ => unreachable!(),
        }
    }
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

#[cfg(test)]
mod tests {
    use std::{iter, path::Path};

    use clap::{Parser, error::ErrorKind};

    use super::Cli;
    use crate::metadata::Source;

    fn parse(args: &[&str]) -> Result<Source, clap::Error> {
        Cli::try_parse_from(iter::once("substrate-metadata-mcp").chain(args.iter().copied()))
            .map(Cli::source)
    }

    fn rejection(args: &[&str]) -> ErrorKind {
        parse(args).err().expect("arguments accepted").kind()
    }

    #[test]
    fn parse_accepts_url_alone() {
        let Ok(Source::Url(url)) = parse(&["--url", "wss://rpc.polkadot.io"]) else {
            panic!("not a URL source");
        };

        assert_eq!(url.as_str(), "wss://rpc.polkadot.io/");
    }

    #[test]
    fn parse_keeps_file_path_as_given() {
        let Ok(Source::File(path)) = parse(&["--file", "polkadot.scale"]) else {
            panic!("not a file source");
        };

        assert_eq!(path, Path::new("polkadot.scale"));
    }

    #[test]
    fn parse_rejects_url_and_file_together() {
        let args = ["--url", "wss://rpc.polkadot.io", "--file", "polkadot.scale"];

        assert_eq!(rejection(&args), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn parse_requires_source() {
        assert_eq!(rejection(&[]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_rejects_url_of_other_scheme() {
        assert_eq!(
            rejection(&["--url", "ftp://example.com"]),
            ErrorKind::ValueValidation
        );
    }
}
