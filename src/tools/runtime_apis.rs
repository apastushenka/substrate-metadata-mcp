//! Listing the runtime's runtime APIs.

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

use crate::metadata::{MetadataSnapshot, item::summary};

// A struct rather than a bare `Vec`: MCP requires `structuredContent` to be an object.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RuntimeApiList {
    /// Every runtime API of the runtime, never truncated, in the order the runtime
    /// declares them. Empty under metadata v14, which carries no runtime APIs at all.
    pub runtime_apis: Vec<RuntimeApiEntry>,
}

/// One runtime API: a group of host-callable methods, and what its own documentation
/// says about the group.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RuntimeApiEntry {
    /// The runtime API's name, exactly as in the metadata. Pass it on verbatim as
    /// `runtime_api` to `list_runtime_api_methods` and `get_runtime_api_method`; do not
    /// normalize case.
    pub name: String,
    /// The first line of the runtime API's own documentation, truncated. Absent when
    /// the runtime API has no documentation of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub summary: Option<String>,
}

pub fn runtime_apis(snapshot: &MetadataSnapshot) -> RuntimeApiList {
    let runtime_apis = snapshot
        .metadata
        .runtime_api_traits()
        .map(|api| RuntimeApiEntry {
            name: String::from(api.name()),
            summary: summary(api.docs()),
        })
        .collect();

    RuntimeApiList { runtime_apis }
}
