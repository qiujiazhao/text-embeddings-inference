// search/src/lancedb_provider.rs (New Content)
use crate::search_trait::{SearchService, SearchServiceError, SearchServiceRequest, SearchResponse as ServiceSearchResponse};
use crate::{
    init_search_engine_ffi, perform_search_ffi, free_search_response_ffi, shutdown_search_engine_ffi,
};
#[allow(unused_imports)]
use search_ffi_types::{SearchResultItemFfi}; // Keep only SearchResultItemFfi or other specific types if directly used without module prefix
use async_trait::async_trait;
use serde::Deserialize;
use std::ffi::{CStr, CString};
// use std::os::raw::c_char; // Not directly needed here as it's part of FFI types
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task;
use tracing::{debug, error, info, warn};

// Helper struct to deserialize metadata_json from FFI
#[derive(Deserialize, Debug)]
struct FfiMetadata {
    source: String,
    ask_method_code: String,
    // Add other fields if they exist in metadata_json
}

pub struct LanceDbFfiSearchProvider {
    initialized: AtomicBool,
}

impl LanceDbFfiSearchProvider {
    pub async fn new(db_uri: &str) -> Result<Self, SearchServiceError> {
        let db_uri_owned = db_uri.to_string();
        // For logging outside spawn_blocking if needed, capture original values or construct string here
        let log_config_str = format!("db_path: {}", db_uri);

        let init_result = task::spawn_blocking(move || {
            let init_config_json = serde_json::json!({
                "db_uri": db_uri_owned,
            }).to_string();
            let c_config_json = match CString::new(init_config_json) {
                Ok(c) => c,
                Err(e) => return Err(SearchServiceError::InternalError(format!("Failed to create CString for config: {}", e))),
            };
            let result_code = unsafe { init_search_engine_ffi(c_config_json.as_ptr()) };
            Ok(result_code)
        })
        .await
        .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI init panicked: {}", e)))??; // Note: double unwrap for Result<Result<_,_>, JoinError>

        match init_result {
            search_ffi_types::FfiResultCode::Success => {
                info!("LanceDB FFI search engine initialized successfully with config: {}", log_config_str);
                Ok(Self {
                    initialized: AtomicBool::new(true),
                })
            }
            _ => {
                let err_msg = format!(
                    "Failed to initialize LanceDB FFI search engine (code: {:?}). Config: {}",
                    init_result, log_config_str
                );
                error!("{}", err_msg);
                Err(SearchServiceError::ExternalServiceError(err_msg))
            }
        }
    }

    // This function is already static-like in its signature, no change needed to its definition
    fn convert_ffi_response(
        ffi_response: &search_ffi_types::SearchResponseFfi,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        let mut service_responses = Vec::new();
        if ffi_response.results.is_null() {
            if ffi_response.num_results > 0 {
                return Err(SearchServiceError::InternalError(
                    "FFI response has non-zero num_results but null results_ptr.".to_string(),
                ));
            }
            // No results, but not an error state by itself.
            return Ok(service_responses);
        }

        let results_slice = unsafe {
            std::slice::from_raw_parts(ffi_response.results, ffi_response.num_results)
        };

        for ffi_item in results_slice {
            let id_str = unsafe {
                if ffi_item.id.is_null() {
                    return Err(SearchServiceError::InternalError("FFI item ID is null".to_string()));
                }
                CStr::from_ptr(ffi_item.id).to_str().map_err(|e| {
                    SearchServiceError::InternalError(format!("Invalid UTF-8 for FFI item ID: {}", e))
                })?
            };
            let id = id_str.to_string();

            let metadata_json_str = unsafe {
                if ffi_item.metadata_json.is_null() {
                     // Assuming metadata might be optional or not always present
                    warn!("FFI item metadata_json is null for ID: {}", id_str);
                    // Provide an empty JSON object string to avoid erroring out if metadata is optional
                    // and FfiMetadata expects fields that might not be there.
                    // Alternatively, FfiMetadata fields could be Option<String>.
                    "{\"source\": \"\", \"ask_method_code\": \"\"}" 
                } else {
                    CStr::from_ptr(ffi_item.metadata_json).to_str().map_err(|e| {
                        SearchServiceError::InternalError(format!("Invalid UTF-8 for FFI item metadata_json: {}", e))
                    })?
                }
            };
            
            let metadata: FfiMetadata = serde_json::from_str(metadata_json_str).map_err(|e| {
                SearchServiceError::InternalError(format!(
                    "Failed to parse FFI item metadata_json for ID '{}': {}, json: '{}'",
                    id_str, e, metadata_json_str
                ))
            })?;

            service_responses.push(ServiceSearchResponse {
                id,
                source: metadata.source,
                similarity: ffi_item.score,
                ask_method_code: metadata.ask_method_code,
            });
        }
        Ok(service_responses)
    }
}

