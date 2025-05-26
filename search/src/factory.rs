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
            let table_name = "P-clothing"; // This could also come from configuration

            tracing::info!(
                "Creating LanceDbFfiSearchProvider with URI: '{}', Table: '{}'",
                db_uri,
                table_name
            );
            
            let provider = LanceDbFfiSearchProvider::new(&db_uri, table_name).await?;
            Ok(Arc::new(provider))
        }
        // Add other providers here
    }
}
