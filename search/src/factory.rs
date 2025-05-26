use std::sync::Arc;

use super::lancedb_provider::LanceDbSearchProvider;
use super::search_provider::MockSearchProvider;
use super::search_trait::{SearchService, SearchServiceError};
use super::types::SearchProviderType;

pub async fn create_search_service(
    provider_type: SearchProviderType,
    db_url: Option<String>, // Kept for MockSearchProvider, can be adapted for future config
) -> Result<Arc<dyn SearchService>, SearchServiceError> {
    match provider_type {
        SearchProviderType::Mock => {
            tracing::info!("Creating MockSearchProvider with db_url: {:?}", db_url);
            Ok(Arc::new(MockSearchProvider::new(db_url)))
        }
        SearchProviderType::LanceDB => {
            // For LanceDB, db_url from argument might be used or a specific config for URI
            // For now, hardcoding URI, table name, and embedding dimension
            let db_uri = "./lance_db_data"; // Example local directory
            let table_name = "vector_table";
            let embedding_dim = 1536; // Example dimension (e.g., OpenAI ada-002)

            tracing::info!(
                "Creating LanceDbSearchProvider with URI: '{}', Table: '{}', Dim: {}",
                db_uri,
                table_name,
                embedding_dim
            );
            
            let provider = LanceDbSearchProvider::new(db_uri, table_name, embedding_dim).await?;
            Ok(Arc::new(provider))
        }
        // Add other providers here
    }
}
