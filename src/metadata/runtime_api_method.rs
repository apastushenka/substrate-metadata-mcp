//! Reading one method of a runtime API.

use anyhow::{Context, bail};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use super::{
    LoadedMetadata,
    item::{docs, resolve_name, split_qualified_name},
    registry::{Field, type_name},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RuntimeApiMethodAddress {
    /// The runtime API the method belongs to. `list_namespaces` lists the valid names;
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
    let metadata = &loaded.metadata;

    // The listings hand back `namespace` and `name` separately, so a qualified name is
    // already split somewhere: hand the split back rather than accept a second spelling.
    if let Some((qualifier, method)) = split_qualified_name(&address.name) {
        bail!(
            "`name` is the method's own name: pass `{qualifier}` as `runtime_api` and `{method}` as `name`"
        );
    }

    let runtime_api = resolve_runtime_api_name(loaded, &address.runtime_api)?;
    let api = metadata
        .runtime_api_trait_by_name(runtime_api)
        .expect("resolve_runtime_api_name selected this runtime API");

    let method_name = resolve_name(&address.name, api.methods().map(|method| method.name()))?
        .with_context(|| {
            format!(
                "no method named `{}` in runtime API `{runtime_api}`; `list_runtime_api_methods` lists them",
                address.name
            )
        })?;

    let method = api
        .method_by_name(method_name)
        .expect("resolve_name selected this method");

    let types = metadata.types();

    let arguments = method
        .inputs()
        .map(|input| Field::from_runtime_api_input(input, types))
        .collect();

    let returns = type_name(method.output_ty(), types);

    Ok(RuntimeApiMethodDetails {
        runtime_api: String::from(api.name()),
        name: String::from(method.name()),
        docs: docs(method.docs()),
        arguments,
        returns,
    })
}

fn resolve_runtime_api_name<'a>(loaded: &'a LoadedMetadata, name: &str) -> anyhow::Result<&'a str> {
    let metadata = &loaded.metadata;

    if let Some(runtime_api) =
        resolve_name(name, metadata.runtime_api_traits().map(|api| api.name()))?
    {
        return Ok(runtime_api);
    }

    if metadata.runtime_api_traits().len() == 0 && loaded.version < 15 {
        bail!(
            "runtime APIs need metadata v15; this metadata is v{}",
            loaded.version
        );
    }

    if let Some(pallet) = resolve_name(name, metadata.pallets().map(|pallet| pallet.name()))? {
        bail!("`{pallet}` is a pallet, not a runtime API; `get_pallet_item` reads its items");
    }

    bail!("no runtime API named `{name}`; `list_namespaces` lists the valid names");
}
