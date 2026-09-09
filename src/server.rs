//! The MCP surface: the server type, and the tools declared against it.

use std::sync::Arc;

use rmcp::{
    ServerHandler,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler, tool_router,
};
use subxt_metadata::Metadata;
use subxt_utils_fetchmetadata::Url;
use tokio::sync::OnceCell;

use crate::metadata;

#[derive(Clone)]
pub struct MetadataServer {
    url: Url,
    metadata: Arc<OnceCell<Metadata>>,
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
    pub async fn metadata(&self) -> anyhow::Result<&Metadata> {
        self.metadata
            .get_or_try_init(|| async {
                let metadata = metadata::load(&self.url).await?;

                eprintln!(
                    "{}: loaded metadata from {}",
                    env!("CARGO_PKG_NAME"),
                    self.url
                );

                anyhow::Ok(metadata)
            })
            .await
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
