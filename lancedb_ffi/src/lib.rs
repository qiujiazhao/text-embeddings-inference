use search_ffi_types::{
    FfiResultCode, SearchEngineConfigFfi, SearchEngineHandle, SearchRequestObjectFfi,
    SearchRequestFfi, SearchResponseFfi, SearchResultItemFfi,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use thiserror::Error;
use tracing::{error, info, warn};
use once_cell::sync::Lazy;
use std::sync::{Mutex, Arc};
use std::collections::HashMap;
use lancedb::{Error as LanceDbErrorExt}; // Renamed to avoid conflict with FfiError::LanceDbError if any
use tokio::runtime::{Runtime, Builder as RuntimeBuilder};
use serde::Deserialize;
use futures::stream::TryStreamExt;
use arrow_array::{RecordBatch, array::{StringArray, Float32Array}};
use lancedb::query::QueryBase;
use lancedb::query::ExecutableQuery;

// Global Tokio runtime for executing async LanceDB operations
static TOKIO_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    RuntimeBuilder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime for LanceDB FFI")
});

// static mut LANCE_DB_CONNECTION: Option<lancedb::connection::Connection> = None; // Old placeholder
// static mut LANCE_DB_TABLE: Option<lancedb::table::Table> = None; // Old placeholder

#[derive(Error, Debug)]
pub enum FfiError {
    #[error("Null pointer argument: {0}")]
    NullArgument(String),
    #[error("Invalid UTF-8 string: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("CString conversion error (contains null byte): {0}")]
    NulError(#[from] std::ffi::NulError),
    #[error("JSON serialization/deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("LanceDB error: {0}")]
    LanceDbError(#[from] LanceDbErrorExt),
    #[error("Initialization failed: {0}")]
    InitializationFailed(String),
    #[error("Search failed: {0}")]
    SearchFailed(String),
    #[error("Internal FFI error: {0}")]
    InternalError(String),
}

impl From<&FfiError> for FfiResultCode {
    fn from(err: &FfiError) -> Self {
        match err {
            FfiError::NullArgument(_) => FfiResultCode::NullArgument,
            FfiError::Utf8Error(_) => FfiResultCode::Utf8Error,
            FfiError::NulError(_) => FfiResultCode::InternalError, // Or a more specific code
            FfiError::JsonError(_) => FfiResultCode::JsonError,
            FfiError::LanceDbError(_) => FfiResultCode::LanceDbError,
            FfiError::InitializationFailed(_) => FfiResultCode::InitializationFailed,
            FfiError::SearchFailed(_) => FfiResultCode::SearchFailed,
            FfiError::InternalError(_) => FfiResultCode::InternalError,
        }
    }
}

/// Helper to convert C string to Rust String.
/// Unsafe because it dereferences a raw pointer.
unsafe fn c_char_to_string(s: *const c_char, field_name: &str) -> Result<String, FfiError> {
    if s.is_null() {
        return Err(FfiError::NullArgument(field_name.to_string()));
    }
    CStr::from_ptr(s).to_str().map(|rs| rs.to_owned()).map_err(FfiError::from)
}

/// Helper to convert Rust String to C string (CString for ownership, then into_raw).
/// Caller is responsible for freeing the C string using `free_ffi_string`.
fn string_to_c_char(s: String) -> Result<*mut c_char, FfiError> {
    CString::new(s).map(|cs| cs.into_raw()).map_err(FfiError::from)
}


/// Frees a C string that was allocated by Rust and passed to C.
/// `s_ptr`: Pointer to the C string to be freed.
/* This function is no longer used by the new API design.
#[no_mangle]
pub unsafe extern "C" fn free_ffi_string(s_ptr: *mut c_char) {
    if !s_ptr.is_null() {
        drop(CString::from_raw(s_ptr));
    }
}
*/

// Basic test to ensure the FFI functions can be linked and called (at least the placeholders).
// More comprehensive tests would require a C/C++ test harness or a Rust test that simulates FFI calls.
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_init_ffi() {
        let config = CString::new("{ \"db_path\": \"/tmp/test_db\" }").unwrap();
        let result = unsafe { init_search_engine_ffi(config.as_ptr()) };
        assert_eq!(result, FfiResultCode::Success);
    }

    #[test]
    fn test_search_and_free_ffi() {
        // Dummy request for vector search
        let dummy_embedding: Vec<f32> = vec![0.1, 0.2, 0.3]; // Example embedding
        let request = SearchRequestFfi {
            embedding_ptr: dummy_embedding.as_ptr(),
            embedding_dim: dummy_embedding.len() as u32,
            top_k: 10,
        };

        // Prepare response struct (on stack for this test)
        let mut response = SearchResponseFfi {
            results: ptr::null_mut(),
            num_results: 0,
            error_message: ptr::null(),
        };

        let result_code = unsafe { perform_search_ffi(&request, &mut response) };
        assert_eq!(result_code, FfiResultCode::Success);
        assert!(!response.results.is_null());
        assert!(response.num_results > 0);
        assert!(response.error_message.is_null());

        // Test freeing the response
        unsafe { free_search_response_ffi(&mut response) };
        // After freeing, the pointers in response should ideally be nulled by free_search_response_ffi
        // or C caller should not use them. For this test, we just check it doesn't crash.
    }

     #[test]
    fn test_shutdown_ffi() {
        let result = shutdown_search_engine_ffi();
        assert_eq!(result, FfiResultCode::Success);
    }
}

// --- New Object-Based API Implementation ---

/// The actual implementation behind the opaque `SearchEngineHandle`.
pub struct SearchEngine {
    connection: Arc<lancedb::connection::Connection>,
    table_cache: HashMap<String, Arc<lancedb::Table>>,
    runtime_handle: tokio::runtime::Handle,
}

impl SearchEngine {
    /// Opens a table, using a cache if possible.
    fn get_table(&mut self, name: &str) -> Result<Arc<lancedb::Table>, FfiError> {
        if let Some(cached_table) = self.table_cache.get(name) {
            return Ok(Arc::clone(cached_table));
        }

        let conn = Arc::clone(&self.connection);
        match self
            .runtime_handle
            .block_on(conn.open_table(name).execute())
        {
            Ok(opened_table) => {
                let table_arc = Arc::new(opened_table);
                self.table_cache
                    .insert(name.to_string(), Arc::clone(&table_arc));
                info!("FFI: Opened and cached table '{}'", name);
                Ok(table_arc)
            }
            Err(e) => {
                error!("FFI Error: Failed to open table '{}': {}", name, e);
                Err(FfiError::LanceDbError(e))
            }
        }
    }

