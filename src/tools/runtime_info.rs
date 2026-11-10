//! Reading the runtime's own identity out of its metadata.

use anyhow::Context;
use frame_decode::constants::decode_constant;
use rmcp::schemars::{self, JsonSchema};
use scale_decode::{DecodeAsType, IntoVisitor};
use serde::Serialize;
use subxt_metadata::Metadata;

use crate::metadata::MetadataSnapshot;

/// The `System` pallet and the constants read from it.
mod system {
    pub const PALLET: &str = "System";
    pub const VERSION: &str = "Version";
    pub const SS58_PREFIX: &str = "SS58Prefix";
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RuntimeInfo {
    /// Version of the metadata format the node served.
    pub metadata_version: u32,
    /// What the runtime calls itself, e.g. `polkadot`. Fixed across runtime upgrades.
    pub spec_name: String,
    /// Version of the runtime code, bumped by every runtime upgrade.
    pub spec_version: u32,
    /// Version of the call interface; a transaction signed for another one is rejected.
    pub transaction_version: u32,
    /// Prefix the runtime uses to render an account id as an address. Absent when the
    /// runtime does not declare one, or when it could not be read.
    // schemars types an `Option` as `["integer", "null"]`, which some MCP clients reject.
    // Skipped rather than nulled, so the single type is the truthful one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "integer"))]
    pub ss58_prefix: Option<u16>,
}

/// The part of the runtime's `RuntimeVersion` we read.
///
/// Fields are matched against the constant's own type from the registry by name, so
/// the rest of `RuntimeVersion` is ignored and a field renamed upstream turns into a
/// decode error rather than a silently wrong value.
#[derive(DecodeAsType)]
struct RuntimeVersion {
    spec_name: String,
    spec_version: u32,
    transaction_version: u32,
}

pub fn runtime_info(snapshot: &MetadataSnapshot) -> anyhow::Result<RuntimeInfo> {
    let metadata = &snapshot.metadata;

    let version: RuntimeVersion = decode_constant(
        system::PALLET,
        system::VERSION,
        metadata,
        metadata.types(),
        RuntimeVersion::into_visitor(),
    )
    .context("failed to read the runtime version")?;

    Ok(RuntimeInfo {
        metadata_version: snapshot.version,
        spec_name: version.spec_name,
        spec_version: version.spec_version,
        transaction_version: version.transaction_version,
        ss58_prefix: ss58_prefix(metadata),
    })
}

fn ss58_prefix(metadata: &Metadata) -> Option<u16> {
    // Whatever is wrong with the prefix, the rest of the identity is still worth
    // reporting, so this never fails the tool.
    decode_constant(
        system::PALLET,
        system::SS58_PREFIX,
        metadata,
        metadata.types(),
        u16::into_visitor(),
    )
    .ok()
}
