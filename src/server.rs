//! The MCP surface: the server type, and the tools declared against it.

use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use subxt_utils_fetchmetadata::Url;
use tokio::sync::OnceCell;

use crate::metadata::{self, LoadedMetadata, RuntimeInfo};

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
