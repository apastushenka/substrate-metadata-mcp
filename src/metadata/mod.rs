//! Fetching and decoding runtime metadata, and reading facts out of it.

use anyhow::{Context, anyhow};
use subxt_metadata::Metadata;
use subxt_utils_fetchmetadata::{MetadataVersion, Url, from_url};

pub async fn load(url: &Url) -> anyhow::Result<Metadata> {
    let bytes = from_url(url.clone(), MetadataVersion::Latest, None)
        .await
        .with_context(|| format!("failed to fetch metadata from {url}"))?;

    Metadata::decode_from(&bytes)
        .map_err(|err| anyhow!("failed to decode metadata from {url}: {err}"))
}