    /// Performs the actual vector search.
    fn search(
        &mut self,
        table_name: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<RecordBatch>, FfiError> {
        let table = self.get_table(table_name)?;
        
        self.runtime_handle.block_on(async {
            let stream = table
                .vector_search(query_vector)
                .map_err(|e| FfiError::InternalError(e.to_string()))?
                .limit(top_k)
                .distance_type(lancedb::DistanceType::Cosine)
                .execute()
                .await
                .map_err(FfiError::LanceDbError)?;
            stream.try_collect().await.map_err(FfiError::LanceDbError)
        })
    }
}

#[no_mangle]
pub unsafe extern "C" fn search_engine_new(
    config_ptr: *const SearchEngineConfigFfi,
) -> *mut SearchEngineHandle {
    // Initialize tracing/logging if not already done by the host application.
    let _ = tracing_subscriber::fmt::try_init();
    info!("FFI: Creating new SearchEngine instance...");

    if config_ptr.is_null() {
        error!("FFI Error in search_engine_new: config_ptr is null.");
        return ptr::null_mut();
    }
    let config = &*config_ptr;

    let db_uri = match c_char_to_string(config.db_uri, "db_uri") {
        Ok(uri) => uri,
        Err(e) => {
            error!("FFI Error in search_engine_new: Invalid db_uri: {}", e);
            return ptr::null_mut();
        }
    };

    let connection = match TOKIO_RUNTIME.block_on(lancedb::connect(&db_uri).execute()) {
        Ok(conn) => Arc::new(conn),
        Err(e) => {
            error!(
                "FFI Error in search_engine_new: Failed to connect to LanceDB at {}: {}",
                db_uri, e
            );
            return ptr::null_mut();
        }
    };

    let engine = SearchEngine {
        connection,
        table_cache: HashMap::new(),
        runtime_handle: TOKIO_RUNTIME.handle().clone(),
    };

    info!("FFI: SearchEngine instance created successfully for db: {}", db_uri);
    Box::into_raw(Box::new(engine)) as *mut SearchEngineHandle
}

#[no_mangle]
pub unsafe extern "C" fn search_engine_drop(engine_ptr: *mut SearchEngineHandle) {
    if engine_ptr.is_null() {
        return;
    }
    // This will reclaim the memory and run the Drop implementation for SearchEngine
    drop(Box::from_raw(engine_ptr as *mut SearchEngine));
    info!("FFI: SearchEngine instance dropped.");
}

#[no_mangle]
pub unsafe extern "C" fn search_engine_search_sync(
    engine_ptr: *mut SearchEngineHandle,
    request_ptr: *const SearchRequestObjectFfi,
    results_out: *mut *mut SearchResultItemFfi,
    num_results_out: *mut usize,
) -> FfiResultCode {
    if engine_ptr.is_null() || request_ptr.is_null() || results_out.is_null() || num_results_out.is_null() {
        error!("FFI Error in search_engine_search_sync: A required pointer argument is null.");
        return FfiResultCode::NullArgument;
    }

    // Initialize output parameters to safe values
    *results_out = ptr::null_mut();
    *num_results_out = 0;

    let engine = &mut *(engine_ptr as *mut SearchEngine);
    let request = &*request_ptr;

    // --- Input Conversion ---
    let table_name = match c_char_to_string(request.table_name, "table_name") {
        Ok(name) => name,
        Err(e) => {
            error!("FFI Error in search_engine_search_sync (table_name): {}", e);
            return FfiResultCode::from(&e);
        }
    };

    if request.embedding_ptr.is_null() || request.embedding_dim == 0 {
        error!("FFI Error in search_engine_search_sync: Invalid embedding_ptr or embedding_dim");
        return FfiResultCode::InvalidArgument;
    }
    let query_vector: Vec<f32> =
        slice::from_raw_parts(request.embedding_ptr, request.embedding_dim as usize).to_vec();

    // --- Core Logic Call ---
    let search_result = engine.search(
        &table_name,
        &query_vector,
        request.top_k as usize,
    );
    
    // --- Output Conversion ---
    match search_result {
        Ok(batches) => {
            let mut ffi_results = Vec::new();
            for batch in batches {
                if let Ok(items) = convert_batch_to_ffi_items(&batch) {
                    ffi_results.extend(items);
                }
            }

            if !ffi_results.is_empty() {
                let mut boxed_slice = ffi_results.into_boxed_slice();
                *results_out = boxed_slice.as_mut_ptr();
                *num_results_out = boxed_slice.len();
                std::mem::forget(boxed_slice);
            }
            FfiResultCode::Success
        }
        Err(e) => {
            error!("FFI search failed: {}", e);
            FfiResultCode::from(&e)
        }
    }
}

/// Frees the memory for search results allocated by `search_engine_search_sync`.
#[no_mangle]
pub unsafe extern "C" fn free_search_results_ffi(
    results: *mut SearchResultItemFfi,
    num_results: usize,
) {
    if results.is_null() {
        return;
    }
    let results_slice = slice::from_raw_parts_mut(results, num_results);
    for item in results_slice {
        if !item.id.is_null() {
            drop(CString::from_raw(item.id));
        }
        if !item.metadata_json.is_null() {
            drop(CString::from_raw(item.metadata_json));
        }
    }
    // Free the array itself
    let _ = Vec::from_raw_parts(results, num_results, num_results);
}


// Helper function to convert a RecordBatch to FFI items.
// This should be adapted from your existing logic.
fn convert_batch_to_ffi_items(batch: &RecordBatch) -> Result<Vec<SearchResultItemFfi>, FfiError> {
    let mut items = Vec::with_capacity(batch.num_rows());

    let id_array_arc = batch
        .column_by_name("expand_id")
        .ok_or_else(|| FfiError::InternalError("Column 'expand_id' not found".to_string()))?;
    let source_table_array_arc = batch
        .column_by_name("source_table")
        .ok_or_else(|| FfiError::InternalError("Column 'source_table' not found".to_string()))?;
    let similarities_array_arc = batch
        .column_by_name("_distance")
        .ok_or_else(|| FfiError::InternalError("Column '_distance' not found".to_string()))?;
    let ask_method_code_array_arc = batch
        .column_by_name("ask_method_code")
        .ok_or_else(|| FfiError::InternalError("Column 'ask_method_code' not found".to_string()))?;

    let id_values: Vec<String> =
        if let Some(string_array) = id_array_arc.as_any().downcast_ref::<StringArray>() {
            string_array.iter().map(|v| v.unwrap_or("").to_string()).collect()
        } else if let Some(int_array) = id_array_arc.as_any().downcast_ref::<arrow_array::Int64Array>() {
            int_array.iter().map(|v| v.unwrap_or(0).to_string()).collect()
        } else {
            return Err(FfiError::InternalError("Type mismatch for 'expand_id'".to_string()));
        };

    let source_table_array = source_table_array_arc.as_any().downcast_ref::<StringArray>()
        .ok_or_else(|| FfiError::InternalError("Type mismatch for 'source_table'".to_string()))?;
    let similarities_array = similarities_array_arc.as_any().downcast_ref::<Float32Array>()
        .ok_or_else(|| FfiError::InternalError("Type mismatch for '_distance'".to_string()))?;
    let ask_method_code_array = ask_method_code_array_arc.as_any().downcast_ref::<StringArray>()
        .ok_or_else(|| FfiError::InternalError("Type mismatch for 'ask_method_code'".to_string()))?;

    for i in 0..batch.num_rows() {
        let id_str = id_values[i].clone();
        let source_table_str = source_table_array.value(i).to_string();
        let distance_val = similarities_array.value(i);
        let ask_method_code_str = ask_method_code_array.value(i).to_string();

        let c_id = string_to_c_char(id_str.clone())
            .map_err(|e| format!("FFI: Failed to convert ID '{}' to CString: {}", id_str, e))?;
        
        let metadata_str = format!(
            "{{\"source\": \"{}\", \"ask_method_code\": \"{}\"}}",
            source_table_str, ask_method_code_str
        );
        let c_metadata = string_to_c_char(metadata_str.clone())
            .map_err(|e| format!("FFI: Failed to convert metadata '{}' to CString: {}", metadata_str, e))?;

        items.push(SearchResultItemFfi {
            id: c_id,
            distance: distance_val,
            metadata_json: c_metadata,
        });
    }

    Ok(items)
}
