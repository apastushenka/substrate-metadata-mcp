//! Fetching and decoding runtime metadata, and what the tools read it with: the walk over
//! pallet and runtime API items, their documentation, type names and values, and name
//! lookup.

pub mod docs;
pub mod item;
pub mod lookup;
pub mod registry;

use std::{fmt, path::PathBuf};

use anyhow::{Context, bail};
use frame_metadata::{META_RESERVED, RuntimeMetadataPrefixed};
use parity_scale_codec::Decode;
use subxt_metadata::{Metadata, SUPPORTED_METADATA_VERSIONS};
use subxt_utils_fetchmetadata::{MetadataVersion, Url, from_url};

#[derive(Clone)]
pub enum Source {
    Url(Url),
    File(PathBuf),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(url) => write!(f, "{url}"),
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// `Metadata` does not keep the version of the format it was decoded from, so the
/// snapshot records it alongside.
pub struct MetadataSnapshot {
    pub metadata: Metadata,
    pub version: u32,
}

pub async fn load(source: &Source) -> anyhow::Result<MetadataSnapshot> {
    let bytes = match source {
        Source::Url(url) => fetch(url).await?,
        Source::File(path) => tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?,
    };

    decode(&bytes).with_context(|| format!("failed to load metadata from {source}"))
}

async fn fetch(url: &Url) -> anyhow::Result<Vec<u8>> {
    // TODO: `Latest` takes the node's newest version, and `decode` rejects a future v17 as
    // unsupported. Fall back to fetching the newest of `SUPPORTED_METADATA_VERSIONS`.
    from_url(url.clone(), MetadataVersion::Latest, None)
        .await
        .with_context(|| format!("failed to fetch metadata from {url}"))
}

/// `bytes` are a SCALE-encoded `RuntimeMetadataPrefixed`, as `from_url` returns them and
/// `subxt metadata` writes them: the `meta` prefix, the version, then the metadata itself.
fn decode(bytes: &[u8]) -> anyhow::Result<MetadataSnapshot> {
    // Without these checks, a file that is not metadata and an unsupported version both fail
    // with the same "Could not decode `RuntimeMetadata`, variant doesn't exist".
    let Some(&version) = bytes
        .strip_prefix(&META_RESERVED.to_le_bytes())
        .and_then(<[u8]>::first)
    else {
        bail!(
            "expected SCALE-encoded metadata starting with `meta`, the default output of \
             `subxt metadata`"
        );
    };

    let version = u32::from(version);

    if !SUPPORTED_METADATA_VERSIONS.contains(&version) {
        let mut supported = SUPPORTED_METADATA_VERSIONS;
        supported.sort_unstable();
        let supported = supported.map(|version| format!("v{version}")).join(", ");

        bail!("metadata v{version} is not supported (supported: {supported})");
    }

    // Not `Metadata::decode_from` or `decode_runtime_metadata`: on some inputs they recurse
    // until the stack overflows.
    let prefixed = RuntimeMetadataPrefixed::decode(&mut &*bytes)
        .with_context(|| format!("failed to decode metadata v{version}"))?;

    let metadata = Metadata::try_from(prefixed)
        .with_context(|| format!("failed to convert metadata v{version}"))?;

    Ok(MetadataSnapshot { metadata, version })
}

#[cfg(test)]
mod tests {
    use frame_metadata::META_RESERVED;

    use super::decode;

    fn prefixed(rest: &[u8]) -> Vec<u8> {
        [&META_RESERVED.to_le_bytes()[..], rest].concat()
    }

    fn decode_error(bytes: &[u8]) -> String {
        match decode(bytes) {
            Ok(_) => panic!("decoded {bytes:?}"),
            Err(err) => format!("{err:#}"),
        }
    }

    // The other two formats of `subxt metadata`, JSON and hex, the `Option` that wraps the
    // metadata in a `state_call` result, a file cut short after `meta`, and an empty one.
    #[test]
    fn decode_rejects_what_is_not_metadata() {
        for bytes in [
            &b"[\n  1635018093,"[..],
            b"0x6d657461",
            &[1, 0xb6],
            &META_RESERVED.to_le_bytes(),
            b"",
        ] {
            let message = decode_error(bytes);

            assert!(message.starts_with("expected SCALE-encoded"), "{message}");
        }
    }

    #[test]
    fn decode_names_an_unsupported_version() {
        for version in [17, 13] {
            let message = decode_error(&prefixed(&[version]));

            assert_eq!(
                message,
                format!("metadata v{version} is not supported (supported: v14, v15, v16)")
            );
        }
    }

    #[test]
    fn decode_names_the_version_of_corrupt_metadata() {
        let message = decode_error(&prefixed(&[15, 0xff]));

        assert!(
            message.starts_with("failed to decode metadata v15: "),
            "{message}"
        );
    }
}
