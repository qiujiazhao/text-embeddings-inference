pub mod search_provider;
pub mod search_trait;
pub mod factory;

pub use factory::create_search_service;
pub use search_trait::{SearchService, SearchServiceError, SearchServiceRequest};
