//! Fetching and decoding runtime metadata, and reading facts out of it.

mod runtime;
mod search;

use anyhow::Context;
use frame_metadata::{META_RESERVED, RuntimeMetadataPrefixed};
use subxt_metadata::{Metadata, decode_runtime_metadata};
use subxt_utils_fetchmetadata::{MetadataVersion, Url, from_url};

pub use self::{
    runtime::{RuntimeInfo, runtime_info},
    search::{SearchRequest, SearchResults, search},
};

/// `Metadata` does not keep the version of the format it was decoded from, so [`load`]
/// records it alongside.
pub struct LoadedMetadata {
    pub metadata: Metadata,
    pub version: u32,
}

/// Fetch and decode the metadata.
pub async fn load(url: &Url) -> anyhow::Result<LoadedMetadata> {
    // TODO: `Latest` takes the node's newest version, which a future v17 would make
    // undecodable here -- and indistinguishable from corrupt bytes, since both fail as
    // an unknown enum variant. Fall back to the newest of `SUPPORTED_METADATA_VERSIONS`.
    let bytes = from_url(url.clone(), MetadataVersion::Latest, None)
        .await
        .with_context(|| format!("failed to fetch metadata from {url}"))?;

    let runtime_metadata = decode_runtime_metadata(&bytes).context("failed to decode metadata")?;

    let version = runtime_metadata.version();

    let metadata = Metadata::try_from(RuntimeMetadataPrefixed(META_RESERVED, runtime_metadata))
        .with_context(|| format!("failed to convert metadata v{version}"))?;

    Ok(LoadedMetadata { metadata, version })
}
