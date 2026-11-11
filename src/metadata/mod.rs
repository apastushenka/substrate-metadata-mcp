//! Fetching and decoding runtime metadata, and what the tools read it with: the walk over
//! pallet and runtime API items, their documentation, type names and values, and name
//! lookup.

pub mod docs;
pub mod item;
pub mod lookup;
pub mod registry;

use anyhow::Context;
use frame_metadata::{META_RESERVED, RuntimeMetadataPrefixed};
use subxt_metadata::{Metadata, decode_runtime_metadata};
use subxt_utils_fetchmetadata::{MetadataVersion, Url, from_url};

/// `Metadata` does not keep the version of the format it was decoded from, so the
/// snapshot records it alongside.
pub struct MetadataSnapshot {
    pub metadata: Metadata,
    pub version: u32,
}

pub async fn load(url: &Url) -> anyhow::Result<MetadataSnapshot> {
    let bytes = fetch(url).await?;

    decode(&bytes)
}

async fn fetch(url: &Url) -> anyhow::Result<Vec<u8>> {
    // TODO: `Latest` takes the node's newest version, which a future v17 would make
    // undecodable here -- and indistinguishable from corrupt bytes, since both fail as
    // an unknown enum variant. Fall back to the newest of `SUPPORTED_METADATA_VERSIONS`.
    from_url(url.clone(), MetadataVersion::Latest, None)
        .await
        .with_context(|| format!("failed to fetch metadata from {url}"))
}

fn decode(bytes: &[u8]) -> anyhow::Result<MetadataSnapshot> {
    let runtime_metadata = decode_runtime_metadata(bytes).context("failed to decode metadata")?;

    let version = runtime_metadata.version();

    let metadata = Metadata::try_from(RuntimeMetadataPrefixed(META_RESERVED, runtime_metadata))
        .with_context(|| format!("failed to convert metadata v{version}"))?;

    Ok(MetadataSnapshot { metadata, version })
}
