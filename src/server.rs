//! The MCP surface: the server type, and the tools declared against it.

use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use subxt_utils_fetchmetadata::Url;
use tokio::sync::OnceCell;

use crate::metadata::{
    self, LoadedMetadata, PalletItemAddress, PalletItemDetails, PalletItems, PalletItemsRequest,
    PalletList, RuntimeApiList, RuntimeApiMethodAddress, RuntimeApiMethodDetails,
    RuntimeApiMethods, RuntimeApiMethodsRequest, RuntimeInfo, SearchRequest, SearchResults,
};

#[derive(Clone)]
pub struct MetadataServer {
    url: Url,
    metadata: Arc<OnceCell<LoadedMetadata>>,
}

// Every tool declares `read_only_hint` true and `open_world_hint` false. The server does
// reach the network: `metadata` fetches from the node, and retries inside a tool call
// whenever the startup preload failed. The world stays closed because the endpoint is
// fixed by `--url` and no tool argument can widen it. A tool that took a URL would have
// to flip `open_world_hint`.
#[tool_router]
impl MetadataServer {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            metadata: Arc::new(OnceCell::new()),
        }
    }

    /// The metadata, fetched on first use.
    pub async fn metadata(&self) -> anyhow::Result<&LoadedMetadata> {
        self.metadata
            .get_or_try_init(|| async {
                let loaded = metadata::load(&self.url).await?;

                eprintln!(
                    "{}: loaded metadata from {}",
                    env!("CARGO_PKG_NAME"),
                    self.url
                );

                anyhow::Ok(loaded)
            })
            .await
    }

    /// Identify the runtime this server indexes: its spec name, its spec and transaction
    /// versions, the SS58 prefix it renders account addresses with, and the version of the
    /// metadata format it was decoded from. The SS58 prefix is missing when the runtime
    /// declares none or it could not be read.
    ///
    /// The metadata version gates what every other tool can show: a kind the format
    /// predates has no items anywhere, so a listing of it comes back empty and reading one
    /// fails with a message naming the version it needs.
    ///
    /// All of it is read from the metadata itself, so it describes the runtime the
    /// metadata was fetched from, not the live state of the chain.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_runtime_info(&self) -> Result<Json<RuntimeInfo>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::runtime_info(loaded).map(Json).map_err(tool_error)
    }

    /// List every pallet of the runtime, never truncated. Almost everything a runtime does
    /// lives in a pallet.
    ///
    /// An entry names a pallet and carries its `index`. A `summary` is usually missing:
    /// runtimes rarely document a pallet at all, so a bare name is normal.
    ///
    /// `counts` says how many items of each kind the pallet holds, so a listing can be
    /// sized before it is requested. A zero there means there is nothing to list, whether
    /// the pallet declares none of that kind or the metadata version predates it.
    ///
    /// A `ModuleError` identifies its pallet by `index` and never by name. No other tool
    /// maps that byte back to a name, so look it up in this listing.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_pallets(&self) -> Result<Json<PalletList>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        Ok(Json(metadata::pallets(loaded)))
    }

    /// List every runtime API of the runtime, never truncated. Each one is a group of
    /// host-callable methods: `state_call` invokes them, and polkadot-js lists them as
    /// runtime calls. An entry names one and gives the first line of its documentation
    /// when it has one.
    ///
    /// The list is empty under metadata v14, which carries no runtime APIs at all.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_runtime_apis(&self) -> Result<Json<RuntimeApiList>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        Ok(Json(metadata::runtime_apis(loaded)))
    }

    /// List what one pallet contains, paged and never truncated. `search` ranks and
    /// truncates instead. An entry names an item and gives the first line of its
    /// documentation, not its contents.
    ///
    /// `total` counts the whole listing, before `offset` and `limit`: there is more
    /// whenever `offset` plus the number of items returned is less than `total`.
    ///
    /// Items are grouped by kind, and within a kind they follow the order the pallet
    /// declares them. A page of mixed kinds can therefore end in the middle of one, and
    /// nothing marks a kind as finished. Pass `kinds` to retrieve one kind in full.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_pallet_items(
        &self,
        Parameters(request): Parameters<PalletItemsRequest>,
    ) -> Result<Json<PalletItems>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::pallet_items(loaded, &request)
            .map(Json)
            .map_err(tool_error)
    }

    /// List every method of one runtime API, never truncated. `search` ranks and truncates
    /// instead. An entry names a method and gives the first line of its documentation, not
    /// its signature.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_runtime_api_methods(
        &self,
        Parameters(request): Parameters<RuntimeApiMethodsRequest>,
    ) -> Result<Json<RuntimeApiMethods>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::runtime_api_methods(loaded, &request)
            .map(Json)
            .map_err(tool_error)
    }

    /// Read one method of a runtime API in full: its complete documentation and its
    /// signature. Only this tool reports the signature, the type of every argument and of
    /// the return value.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_runtime_api_method(
        &self,
        Parameters(address): Parameters<RuntimeApiMethodAddress>,
    ) -> Result<Json<RuntimeApiMethodDetails>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::runtime_api_method(loaded, &address)
            .map(Json)
            .map_err(tool_error)
    }

    /// Read one item of a pallet in full: its complete documentation and the fields that
    /// its `kind` determines.
    ///
    /// Only this tool reports those fields: the SCALE indices that identify a call, an
    /// event or an error, the hashers of a storage entry's keys, a constant's decoded
    /// value. The one exception is `pallet_index`, the byte the pallet listing already
    /// carries as `index`. The second index, which selects the variant within the pallet,
    /// comes from here alone; it is the codec index (`#[codec(index = N)]`), not a
    /// position in any listing, and FRAME leaves gaps in it.
    ///
    /// A storage entry carries `default` exactly when `optional` is false, so a missing
    /// `default` is expected rather than an error. A call has no return type in the
    /// metadata, so no tool reports one.
    ///
    /// A constant's `value` and a storage entry's `default` are scale-value text, not
    /// JSON: `None ()` is the absent variant, a single parenthesised number is a one-field
    /// wrapper rather than a number, and a byte array such as an `AccountId32` is a list
    /// of decimal numbers, not hex. The exception is a `u256` or an `i256`, which has no
    /// text form and is returned as hex.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_pallet_item(
        &self,
        Parameters(address): Parameters<PalletItemAddress>,
    ) -> Result<Json<PalletItemDetails>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::pallet_item(loaded, &address)
            .map(Json)
            .map_err(tool_error)
    }

    /// Find any item of the metadata whose name or documentation matches a query. The
    /// runtime's metadata is far too large to read whole, and this is how to locate
    /// something in it. A hit names an item and gives the first line of its documentation,
    /// not its contents.
    ///
    /// Use it when you do not know where something is. If you already know the name, read
    /// that item directly; to see everything one pallet holds, list it. Both are complete,
    /// while this is ranked and truncated.
    ///
    /// There is no paging: hits beyond `limit` cannot be retrieved. `total` counts every
    /// match, so a `total` larger than the number of hits means the result is incomplete;
    /// narrow the query rather than repeat it.
    ///
    /// Every word of the query must match for an item to be returned, so `unmatched`
    /// explains an empty result. A hit whose `matched_in` is `docs` matched only the
    /// documentation: check it rather than trust it.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    async fn search(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<SearchResults>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::search(loaded, &request)
            .map(Json)
            .map_err(tool_error)
    }
}