#[async_trait]
impl SearchService for LanceDbFfiSearchProvider {
    async fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(SearchServiceError::ExternalServiceError(
                "LanceDB FFI provider not initialized or initialization failed.".to_string(),
            ));
        }

        let mut embedding_vec = request.question_embedding; // Take ownership
        let top_k = request.top_k;
    // Move table_name from request into the closure by capturing it here.
    // No longer using self.table_name for the FFI request's table_name.
    let request_table_name = request.table_name; 

    task::spawn_blocking(move || {
        embedding_vec.shrink_to_fit(); // Good practice

        // Convert the captured request_table_name to CString for FFI
        let c_table_name = match CString::new(request_table_name.as_str()) {
            Ok(s) => s,
            Err(e) => return Err(SearchServiceError::InternalError(format!("Failed to create CString for table_name '{}': {}", request_table_name, e))),
        };

        let ffi_request = search_ffi_types::SearchRequestFfi {
                embedding_ptr: embedding_vec.as_ptr(),
                embedding_dim: embedding_vec.len() as u32,
            top_k: top_k as u32,
            table_name: c_table_name.as_ptr(), // Use the CString from request_table_name
            // filters_json: std::ptr::null(),    // Example for filters, if needed later
        };

        let ffi_request_ptr = &ffi_request as *const search_ffi_types::SearchRequestFfi;

            // Added detailed logging for c_table_name
            info!(
                "LanceDbFfiSearchProvider: About to call FFI. c_table_name pointer: {:?}, value (lossy): '{}'",
                c_table_name.as_ptr(),
                c_table_name.to_string_lossy()
            );

            // This will hold the pointer to the FFI-allocated SearchResponseFfi
        let mut ffi_response_raw_ptr: *mut search_ffi_types::SearchResponseFfi = ptr::null_mut(); 

        // Call FFI, passing the address of our raw pointer
        let result_code = unsafe { perform_search_ffi(ffi_request_ptr, &mut ffi_response_raw_ptr) };
        
            // `embedding_vec` is owned by this closure and its lifetime is managed correctly.
            // `c_table_name` (CString) is also owned and its pointer is valid for the FFI call.
        // `c_table_name` (CString) is also owned and its pointer is valid for the FFI call.
            // `c_table_name` (CString) is also owned and its pointer is valid for the FFI call.

            match result_code {
                search_ffi_types::FfiResultCode::Success => {
                    if ffi_response_raw_ptr.is_null() {
                        // This case should ideally not happen if FFI returns Success
                        error!("FFI search returned Success but response pointer is null.");
                        return Err(SearchServiceError::ExternalServiceError(
                            "FFI search succeeded but returned a null response.".to_string(),
                        ));
                    }
                    // Safely dereference the raw pointer to get a reference
                    let ffi_response = unsafe { &*ffi_response_raw_ptr };
                    debug!("FFI search successful. num_results: {}", ffi_response.num_results);
                    
                    let conversion_result = LanceDbFfiSearchProvider::convert_ffi_response(ffi_response);
                    
                    // IMPORTANT: Free the FFI-allocated SearchResponseFfi structure
                    unsafe {
                        free_search_response_ffi(ffi_response_raw_ptr);
                    }
                    conversion_result // This is Result<Vec<ServiceSearchResponse>, SearchServiceError>
                }
                _ => { // Handles all other FfiResultCode variants as errors
                    let mut error_message_str = format!("FFI search failed with code: {:?}", result_code);
                    // Try to get a more specific error message from the FFI response if the pointer is valid
                    // (even on error, FFI might populate error_message)
                    if !ffi_response_raw_ptr.is_null() {
                        let ffi_response_on_error = unsafe { &*ffi_response_raw_ptr };
                        if !ffi_response_on_error.error_message.is_null() {
                            unsafe {
                                match CStr::from_ptr(ffi_response_on_error.error_message).to_str() {
                                    Ok(ffi_err_msg) => {
                                        error_message_str.push_str(&format!(" FFI Error: {}", ffi_err_msg));
                                    }
                                    Err(e) => {
                                        error_message_str.push_str(&format!(" FFI Error message unreadable: {}", e));
                                    }
                                }
                                // Assuming free_search_response_ffi handles freeing error_message if present.
                            }
                        }
                        // IMPORTANT: Free the FFI-allocated SearchResponseFfi structure even on error, if it was allocated.
                        unsafe {
                            free_search_response_ffi(ffi_response_raw_ptr);
                        }
                    }
                    error!("{}", error_message_str);
                    Err(SearchServiceError::ExternalServiceError(error_message_str))
                }
            }
        })
        .await
        .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI search panicked: {}", e)))?
        // The final '?' unwraps the Result from the closure itself.
    }
}

impl Drop for LanceDbFfiSearchProvider {
    fn drop(&mut self) {
        if self.initialized.load(Ordering::SeqCst) {
            info!("Shutting down LanceDB FFI search engine...");
            // This is a blocking call in drop, which is generally okay.
            // If shutdown_search_engine_ffi could panic, it's more problematic.
            let result_code = unsafe { shutdown_search_engine_ffi() };
            match result_code {
                search_ffi_types::FfiResultCode::Success => info!("LanceDB FFI search engine shut down successfully."),
                _ => error!(
                    "Failed to shut down LanceDB FFI search engine (code: {:?})",
                    result_code
                ),
            }
            self.initialized.store(false, Ordering::SeqCst);
        }
    }
}
