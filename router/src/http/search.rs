use crate::http::types::TruncationDirection;
use crate::{ErrorResponse, Info};
use axum::extract::Extension;
use axum::http::StatusCode;
use axum::Json;
use search::{SearchService, SearchServiceRequest, SearchServiceError, SearchResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use text_embeddings_core::infer::Infer;
use tracing::instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use utoipa::ToSchema;

// Search API Types
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct SearchQuery {
    #[schema(example = "What is the capital of France?")]
    pub question: String,
    #[schema(example = "general")]
    pub table_name: String,
    #[schema(example = 10)]
    pub top_k: i32,
}

/// Search for relevant documents based on a query.
#[utoipa::path(
post,
tag = "Text Embeddings Inference",
path = "/search",
request_body = SearchQuery,
responses(
(status = 200, description = "Search results returned successfully", body = Vec<SearchResponse>),
(status = 422, description = "Validation Error: Request body is invalid", body = ErrorResponse, example = json!({"error": "Failed to deserialize request body: invalid type: integer `123`, expected a string for field `question`", "error_type": "validation"})),
(status = 500, description = "Internal Server Error", body = ErrorResponse, example = json!({"error": "An unexpected error occurred during search", "error_type": "backend"}))
)
)]
#[instrument(skip_all, fields(total_time, queue_time, search_processing_time, embedding_time))]
pub(crate) async fn search(
    infer: Extension<Infer>,
    info: Extension<Info>,
    Extension(context): Extension<Option<opentelemetry::Context>>,
    Extension(search_service): Extension<Arc<dyn SearchService>>,
    Json(req): Json<SearchQuery>,
) -> Result<Json<Vec<SearchResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let span = tracing::Span::current();
    span.set_parent(context.unwrap_or_else(opentelemetry::Context::current));
    let search_start_time = Instant::now();

    tracing::info!("Received search request: {:?}", req);

    // --- Embedding Logic ---
    let embedding_start_time = Instant::now();
    
    let permit = infer.try_acquire_permit().map_err(|e: text_embeddings_core::TextEmbeddingsError| {
        let error_response = crate::ErrorResponse::from(e);
        let status_code = StatusCode::from(&error_response.error_type);
        (status_code, Json(error_response))
    })?;

    let truncate_param = info.auto_truncate;
    let truncation_direction_param = TruncationDirection::default();
    let normalize_param = true;
    let prompt_name_param = None;

    let embedding_response = infer
        .embed_pooled(
            req.question.clone(),
            truncate_param,
            truncation_direction_param.into(),
            prompt_name_param,
            normalize_param,
            permit,
        )
        .await
        .map_err(|e: text_embeddings_core::TextEmbeddingsError| {
            let error_response = crate::ErrorResponse::from(e);
            let status_code = StatusCode::from(&error_response.error_type);
            (status_code, Json(error_response))
        })?;

    let question_embedding: Vec<f32> = embedding_response.results;
    let embedding_time = embedding_start_time.elapsed().as_millis();
    span.record("embedding_time", &embedding_time);

    tracing::info!(
        "Question embedded successfully in {}ms. Embedding vector dimension: {}",
        embedding_time,
        question_embedding.len()
    );

    // --- Search Logic using SearchService ---
    let actual_search_processing_start_time = Instant::now();
    let search_service_request = SearchServiceRequest {
        question_embedding,
        table_name: req.table_name.clone(), // Renamed from industry, using req.industry as source for table_name
        // solution_archetype has been removed
        search_param: req.question.clone(), // Using req.question for search_param
        top_k: req.top_k,
    };

    let responses = search_service.search(search_service_request).await
        .map_err(|search_service_error: SearchServiceError| {
            tracing::error!("Search service error: {:?}", search_service_error);
            let (error_type_enum, error_message_str) = match search_service_error {
                SearchServiceError::DatabaseError(msg) => (crate::ErrorType::Backend, msg),
                SearchServiceError::ValidationError(msg) => (crate::ErrorType::Validation, msg), // Or Backend, depending on desired HTTP status
                SearchServiceError::ExternalServiceError(msg) => (crate::ErrorType::Backend, msg),
                SearchServiceError::NotFound(msg) => (crate::ErrorType::NotFound, msg),
                SearchServiceError::InternalError(msg) => (crate::ErrorType::Backend, msg), // Added this line
                SearchServiceError::Unknown(msg) => (crate::ErrorType::Backend, msg),
            };
            let status_code = StatusCode::from(&error_type_enum);
            let error_response_struct = crate::ErrorResponse {
                error: error_message_str,
                error_type: error_type_enum,
            };
            (status_code, Json(error_response_struct))
        })?;

    let actual_search_processing_time = actual_search_processing_start_time.elapsed().as_millis();
    let total_time = search_start_time.elapsed().as_millis();

    span.record("search_processing_time", &actual_search_processing_time);
    span.record("queue_time", &0u128);
    span.record("total_time", &total_time);

    tracing::info!(
        "Search completed in {}ms (Embedding: {}ms, Processing: {}ms, Queue: 0ms)",
        total_time,
        embedding_time,
        actual_search_processing_time
    );

    Ok(Json(responses))
} 