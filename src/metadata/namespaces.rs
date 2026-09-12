//! Listing the containers of the metadata: pallets and runtime API traits.

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

use super::{
    LoadedMetadata,
    item::{Item, for_each_item},
};

// A struct rather than a bare `Vec`: MCP requires `structuredContent` to be an object.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NamespaceList {
    /// All of them, never truncated: pallets first, then runtime API traits, each
    /// group in the order the runtime declares it. A pallet's `summary` is usually
    /// absent -- runtimes rarely document pallets in their metadata -- so a bare name
    /// is normal, not a sign of a missing pallet.
    pub namespaces: Vec<Item>,
}

pub fn namespaces(loaded: &LoadedMetadata) -> NamespaceList {
    let mut namespaces = Vec::new();

    // Walking every item to collect a few dozen costs the same milliseconds as a direct pass
    // over `pallets()` and `runtime_api_traits()`, and keeps the one place that knows
    // how a pallet becomes an `Item`.
    for_each_item(&loaded.metadata, |item| {
        // Containers are the items without a namespace.
        if item.namespace.is_none() {
            namespaces.push(Item::new(&item));
        }
    });

    NamespaceList { namespaces }
}
