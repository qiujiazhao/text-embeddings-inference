use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    pub id: String,
    pub source: String,
    pub similarity: f32,
    pub ask_method_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchServiceRequest {
    pub question_embedding: Vec<f32>,
    pub industry: String,
    pub solution_archetype: String,
    pub search_param: String,
    pub top_k: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum SearchProviderType {
    Mock,
    LanceDB,
    // Add other provider types here, e.g., Qdrant, etc.
}

#[derive(Error, Debug, Clone, Serialize, Deserialize, ToSchema)] 
pub enum SearchServiceError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Request validation error: {0}")]
    ValidationError(String),
    #[error("External service error: {0}")]
    ExternalServiceError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}
