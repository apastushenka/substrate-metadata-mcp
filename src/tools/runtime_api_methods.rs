//! Listing the methods of one runtime API.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::metadata::{
    MetadataSnapshot,
    item::{Item, ItemKind, for_each_item},
    lookup,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeApiMethodsRequest {
    /// The runtime API whose methods to list. `list_runtime_apis` lists the valid names;
    /// case is forgiven, `core` finds `Core`, and the response spells the name as the
    /// metadata does. An unknown runtime API is an error. A pallet name is an error too:
    /// this tool lists runtime API methods only.
    pub runtime_api: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuntimeApiMethods {
    /// Every method of the runtime API, never truncated, in the order it declares them.
    pub methods: Vec<Item>,
}

pub fn runtime_api_methods(
    snapshot: &MetadataSnapshot,
    request: &RuntimeApiMethodsRequest,
) -> anyhow::Result<RuntimeApiMethods> {
    let metadata = &snapshot.metadata;

    // The filter below cannot tell "no methods" from "no such runtime API".
    let runtime_api = lookup::runtime_api(snapshot, &request.runtime_api)?.name();

    let mut methods = Vec::new();

    for_each_item(metadata, |item| {
        if item.namespace == Some(runtime_api) && item.kind == ItemKind::RuntimeApiMethod {
            methods.push(Item::new(&item));
        }
    });

    Ok(RuntimeApiMethods { methods })
}
