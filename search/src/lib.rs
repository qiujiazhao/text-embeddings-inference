pub mod factory;
pub mod search_provider;
pub mod search_trait;
pub mod lancedb_provider;
pub mod types;

use search_ffi_types::{FfiResultCode, SearchRequestFfi, SearchResponseFfi};
use std::os::raw::c_char;

extern "C" {
    // FFI functions from lancedb_ffi static library
    // These are unsafe to call because they are FFI calls.
    fn init_search_engine_ffi(config_json_ptr: *const c_char) -> FfiResultCode;
    fn perform_search_ffi(
        request_ptr: *const SearchRequestFfi,
        response_ptr_ptr: *mut *mut SearchResponseFfi, // Corrected: pointer to pointer
    ) -> FfiResultCode;
    fn free_search_response_ffi(response_ptr: *mut SearchResponseFfi);
    #[allow(dead_code)]
    fn free_ffi_string(s_ptr: *mut c_char);
    fn shutdown_search_engine_ffi() -> FfiResultCode;
}

// Re-export the public APIs
pub use factory::create_search_service;
// MockSearchProvider is internal to this crate, created by the factory, so no need to re-export it directly
// pub use search_provider::MockSearchProvider;
pub use search_trait::SearchService;
pub use types::{SearchResponse, SearchServiceRequest, SearchServiceError};
