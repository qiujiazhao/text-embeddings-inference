pub mod factory;
pub mod ffi_client;
pub mod lancedb_provider;
pub mod search_provider;
pub mod search_trait;
pub mod types;

// Re-export the public APIs
pub use factory::create_search_service;
// MockSearchProvider is internal to this crate, created by the factory, so no need to re-export it directly
// pub use search_provider::MockSearchProvider;
pub use search_trait::SearchService;
pub use types::{SearchResponse, SearchServiceRequest, SearchServiceError};
