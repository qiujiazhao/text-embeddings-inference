use crate::http::types::SearchResponse; // Adjust path if SearchResponse is elsewhere
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct SearchServiceRequest {
    pub question_embedding: Vec<f32>,
    pub industry: String,
    pub top_k: i32,
    pub original_question: String,
}

#[derive(Error, Debug)]
pub enum SearchServiceError {
    #[error("Search provider failed: {0}")]
    ProviderError(String),
    #[error("Search internal error: {0}")]
    InternalError(String),
}

#[async_trait]
pub trait SearchService: Send + Sync {
    async fn search(&self, request: SearchServiceRequest) -> Result<Vec<SearchResponse>, SearchServiceError>;
}
