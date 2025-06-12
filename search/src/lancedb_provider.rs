// search/src/lancedb_provider.rs (New Content)
use crate::search_trait::{
    SearchService, SearchServiceError, SearchServiceRequest, SearchResponse as ServiceSearchResponse,
};
use crate::{
    free_search_results_ffi, search_engine_drop, search_engine_new, search_engine_search_sync,
};
use async_trait::async_trait;
use search_ffi_types::{
    FfiResultCode, SearchEngineConfigFfi, SearchEngineHandle, SearchRequestObjectFfi,
    SearchResultItemFfi,
};
use serde::Deserialize;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task;
use tracing::{debug, error, info, warn};

// Helper struct to deserialize metadata_json from FFI
#[derive(Deserialize, Debug)]
struct FfiMetadata {
    source: String,
    ask_method_code: String,
}

pub struct LanceDbFfiSearchProvider {
    engine: *mut SearchEngineHandle,
    // We keep this to ensure the Drop logic runs only once,
    // especially important with raw pointers.
    is_dropped: AtomicBool,
}

// Ensure the provider is Send + Sync
unsafe impl Send for LanceDbFfiSearchProvider {}
unsafe impl Sync for LanceDbFfiSearchProvider {}

impl LanceDbFfiSearchProvider {
    pub async fn new(db_uri: &str) -> Result<Self, SearchServiceError> {
        let db_uri_owned = db_uri.to_string();

        let engine = task::spawn_blocking(move || {
            let c_db_uri = match CString::new(db_uri_owned.as_str()) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create CString for db_uri: {}", e);
                    return ptr::null_mut();
                }
            };

            let config = SearchEngineConfigFfi {
                db_uri: c_db_uri.as_ptr(),
            };

            unsafe { search_engine_new(&config) }
        })
        .await
        .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI init panicked: {}", e)))?;

        if engine.is_null() {
            let err_msg = format!("Failed to initialize LanceDB FFI search engine for URI: {}", db_uri);
            error!("{}", err_msg);
            Err(SearchServiceError::ExternalServiceError(err_msg))
        } else {
            info!("LanceDB FFI search engine initialized successfully for URI: {}", db_uri);
            Ok(Self {
                engine,
                is_dropped: AtomicBool::new(false),
            })
        }
    }

    fn convert_ffi_results(
        results: *mut SearchResultItemFfi,
        num_results: usize,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        if results.is_null() {
            return Ok(Vec::new());
        }

        let results_slice = unsafe { std::slice::from_raw_parts(results, num_results) };
        let mut service_responses = Vec::with_capacity(num_results);

        for ffi_item in results_slice {
            let id = unsafe { CStr::from_ptr(ffi_item.id).to_string_lossy().into_owned() };
            let metadata_json = unsafe { CStr::from_ptr(ffi_item.metadata_json).to_string_lossy() };
            
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
    }
}

#[async_trait]
impl SearchService for LanceDbFfiSearchProvider {
    async fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<ServiceSearchResponse>, SearchServiceError> {
        if self.is_dropped.load(Ordering::SeqCst) {
            return Err(SearchServiceError::ExternalServiceError(
                "LanceDB FFI provider has been shut down.".to_string(),
            ));
        }

        let mut embedding_vec = request.question_embedding;
        let c_table_name = CString::new(request.table_name.as_str())
            .map_err(|e| SearchServiceError::InternalError(format!("Invalid table name: {}", e)))?;

        // Since the FFI call is blocking, we must not block the async runtime.
        let engine_ptr = self.engine;
        task::spawn_blocking(move || {
            embedding_vec.shrink_to_fit();

            let ffi_request = SearchRequestObjectFfi {
                embedding_ptr: embedding_vec.as_ptr(),
                embedding_dim: embedding_vec.len() as u32,
                top_k: request.top_k as u32,
                table_name: c_table_name.as_ptr(),
            };
            
            let mut results_ptr: *mut SearchResultItemFfi = ptr::null_mut();
            let mut num_results: usize = 0;

            let result_code = unsafe {
                search_engine_search_sync(
                    engine_ptr,
                    &ffi_request,
                    &mut results_ptr,
                    &mut num_results,
                )
            };

            match result_code {
                FfiResultCode::Success => {
                    let conversion_result = Self::convert_ffi_results(results_ptr, num_results);
                    // Crucially, free the memory allocated by the FFI layer.
                    unsafe {
                        free_search_results_ffi(results_ptr, num_results);
                    }
                    conversion_result
                }
                _ => {
                    let error_message = format!("FFI search failed with code: {:?}", result_code);
                    error!("{}", error_message);
                    Err(SearchServiceError::ExternalServiceError(error_message))
                }
            }
        })
        .await
        .map_err(|e| SearchServiceError::InternalError(format!("Task for FFI search panicked: {}", e)))?
    }
}

impl Drop for LanceDbFfiSearchProvider {
    fn drop(&mut self) {
        // Use compare_exchange to ensure drop is only called once.
        if self.is_dropped.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            info!("Shutting down LanceDB FFI search engine...");
            unsafe {
                search_engine_drop(self.engine);
            }
            info!("LanceDB FFI search engine shut down successfully.");
        }
    }
}
