// search/src/lancedb_provider.rs (New Content)
use crate::ffi_client::LanceDbFfiClient;
use crate::search_trait::{
    SearchService, SearchServiceError, SearchServiceRequest, SearchResponse as ServiceSearchResponse,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::task;

pub struct LanceDbFfiSearchProvider {
    client: Arc<LanceDbFfiClient>,
}

impl LanceDbFfiSearchProvider {
    pub async fn new(db_uri: &str) -> Result<Self, SearchServiceError> {
        let db_uri_owned = db_uri.to_string();

        // The FFI client's `new` method is blocking, so we spawn it on a blocking thread.
        let client = task::spawn_blocking(move || LanceDbFfiClient::new(&db_uri_owned))
            .await
            .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI init panicked: {}", e)))??;

        Ok(Self {
            client: Arc::new(client),
        })
    }
}

#[async_trait]
impl SearchService for LanceDbFfiSearchProvider {
    async fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        let client = Arc::clone(&self.client);
        task::spawn_blocking(move || client.search(request))
            .await
            .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI search panicked: {}", e)))?
    }
}
// No more Drop trait needed, RAII on the `client` field handles it.
// No more `unsafe impl Send + Sync` needed, the struct is safe by construction.
