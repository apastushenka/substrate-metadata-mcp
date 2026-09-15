//! Listing the runtime's pallets.

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

use super::{LoadedMetadata, item::summary};

// A struct rather than a bare `Vec`: MCP requires `structuredContent` to be an object.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletList {
    /// Every pallet of the runtime, never truncated, in the order the runtime
    /// declares them -- which is not the order of their indices.
    pub pallets: Vec<PalletEntry>,
}

/// One pallet: the name every other pallet tool takes, and what its own
/// documentation says about it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletEntry {
    /// The pallet's name, exactly as in the metadata. Pass it on verbatim as `pallet` to
    /// `list_pallet_items` and `get_pallet_item`; do not normalize case.
    pub name: String,
    /// The first line of the pallet's own documentation, truncated. Usually absent:
    /// runtimes rarely document pallets in their metadata, so a bare name is normal and
    /// not a sign of a missing pallet.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub summary: Option<String>,
}

pub fn pallets(loaded: &LoadedMetadata) -> PalletList {
    let pallets = loaded
        .metadata
        .pallets()
        .map(|pallet| PalletEntry {
            name: String::from(pallet.name()),
            summary: summary(pallet.docs()),
        })
        .collect();

    PalletList { pallets }
}