#[tool_handler]
impl ServerHandler for MetadataServer {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder().enable_tools().build();

        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(format!(
                "Read-only index of the Substrate runtime metadata at {}, \
                 a snapshot loaded once and never refreshed. Covers pallets \
                 with their calls, storage, events, errors, constants and \
                 view functions; runtime APIs with their methods. Not \
                 a chain client: no account state, no blocks, no extrinsic \
                 submission.",
                self.url
            ))
    }
}

/// A failed tool call, as the content of a successful response.
fn tool_error(err: anyhow::Error) -> String {
    format!("{err:#}")
}

#[cfg(test)]
mod tests {
    use rmcp::serde_json::{Map, Value};

    use super::MetadataServer;

    fn for_each_schema(mut visit: impl FnMut(&str, &Map<String, Value>)) {
        for tool in MetadataServer::tool_router().list_all() {
            visit(&format!("{}.input", tool.name), &tool.input_schema);

            let Some(output) = tool.output_schema.as_deref() else {
                continue;
            };

            visit(&format!("{}.output", tool.name), output);
        }
    }

    fn for_each_subschema(
        schema: &Map<String, Value>,
        path: &str,
        visit: &mut impl FnMut(&str, &Map<String, Value>),
    ) {
        visit(path, schema);

        for key in ["properties", "patternProperties", "$defs", "definitions"] {
            let Some(members) = schema.get(key).and_then(Value::as_object) else {
                continue;
            };

            for (name, child) in members {
                let Some(child) = child.as_object() else {
                    continue;
                };

                for_each_subschema(child, &format!("{path}/{key}/{name}"), visit);
            }
        }

        for key in ["items", "additionalProperties", "not", "if", "then", "else"] {
            match schema.get(key) {
                Some(Value::Object(child)) => {
                    for_each_subschema(child, &format!("{path}/{key}"), visit);
                }
                Some(Value::Array(children)) => {
                    for (i, child) in children.iter().enumerate() {
                        let Some(child) = child.as_object() else {
                            continue;
                        };

                        for_each_subschema(child, &format!("{path}/{key}/{i}"), visit);
                    }
                }
                _ => {}
            }
        }

        for key in ["prefixItems", "anyOf", "oneOf", "allOf"] {
            let Some(children) = schema.get(key).and_then(Value::as_array) else {
                continue;
            };

            for (i, child) in children.iter().enumerate() {
                let Some(child) = child.as_object() else {
                    continue;
                };

                for_each_subschema(child, &format!("{path}/{key}/{i}"), visit);
            }
        }
    }

