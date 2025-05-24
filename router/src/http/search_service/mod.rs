pub mod search_provider;
pub mod search_trait;

pub use search_trait::{SearchService, SearchServiceError, SearchServiceRequest};
pub use search_provider::MockSearchProvider;
