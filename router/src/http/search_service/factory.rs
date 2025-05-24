use std::sync::Arc;

use super::search_provider::MockSearchProvider;
use super::search_trait::SearchService;

pub fn create_search_service(db_url: Option<String>) -> Arc<dyn SearchService> {
    // In the future, we can add logic here to decide which SearchService implementation
    // to create based on db_url or other configurations.
    // For example, if db_url is Some, create a real provider, otherwise mock or error.
    tracing::info!("create_search_service (function) creating MockSearchProvider with db_url: {:?}", db_url);
    // Ensure MockSearchProvider::new() receives the db_url
    Arc::new(MockSearchProvider::new())
}