    fn assert_members_have_descriptions(schema: &Map<String, Value>, path: &str, key: &str) {
        let Some(members) = schema.get(key).and_then(Value::as_object) else {
            return;
        };

        for (name, member) in members {
            let description = member
                .as_object()
                .and_then(|member| member.get("description"))
                .and_then(Value::as_str);

            // Internally tagged unions get a `kind` property with `const` and no
            // description: the constant names the branch, and there is nothing else to
            // say.
            if member
                .as_object()
                .is_some_and(|member| member.contains_key("const"))
            {
                continue;
            }

            assert!(
                description.is_some_and(|text| !text.is_empty()),
                "{path}/{key}/{name}: missing description"
            );
        }
    }

    #[test]
    fn tools_declare_read_only_and_no_open_world() {
        for tool in MetadataServer::tool_router().list_all() {
            let annotations = tool.annotations.as_ref();

            assert_eq!(
                annotations.and_then(|annotations| annotations.read_only_hint),
                Some(true),
                "{}: read_only_hint is not true",
                tool.name
            );

            assert_eq!(
                annotations.and_then(|annotations| annotations.open_world_hint),
                Some(false),
                "{}: open_world_hint is not false",
                tool.name
            );
        }
    }

    #[test]
    fn tools_have_output_schema() {
        for tool in MetadataServer::tool_router().list_all() {
            assert!(
                tool.output_schema.is_some(),
                "{}: no output_schema",
                tool.name
            );
        }
    }

    // A request type without `deny_unknown_fields` drops a field the model made up without a
    // word, and the model goes on to trust a filter that never ran.
    #[test]
    fn tools_reject_undeclared_arguments() {
        for tool in MetadataServer::tool_router().list_all() {
            let takes_arguments = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|properties| !properties.is_empty());

            if !takes_arguments {
                continue;
            }

            assert_eq!(
                tool.input_schema.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "{}: accepts arguments it does not declare",
                tool.name
            );
        }
    }

    #[test]
    fn option_fields_have_single_type() {
        for_each_schema(|path, schema| {
            for_each_subschema(schema, path, &mut |path, schema| {
                if schema.get("type").is_some_and(Value::is_array) {
                    panic!("{path}: type is a union, must be a single word");
                }
            });
        });
    }

    #[test]
    fn schemas_have_no_null() {
        for_each_schema(|path, schema| {
            for_each_subschema(schema, path, &mut |path, schema| {
                assert_ne!(
                    schema.get("type").and_then(Value::as_str),
                    Some("null"),
                    "{path}: type is null"
                );

                if let Some(Value::Array(variants)) = schema.get("enum") {
                    assert!(
                        variants.iter().all(|variant| !variant.is_null()),
                        "{path}: enum contains null"
                    );
                }

                assert!(
                    !schema.get("const").is_some_and(Value::is_null),
                    "{path}: const is null"
                );
            });
        });
    }

    #[test]
    fn objects_declare_their_type() {
        for_each_schema(|path, schema| {
            for_each_subschema(schema, path, &mut |path, schema| {
                if schema.contains_key("properties") {
                    assert_eq!(
                        schema.get("type").and_then(Value::as_str),
                        Some("object"),
                        "{path}: has properties but type is not object"
                    );
                }
            });
        });
    }

    #[test]
    fn properties_have_descriptions() {
        for_each_schema(|path, schema| {
            for_each_subschema(schema, path, &mut |path, schema| {
                assert_members_have_descriptions(schema, path, "properties");
            });
        });
    }

    #[test]
    fn defs_have_descriptions() {
        for_each_schema(|path, schema| {
            for_each_subschema(schema, path, &mut |path, schema| {
                assert_members_have_descriptions(schema, path, "$defs");
                assert_members_have_descriptions(schema, path, "definitions");
            });
        });
    }
}
