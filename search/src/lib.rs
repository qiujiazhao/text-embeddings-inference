pub mod factory;
pub mod lancedb_provider;
pub mod search_provider;
pub mod search_trait;
pub mod types;

use search_ffi_types::{
    FfiResultCode, SearchEngineConfigFfi, SearchEngineHandle, SearchRequestObjectFfi,
    SearchResultItemFfi,
};
use std::os::raw::c_char;

extern "C" {
    // --- New Object-Based FFI functions ---
    fn search_engine_new(config: *const SearchEngineConfigFfi) -> *mut SearchEngineHandle;
    fn search_engine_drop(engine: *mut SearchEngineHandle);
    fn search_engine_search_sync(
        engine: *mut SearchEngineHandle,
        request: *const SearchRequestObjectFfi,
        results_out: *mut *mut SearchResultItemFfi,
        num_results_out: *mut usize,
    ) -> FfiResultCode;
    fn free_search_results_ffi(results: *mut SearchResultItemFfi, num_results: usize);
    // fn free_ffi_string(s_ptr: *mut c_char); // This is no longer needed
}

// Re-export the public APIs
pub use factory::create_search_service;
// MockSearchProvider is internal to this crate, created by the factory, so no need to re-export it directly
// pub use search_provider::MockSearchProvider;
pub use search_trait::SearchService;
pub use types::{SearchResponse, SearchServiceRequest, SearchServiceError};
