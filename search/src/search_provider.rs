use async_trait::async_trait;
// TODO: Adjust this import. SearchResponse might need to be moved or accessed differently.
// use crate::http::types::SearchResponse;
use super::search_trait::SearchService;
use super::types::{SearchServiceError, SearchServiceRequest, SearchResponse};

#[derive(Clone, Debug)] // Added Debug
pub struct MockSearchProvider {
    db_url: Option<String>, // Added db_url field
}

impl MockSearchProvider {
    pub fn new(db_url: Option<String>) -> Self { // Constructor now accepts db_url
        tracing::info!("MockSearchProvider initialized with db_url: {:?}", db_url);
        MockSearchProvider { db_url }
    }
}

#[async_trait]
impl SearchService for MockSearchProvider {
    async fn search(&self, request: SearchServiceRequest) -> Result<Vec<SearchResponse>, SearchServiceError> {
        tracing::info!("MockSearchProvider searching with db_url: {:?}", self.db_url);
        // Simulate error conditions based on the input question for demonstration
        if request.search_param.contains("trigger_provider_error") {
            return Err(SearchServiceError::ExternalServiceError(
                "Mock provider error triggered by search_param.".to_string(),
            ));
        }

        if request.search_param.contains("trigger_internal_error") {
            return Err(SearchServiceError::Unknown(
                "Mock internal error triggered by search_param.".to_string(),
            ));
        }

        let embedding_preview_len = std::cmp::min(3, request.question_embedding.len());
        let embedding_preview = request.question_embedding.iter().take(3).collect::<Vec<_>>();

        tracing::info!(
            "MockSearchProvider: Simulating search for table_name '{}', search_param '{}', top_k '{}'. Embedding (first {} dims): {:?}",
            request.table_name,
            request.search_param,
            request.top_k,
            embedding_preview_len,
            embedding_preview
        );

        let responses = vec![
            SearchResponse {
                id: "1".to_string(),
                source: format!("Mocked source for search_param: '{}' (db_url: {:?})", request.search_param, self.db_url),
                similarity: 0.98,
                ask_method_code: "mock_exact_match".to_string(),
            },
            SearchResponse {
                id: "2".to_string(),
                source: "Another mocked source".to_string(),
                similarity: 0.92,
                ask_method_code: "mock_semantic_match".to_string(),
            },
        ];
        
        let limited_responses = responses.into_iter().take(request.top_k as usize).collect();

        Ok(limited_responses)
    }
}
