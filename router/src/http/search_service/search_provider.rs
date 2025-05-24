use async_trait::async_trait;
use crate::http::types::SearchResponse; // Adjust path if SearchResponse is elsewhere
use super::search_trait::{SearchService, SearchServiceError, SearchServiceRequest};

pub struct MockSearchProvider;

impl MockSearchProvider {
    pub fn new() -> Self {
        MockSearchProvider
    }
}

#[async_trait]
impl SearchService for MockSearchProvider {
    async fn search(&self, request: SearchServiceRequest) -> Result<Vec<SearchResponse>, SearchServiceError> {
        // TODO: 第二步: 使用 `request.question_embedding`, `request.industry`, 和 `request.top_k` 
        // 来查询向量数据库或搜索引擎。
        // 这是从 server.rs 迁移过来的 TODO。
        
        tracing::info!(
            "MockSearchProvider: Simulating search for industry '{}', top_k '{}'. Original question: '{}'. Embedding (first {} dims): {:?}",
            request.industry,
            request.top_k,
            request.original_question,
            std::cmp::min(3, request.question_embedding.len()),
            request.question_embedding.iter().take(3).collect::<Vec<_>>()
        );

        let responses = vec![
            SearchResponse {
                id: 1,
                source: format!("Mocked source for query: {}", request.original_question),
                similarity: 0.98,
                ask_method_code: "mock_exact_match".to_string(),
            },
            SearchResponse {
                id: 2,
                source: "Another mocked source".to_string(),
                similarity: 0.92,
                ask_method_code: "mock_semantic_match".to_string(),
            },
        ];
        
        let limited_responses = responses.into_iter().take(request.top_k as usize).collect();

        Ok(limited_responses)
    }
}
