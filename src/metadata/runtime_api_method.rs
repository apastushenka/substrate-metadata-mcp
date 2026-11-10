//! Reading one method of a runtime API.

use anyhow::anyhow;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use super::{
    LoadedMetadata,
    item::docs,
    lookup::{self, runtime_api_method_lookup_error_message},
    registry::{Field, type_name},
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeApiMethodAddress {
    /// The runtime API the method belongs to. `list_runtime_apis` lists the valid names;
    /// take a runtime API's `name` from there, or a method's `namespace` from `search` or
    /// `list_runtime_api_methods`, as `runtime_api`. Case is forgiven, `core` finds
    /// `Core`, and the response spells the name as the metadata does. A pallet name is
    /// an error: this tool reads runtime API methods only.
    pub runtime_api: String,

    /// The method's own name, exactly as `search` and `list_runtime_api_methods` return
    /// it in `name` -- without the runtime API. Pass it on verbatim; do not qualify it
    /// (`Core::version` belongs in two fields, not one). Case is forgiven. An unknown
    /// name is an error.
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuntimeApiMethodDetails {
    /// The runtime API the method belongs to, spelled as the metadata does -- not as
    /// the request did.
    pub runtime_api: String,

    /// The method's own name, spelled as the metadata does -- not as the request did.
    pub name: String,

    /// The method's own documentation, uncut, one string per line, with the leading
    /// space FRAME leaves after `///` stripped. Empty when the method has none. Unlike
    /// `search` and `list_runtime_api_methods`, this is not a summary and is not
    /// truncated.
    pub docs: Vec<String>,

    /// The method's arguments, in the order the runtime API declares them. Each has a
    /// name and a type from the registry; their `docs` are always empty -- metadata does
    /// not document runtime API parameters.
    pub arguments: Vec<Field>,

    /// The method's return type, rendered from the registry the same way as each
    /// argument's `type`: a primitive (`u32`), a sequence (`Vec<u32>`), a named type
    /// with its parameters (`Result<RuntimeVersion, DispatchError>`), and so on. The
    /// last path segment only, not the module path.
    pub returns: String,
}

pub fn runtime_api_method(
    loaded: &LoadedMetadata,
    address: &RuntimeApiMethodAddress,
) -> anyhow::Result<RuntimeApiMethodDetails> {
    let (runtime_api, method) =
        lookup::runtime_api_method(&loaded.metadata, &address.runtime_api, &address.name).map_err(
            |error| {
                anyhow!(runtime_api_method_lookup_error_message(
                    &error,
                    loaded.version
                ))
            },
        )?;

    let types = loaded.metadata.types();

    let arguments = method
        .inputs()
        .map(|input| Field::from_runtime_api_input(input, types))
        .collect();

    let returns = type_name(method.output_ty(), types);

    Ok(RuntimeApiMethodDetails {
        runtime_api: String::from(runtime_api.name()),
        name: String::from(method.name()),
        docs: docs(method.docs()),
        arguments,
        returns,
    })
}
