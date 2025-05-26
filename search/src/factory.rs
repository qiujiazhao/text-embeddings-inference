use std::sync::Arc;

use super::lancedb_provider::LanceDbFfiSearchProvider;
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
            // Use db_url from argument if provided, otherwise default.
            let db_uri = db_url.unwrap_or_else(|| "./lance_db_data".to_string());

            tracing::info!(
                "Creating LanceDbFfiSearchProvider with URI: '{}'",
                db_uri
            );
            
            let provider = LanceDbFfiSearchProvider::new(&db_uri).await?;
            Ok(Arc::new(provider))
        }
        // Add other providers here
    }
}
