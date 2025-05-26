// TODO: Adjust this import. SearchResponse might need to be moved or accessed differently.
use async_trait::async_trait;
pub use crate::types::{SearchResponse, SearchServiceRequest, SearchServiceError};

#[async_trait]
pub trait SearchService: Send + Sync {
    async fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<SearchResponse>, SearchServiceError>;
}
