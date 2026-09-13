//! Listing the methods of one runtime API.

use anyhow::Context;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use super::{
    LoadedMetadata,
    item::{Item, ItemKind, for_each_item, resolve_name},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RuntimeApiMethodsRequest {
    /// The runtime API whose methods to list. `namespaces` lists the valid names;
    /// case is forgiven, `core` finds `Core`, and the response spells the name as the
    /// metadata does. An unknown runtime API is an error.
    pub runtime_api: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuntimeApiMethods {
    /// Every method of the runtime API, never truncated, in the order it declares them.
    pub methods: Vec<Item>,
}

pub fn runtime_api_methods(
    loaded: &LoadedMetadata,
    request: &RuntimeApiMethodsRequest,
) -> anyhow::Result<RuntimeApiMethods> {
    let metadata = &loaded.metadata;

    // The filter below cannot tell "no methods" from "no such runtime API".
    let runtime_api = resolve_name(
        &request.runtime_api,
        metadata.runtime_api_traits().map(|api| api.name()),
    )?
    .with_context(|| {
        format!(
            "no runtime API named `{}`; `namespaces` lists the valid names",
            request.runtime_api
        )
    })?;

    let mut methods = Vec::new();

    for_each_item(metadata, |item| {
        if item.namespace == Some(runtime_api) && item.kind == ItemKind::RuntimeApiMethod {
            methods.push(Item::new(&item));
        }
    });

    Ok(RuntimeApiMethods { methods })
}
