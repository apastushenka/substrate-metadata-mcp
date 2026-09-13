//! The MCP surface: the server type, and the tools declared against it.

use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use subxt_utils_fetchmetadata::Url;
use tokio::sync::OnceCell;

use crate::metadata::{
    self, LoadedMetadata, NamespaceList, PalletItems, PalletItemsRequest, RuntimeApiMethods,
    RuntimeApiMethodsRequest, RuntimeInfo, SearchRequest, SearchResults,
};

#[derive(Clone)]
pub struct MetadataServer {
    url: Url,
    metadata: Arc<OnceCell<LoadedMetadata>>,
}

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

    /// Identify the runtime this server indexes.
    #[tool]
    async fn runtime_info(&self) -> Result<Json<RuntimeInfo>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::runtime_info(loaded).map(Json).map_err(tool_error)
    }

    /// List the runtime's pallets and runtime API traits -- the map to read first. Each
    /// entry's `kind` says which of the two it is, and so which tool lists what it contains:
    /// `pallet_items` takes a pallet's `name` as `pallet`, `runtime_api_methods` takes
    /// a trait's `name` as `runtime_api`.
    #[tool]
    async fn namespaces(&self) -> Result<Json<NamespaceList>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        Ok(Json(metadata::namespaces(loaded)))
    }

    /// List every item of one pallet: its calls, view functions, storage entries,
    /// constants, events and errors, grouped by kind, each kind in the order the pallet
    /// declares it, unlike `search`, which ranks; a long listing is paged, not cut.
    /// If `offset` plus the number of items returned is less than `total`, there are
    /// more: repeat with a larger `offset`. A mixed page can end mid-kind, and nothing
    /// marks that kind complete -- pass `kinds` to take one whole.
    #[tool]
    async fn pallet_items(
        &self,
        Parameters(request): Parameters<PalletItemsRequest>,
    ) -> Result<Json<PalletItems>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::pallet_items(loaded, &request)
            .map(Json)
            .map_err(tool_error)
    }

    /// List every method of one runtime API, never truncated, in the order the trait
    /// declares them -- unlike `search`, which ranks and truncates.
    #[tool]
    async fn runtime_api_methods(
        &self,
        Parameters(request): Parameters<RuntimeApiMethodsRequest>,
    ) -> Result<Json<RuntimeApiMethods>, String> {
        let loaded = self.metadata().await.map_err(tool_error)?;

        metadata::runtime_api_methods(loaded, &request)
            .map(Json)
            .map_err(tool_error)
    }

    /// Find pallets, runtime APIs, calls, runtime API methods, view functions, storage
    /// entries, constants, events and errors whose name or documentation matches a query.
    /// The runtime's metadata is far too large to read whole; this is how you locate
    /// something in it. Results are ranked and truncated -- check `total` before assuming
    /// you have all of them.
    #[tool]
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
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder().enable_tools().build();

        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(format!(
                "Read-only index of the Substrate runtime metadata at {}.",
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

            assert!(
                description.is_some_and(|text| !text.is_empty()),
                "{path}/{key}/{name}: missing description"
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
