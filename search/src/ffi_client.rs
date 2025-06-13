// search/src/ffi_client.rs
use std::ffi::{CStr, CString};
use std::ptr;

use search_ffi_types::{
    FfiResultCode, SearchEngineConfigFfi, SearchEngineHandle, SearchRequestObjectFfi,
    SearchResultItemFfi,
};
use serde::Deserialize;
use tracing::{error, info};

use crate::search_trait::{SearchServiceError, SearchResponse as ServiceSearchResponse, SearchServiceRequest};

// FFI function signatures from the `lancedb_ffi` crate.
// These are marked as unsafe because they call into foreign code.
extern "C" {
    fn search_engine_new(config_ptr: *const SearchEngineConfigFfi) -> *mut SearchEngineHandle;
    fn search_engine_drop(engine_ptr: *mut SearchEngineHandle);
    fn search_engine_search_sync(
        engine_ptr: *mut SearchEngineHandle,
        request_ptr: *const SearchRequestObjectFfi,
        results_out: *mut *mut SearchResultItemFfi,
        num_results_out: *mut usize,
    ) -> FfiResultCode;
    fn free_search_results_ffi(results: *mut SearchResultItemFfi, num_results: usize);
    
    // New FFI functions for detailed error handling.
    fn lancedb_ffi_get_last_error() -> *mut i8; // Corresponds to c_char
    fn lancedb_ffi_free_string(s: *mut i8);
}

// Helper struct to deserialize metadata_json from FFI
#[derive(Deserialize, Debug)]
struct FfiMetadata {
    source: String,
    ask_method_code: String,
}


/// A safe, thread-safe client that wraps the unsafe FFI calls to the LanceDB engine.
pub struct LanceDbFfiClient {
    engine: *mut SearchEngineHandle,
}

// By implementing Send and Sync, we are asserting that the underlying FFI calls
// are thread-safe. This is a crucial contract for using this client in a concurrent context.
unsafe impl Send for LanceDbFfiClient {}
unsafe impl Sync for LanceDbFfiClient {}

impl LanceDbFfiClient {
    /// Creates a new FFI client, initializing the underlying search engine.
    /// This is a blocking operation.
    pub fn new(db_uri: &str) -> Result<Self, SearchServiceError> {
        let db_uri_owned = db_uri.to_string();
        
        let engine = {
            let c_db_uri = CString::new(db_uri_owned.as_str()).map_err(|e| {
                SearchServiceError::InternalError(format!("Failed to create CString for db_uri: {}", e))
            })?;

            let config = SearchEngineConfigFfi {
                db_uri: c_db_uri.as_ptr(),
            };

            // This is the unsafe block where we call the FFI function.
            unsafe { search_engine_new(&config) }
        };

        if engine.is_null() {
            let err_msg = format!("Failed to initialize LanceDB FFI search engine for URI: {}", db_uri);
            error!("{}", err_msg);
            Err(SearchServiceError::ExternalServiceError(err_msg))
        } else {
            info!("LanceDB FFI search engine initialized successfully for URI: {}", db_uri);
            Ok(Self { engine })
        }
    }

    /// Performs a search using the FFI engine.
    /// This is a blocking operation.
    pub fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {

        let mut embedding_vec = request.question_embedding;
        let c_table_name = CString::new(request.table_name.as_str())
            .map_err(|e| SearchServiceError::InternalError(format!("Invalid table name: {}", e)))?;
        
        // Define the column names to be passed to the FFI layer.
        let c_id_column = CString::new("expand_id").unwrap();
        let c_source_column = CString::new("source_table").unwrap();
        let c_distance_column = CString::new("_distance").unwrap();
        let c_ask_method_code_column = CString::new("ask_method_code").unwrap();

        embedding_vec.shrink_to_fit();

        let ffi_request = SearchRequestObjectFfi {
            embedding_ptr: embedding_vec.as_ptr(),
            embedding_dim: embedding_vec.len() as u32,
            top_k: request.top_k as u32,
            table_name: c_table_name.as_ptr(),
            id_column: c_id_column.as_ptr(),
            source_column: c_source_column.as_ptr(),
            distance_column: c_distance_column.as_ptr(),
            ask_method_code_column: c_ask_method_code_column.as_ptr(),
        };
        
        let mut results_ptr: *mut SearchResultItemFfi = ptr::null_mut();
        let mut num_results: usize = 0;

        // Unsafe block for the search call
        let result_code = unsafe {
            search_engine_search_sync(
                self.engine,
                &ffi_request,
                &mut results_ptr,
                &mut num_results,
            )
        };

        match result_code {
            FfiResultCode::Success => {
                // The conversion and freeing are logically coupled to the unsafe call.
                let conversion_result = Self::convert_and_free_ffi_results(results_ptr, num_results);
                conversion_result
            }
            _ => {
                let mut error_message = format!("FFI search failed with code: {:?}", result_code);
                
                // Attempt to get a more detailed error message from the FFI layer.
                let ffi_error_str_ptr = unsafe { lancedb_ffi_get_last_error() };
                if !ffi_error_str_ptr.is_null() {
                    unsafe {
                        let detailed_error = CStr::from_ptr(ffi_error_str_ptr).to_string_lossy().into_owned();
                        error_message = format!("{} - Details: {}", error_message, detailed_error);
                        // Free the string provided by the FFI layer.
                        lancedb_ffi_free_string(ffi_error_str_ptr);
                    }
                }

                error!("{}", error_message);
                // Ensure we still attempt to free memory if the FFI layer allocated it before failing.
                unsafe {
                    free_search_results_ffi(results_ptr, num_results);
                }
                Err(SearchServiceError::ExternalServiceError(error_message))
            }
        }
    }

    /// Converts the raw FFI results into safe Rust structs and frees the FFI-allocated memory.
    fn convert_and_free_ffi_results(
        results_ptr: *mut SearchResultItemFfi,
        num_results: usize,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        if results_ptr.is_null() {
            return Ok(Vec::new());
        }

        // This is the correct pattern:
        // 1. Unsafely create a slice to view the FFI data without taking ownership.
        // 2. Iterate through the slice, copying the data into safe, owned Rust types.
        // 3. Call the FFI-provided function to free the original memory.

        let conversion_result = unsafe {
            let results_slice = std::slice::from_raw_parts(results_ptr, num_results);
            let mut service_responses = Vec::with_capacity(num_results);

            for ffi_item in results_slice {
                let id = CStr::from_ptr(ffi_item.id).to_string_lossy().into_owned();
                let metadata_json = CStr::from_ptr(ffi_item.metadata_json).to_string_lossy();
                
                let metadata: FfiMetadata = serde_json::from_str(&metadata_json).map_err(|e| {
                    SearchServiceError::InternalError(format!(
                        "Failed to parse FFI item metadata_json for ID '{}': {}, json: '{}'",
                        id, e, metadata_json
                    ))
                })?;

                service_responses.push(ServiceSearchResponse {
                    id,
                    source: metadata.source,
                    distance: ffi_item.distance,
                    ask_method_code: metadata.ask_method_code,
                });
            }
            Ok(service_responses)
        };
        
        // After successfully converting all data (or on error path), free the original C structures.
        unsafe {
            free_search_results_ffi(results_ptr, num_results);
        }

        conversion_result
    }
}

impl Drop for LanceDbFfiClient {
    fn drop(&mut self) {
        info!("Shutting down LanceDB FFI search engine via FFI client drop...");
        if !self.engine.is_null() {
            // Unsafe call to the FFI drop function.
            unsafe {
                search_engine_drop(self.engine);
            }
        }
    }
} 