use search_ffi_types::{FfiResultCode, SearchRequestFfi, SearchResponseFfi, SearchResultItemFfi};
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
// use arrow_schema::{DataType as ArrowDataType, Field, Schema}; // Commenting out as they seem unused

#[derive(Deserialize, Debug)]
struct InitConfig {
    db_uri: String,
}

struct LanceDbGlobalState {
    connection: Option<Arc<lancedb::connection::Connection>>,
    table_cache: HashMap<String, Arc<lancedb::Table>>,
}

static LANCE_DB_STATE: Lazy<Mutex<LanceDbGlobalState>> = Lazy::new(|| {
    Mutex::new(LanceDbGlobalState {
        connection: None,
        table_cache: HashMap::new(),
    })
});

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

/// Placeholder for initialization logic.
/// This function should establish a connection to LanceDB and open a table.
/// `config_json_ptr`: A JSON string with configuration details (e.g., DB path, table name).
#[no_mangle]
pub unsafe extern "C" fn init_search_engine_ffi(config_json_ptr: *const c_char) -> FfiResultCode {
    // Initialize tracing/logging if not already done by the host application.
    let _ = tracing_subscriber::fmt::try_init(); // Attempt to init, ignore error if already init
    info!("!!!!!!!!!!!!!! LANCEDB_FFI: init_search_engine_ffi IS EXECUTING !!!!!!!!!!!!!!");
    info!("FFI: init_search_engine_ffi called (original log)");

    let config_json = match c_char_to_string(config_json_ptr, "config_json") {
        Ok(json) => json,
        Err(e) => {
            error!("FFI Error in init: {}", e);
            return FfiResultCode::from(&e);
        }
    };

    info!("FFI Init config JSON: {}", config_json);

    let config: InitConfig = match serde_json::from_str(&config_json) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("FFI Error in init: Failed to parse config_json: {}", e);
            return FfiResultCode::JsonError;
        }
    };

    info!("FFI Parsed init config: {:?}", config);

    let mut state = match LANCE_DB_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            error!("FFI Error in init: Mutex poisoned: {}", poisoned);
            return FfiResultCode::InternalError;
        }
    };

    if state.connection.is_some() {
        warn!("FFI init: LanceDB connection already initialized. Skipping re-initialization.");
        return FfiResultCode::Success; // Or an error indicating it's already initialized
    }

    let new_connection_result: Result<Arc<lancedb::connection::Connection>, LanceDbErrorExt> = TOKIO_RUNTIME.block_on(async {
        lancedb::connect(&config.db_uri).execute().await.map(Arc::new)
    });
    let new_connection = match new_connection_result {
        Ok(conn) => conn,
        Err(e) => {
            error!("FFI Error in init: Failed to connect to LanceDB at {}: {}", config.db_uri, e);
            return FfiResultCode::from(&FfiError::LanceDbError(e));
        }
    };
    state.connection = Some(new_connection);
    info!("FFI: LanceDB connection established successfully to {}", config.db_uri);
    FfiResultCode::Success
}

#[no_mangle]
pub unsafe extern "C" fn perform_search_ffi(
    request_ptr: *const SearchRequestFfi,
    response_ptr: *mut *mut SearchResponseFfi, // Double pointer for response
) -> FfiResultCode {
    info!("!!!!!!!!!!!!!! LANCEDB_FFI: perform_search_ffi IS EXECUTING !!!!!!!!!!!!!!");
    info!("FFI: perform_search_ffi called");

    // Validate response_ptr itself first, as we need it to return any error details.
    if response_ptr.is_null() {
        error!("FFI Error in perform_search: output parameter response_ptr is null. Cannot return detailed error or result.");
        return FfiResultCode::NullArgument;
    }

    // Initialize the caller's pointer to null. This FFI function is responsible for allocating.
    // This must be done early. All subsequent returns must assign a valid (or null for specific errors) Boxed SearchResponseFfi to *response_ptr.
    *response_ptr = ptr::null_mut();

    // Validate request_ptr input parameter.
    if request_ptr.is_null() {
        error!("FFI Error in perform_search: input parameter request_ptr is null.");
        let mut err_resp = SearchResponseFfi::default();
        err_resp.error_message = string_to_c_char("request_ptr input is null".to_string()).unwrap_or(ptr::null_mut());
        *response_ptr = Box::into_raw(Box::new(err_resp));
        return FfiResultCode::NullArgument;
    }

    // Dereference request_ptr to get the actual request struct.
    let request = match unsafe { request_ptr.as_ref() } {
        Some(req) => req,
        None => {
            // This case implies request_ptr was not null but still couldn't be dereferenced (e.g., misaligned, invalid memory).
            error!("FFI Error in perform_search: Failed to dereference request_ptr (was not null but as_ref() failed).");
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char("Failed to dereference request_ptr".to_string()).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::NullArgument;
        }
    };

    // DO NOT use a local `response` variable like `let response = &mut *response_ptr;` anymore.
    // All assignments must be to a new SearchResponseFfi that is then Boxed and its raw pointer assigned to *response_ptr.

    // Convert table_name C string to Rust String early
    let table_name_str = match unsafe { c_char_to_string(request.table_name, "table_name") } {
        Ok(name) => name,
        Err(e) => {
            error!("FFI Error in perform_search_ffi (table_name): {}", e);
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char(format!("Invalid table_name: {}", e)).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::from(&e);
        }
    };
    info!("FFI perform_search_ffi: Requested table_name: '{}'", table_name_str);

    // TODO: Validate other request fields (e.g., embedding_ptr not null, embedding_dim > 0)
    // Ensure embedding_ptr is valid for the given embedding_dim
    if request.embedding_ptr.is_null() || request.embedding_dim == 0 {
        error!("FFI Error in perform_search: Invalid embedding_ptr or embedding_dim");
        let mut err_resp = SearchResponseFfi::default();
        err_resp.error_message = string_to_c_char("Invalid embedding_ptr or embedding_dim".to_string()).unwrap_or(ptr::null_mut());
        *response_ptr = Box::into_raw(Box::new(err_resp));
        return FfiResultCode::InvalidArgument;
    }
    let query_vector_slice: &[f32] = unsafe { slice::from_raw_parts(request.embedding_ptr, request.embedding_dim as usize) };
    let owned_query_vec: Vec<f32> = query_vector_slice.iter().map(|&x| x).collect();

    // Get a lock on the global state
    let mut state = match LANCE_DB_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            error!("FFI Error in perform_search: Mutex poisoned: {}", poisoned);
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char(format!("Mutex poisoned: {}", poisoned)).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::InternalError;
        }
    };

    let connection: Arc<lancedb::connection::Connection> = match state.connection.as_ref() {
        Some(conn) => Arc::clone(conn), // Clone Arc to use connection
        None => {
            error!("FFI Error in perform_search: LanceDB connection not initialized.");
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char("LanceDB connection not initialized.".to_string()).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::InitializationFailed;
        }
    };

    // Table caching logic
    let table: Arc<lancedb::Table> = if let Some(cached_table) = state.table_cache.get(&table_name_str) {
        info!("FFI: Using cached table '{}'", table_name_str);
        Arc::clone(cached_table)
    } else {
        info!("FFI: Table '{}' not in cache. Opening...", table_name_str);
        let new_table_result: Result<Arc<lancedb::Table>, LanceDbErrorExt> = TOKIO_RUNTIME.block_on(async {
            connection.open_table(&table_name_str).execute().await.map(Arc::new)
        });
        let opened_table = match new_table_result {
            Ok(tbl) => tbl,
            Err(e) => {
                error!("FFI Error in perform_search: Failed to open table '{}': {}", table_name_str, e);
                let mut err_resp = SearchResponseFfi::default();
                err_resp.error_message = string_to_c_char(format!("Failed to open table '{}': {}", table_name_str, e)).unwrap_or(ptr::null_mut());
                *response_ptr = Box::into_raw(Box::new(err_resp));
                return FfiResultCode::from(&FfiError::LanceDbError(e));
            }
        };
        state.table_cache.insert(table_name_str.clone(), Arc::clone(&opened_table));
        info!("FFI: Opened and cached table '{}'", table_name_str);
        opened_table
    };

    // --- Placeholder for actual search and result conversion --- 
    info!("FFI: Performing search on table '{}' with top_k: {}", table_name_str, request.top_k);
    // TODO: Execute the search using TOKIO_RUNTIME.block_on(table.search(query_vector)...)
    // TODO: Convert LanceDB search results to Vec<SearchResultItemFfi>
    // This involves:
    //   1. Iterating over LanceDB results (e.g., RecordBatch).
    //   2. For each result item:
    //      a. Extracting the ID (ensure it's a string, or convert if necessary).
    //      b. Extracting the score/similarity.
    //      c. CRITICAL: Extracting or constructing the metadata_json string.
    //         This JSON string MUST conform to the FfiMetadata struct in search/src/lancedb_provider.rs,
    //         meaning it must have "source" and "ask_method_code" fields.
    //         Example: let metadata_json = format!("{{\"source\": \"{}\", \"ask_method_code\": \"{}\"}}", actual_source, actual_ask_code);
    //      d. Converting id_str and metadata_json_str to *mut c_char using string_to_c_char.
    //   3. Populating response.results with a pointer to the Vec<SearchResultItemFfi> (after converting Vec to raw parts).
    //   4. Setting response.num_results.

    // --- Actual search and result conversion --- 

    let search_result_batches: Result<Vec<RecordBatch>, LanceDbErrorExt> = TOKIO_RUNTIME.block_on(async {
        let query_slice_for_search: &[f32] = owned_query_vec.as_slice();
        let vector_query_builder = (*table).vector_search(query_slice_for_search)
            .map_err(|e| LanceDbErrorExt::Runtime { message: format!("Failed to build vector search query: {}", e) })?;
        let stream = vector_query_builder.limit(request.top_k as usize)
            .distance_type(lancedb::DistanceType::Cosine) // Specify Cosine distance using the correct method and enum
            // .nprobes(10) // Example: add if needed
            // .refine_factor(2) // Example: add if needed
            .execute()
            .await.map_err(|e| LanceDbErrorExt::Runtime { message: format!("Failed to execute search stream: {}", e) })?;
        stream.try_collect().await.map_err(|e| LanceDbErrorExt::Runtime { message: format!("Failed to collect search stream: {}", e) })
    });

    let batches = match search_result_batches {
        Ok(b) => b,
        Err(e) => {
            error!("FFI: LanceDB search execution or stream collection failed: {}", e);
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char(format!("LanceDB search/collect failed: {}", e)).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::from(&FfiError::LanceDbError(e));
        }
    };

    if batches.is_empty() {
        info!("FFI: Search returned no batches.");
        let success_resp = SearchResponseFfi::default(); // Default is no results, no error
        *response_ptr = Box::into_raw(Box::new(success_resp));
        return FfiResultCode::Success;
    }

    // For simplicity, concatenate all batches into one. 
    // A more sophisticated approach might process them iteratively if memory is a concern.
    // This assumes `lancedb::arrow::utils::concatenate_batches` or similar exists or we implement it.
    // For now, let's just process the first batch if available, or error if schema mismatch etc.
    // TODO: Properly concatenate batches if multiple are returned and schema is compatible.
    let batch = if batches.len() > 1 {
        warn!("FFI: Search returned multiple batches ({}). Processing only the first. Proper concatenation TODO.", batches.len());
        &batches[0]
    } else {
        &batches[0]
    };

    if batch.num_rows() == 0 {
        info!("FFI: Search returned an empty batch.");
        let success_resp = SearchResponseFfi::default(); // Default is no results, no error
        *response_ptr = Box::into_raw(Box::new(success_resp));
        return FfiResultCode::Success;
    }

    let mut ffi_results_vec: Vec<SearchResultItemFfi> = Vec::new();

    let single_batch_items_result: Result<Vec<SearchResultItemFfi>, String> = (|| {
        let mut current_batch_ffi_items: Vec<SearchResultItemFfi> = Vec::with_capacity(batch.num_rows());

        let id_array_arc = batch.column_by_name("expand_id")
            .ok_or_else(|| "FFI: Column 'expand_id' not found".to_string())?;
        let source_table_array_arc = batch.column_by_name("source_table")
            .ok_or_else(|| "FFI: Column 'source_table' not found".to_string())?;
        let similarities_array_arc = batch.column_by_name("_distance")
            .ok_or_else(|| "FFI: Column '_distance' not found".to_string())?;
        let ask_method_code_array_arc = batch.column_by_name("ask_method_code")
            .ok_or_else(|| "FFI: Column 'ask_method_code' not found".to_string())?;

        let id_values: Vec<String> = if let Some(string_array) = id_array_arc.as_any().downcast_ref::<StringArray>() {
            string_array.iter().map(|val| val.unwrap_or("").to_string()).collect()
        } else if let Some(int_array) = id_array_arc.as_any().downcast_ref::<arrow_array::Int64Array>() { // Explicitly specify Int64Array path
            int_array.iter().map(|val| val.unwrap_or(0).to_string()).collect()
        } else {
            return Err("FFI: Type mismatch for column 'expand_id', expected StringArray or Int64Array".to_string());
        };

        let source_table_array = source_table_array_arc.as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| "FFI: Type mismatch for column 'source_table', expected StringArray".to_string())?;
        let similarities_array = similarities_array_arc.as_any().downcast_ref::<Float32Array>()
            .ok_or_else(|| "FFI: Type mismatch for column '_distance', expected Float32Array".to_string())?;
        let ask_method_code_array = ask_method_code_array_arc.as_any().downcast_ref::<StringArray>()
            .ok_or_else(|| "FFI: Type mismatch for column 'ask_method_code', expected StringArray".to_string())?;

        for i in 0..batch.num_rows() {
            let id_str = id_values[i].clone(); // Use the collected and converted id_values
            let source_table_str = source_table_array.value(i).to_string();
            let distance_val = similarities_array.value(i);
            let ask_method_code_str = ask_method_code_array.value(i).to_string();

            let c_id = string_to_c_char(id_str.clone())
                .map_err(|e| format!("FFI: Failed to convert ID '{}' to CString: {}", id_str, e))?;
            
            let metadata_str = format!("{{\"source\": \"{}\", \"ask_method_code\": \"{}\"}}", source_table_str, ask_method_code_str);
            let c_metadata = string_to_c_char(metadata_str.clone())
                .map_err(|e| format!("FFI: Failed to convert metadata '{}' to CString: {}", metadata_str, e))?;

            current_batch_ffi_items.push(SearchResultItemFfi {
                id: c_id,
                distance: distance_val, // Use the raw distance value
                metadata_json: c_metadata,
            });
        }
        Ok(current_batch_ffi_items)
    })();

    match single_batch_items_result {
        Ok(new_items) => {
            ffi_results_vec.extend(new_items);
        }
        Err(e_str) => {
            error!("FFI: Error processing batch: {}", e_str);
            // Cleanup items from ffi_results_vec (from previous successful batches)
            for item_to_free in ffi_results_vec {
                free_ffi_string(item_to_free.id);
                free_ffi_string(item_to_free.metadata_json);
            }
            // Also clean up any items potentially made in the failing batch before the error
            // (though the closure design should prevent this if an error occurs mid-batch)
            let mut err_resp = SearchResponseFfi::default();
            err_resp.error_message = string_to_c_char(e_str).unwrap_or(ptr::null_mut());
            *response_ptr = Box::into_raw(Box::new(err_resp));
            return FfiResultCode::InternalError; // Return from the outer perform_search_ffi function
        }
    }

    // Final success path: Box the ffi_results_vec and return
    let mut success_resp = SearchResponseFfi::default();
    if ffi_results_vec.is_empty() {
        info!("FFI: Processed results but ffi_results_vec is empty (or all conversions failed before push).");
    } else {
        let mut ffi_results_boxed_slice = ffi_results_vec.into_boxed_slice();
        success_resp.results = ffi_results_boxed_slice.as_mut_ptr();
        success_resp.num_results = ffi_results_boxed_slice.len();
        std::mem::forget(ffi_results_boxed_slice);
    }
    *response_ptr = Box::into_raw(Box::new(success_resp));
    FfiResultCode::Success
}

/// Frees the memory allocated for `SearchResponseFfi` and its contents.
/// `response_ptr`: Pointer to the `SearchResponseFfi` struct to be freed.
#[no_mangle]
pub unsafe extern "C" fn free_search_response_ffi(response_ptr: *mut SearchResponseFfi) {
    if response_ptr.is_null() {
        return;
    }
    let response = &*response_ptr;

    // Free the error_message string if it's not null
    if !response.error_message.is_null() {
        drop(CString::from_raw(response.error_message as *mut _));
    }

    // Free each SearchResultItemFfi and its string fields
    if !response.results.is_null() && response.num_results > 0 {
        let results_slice = slice::from_raw_parts_mut(response.results, response.num_results);
        for item in results_slice {
            if !item.id.is_null() {
                drop(CString::from_raw(item.id as *mut _));
            }
            if !item.metadata_json.is_null() {
                drop(CString::from_raw(item.metadata_json as *mut _));
            }
        }
        // Free the array of SearchResultItemFfi itself
        // This was created from a Vec, so we reconstruct the Vec and let it drop.
        let _ = Vec::from_raw_parts(response.results, response.num_results, response.num_results);
    }
    info!("FFI: free_search_response_ffi called and memory potentially freed.");
    // Note: The `response_ptr` itself is typically managed by the C caller, so we don't free it here.
    // If C allocated the SearchResponseFfi struct, C must free it.
    // If Rust allocated it and passed ownership (e.g. Box::into_raw), then C would call a Rust function to free it.
    // In our current design, C passes a pointer to a C-allocated or stack-allocated struct.
}

/// Frees a C string that was allocated by Rust and passed to C.
/// `s_ptr`: Pointer to the C string to be freed.
#[no_mangle]
pub unsafe extern "C" fn free_ffi_string(s_ptr: *mut c_char) {
    if !s_ptr.is_null() {
        drop(CString::from_raw(s_ptr));
    }
}

/// Placeholder for shutdown logic.
/// This function should close any open LanceDB connections/tables and release resources.
#[no_mangle]
pub extern "C" fn shutdown_search_engine_ffi() -> FfiResultCode {
    info!("FFI: shutdown_search_engine_ffi called");
    // TODO: Implement shutdown logic for LANCE_DB_CONNECTION and LANCE_DB_TABLE
    // unsafe {
    //     if let Some(table) = LANCE_DB_TABLE.take() {
    //         // Perform any cleanup for table if necessary
    //     }
    //     if let Some(conn) = LANCE_DB_CONNECTION.take() {
    //         // Perform any cleanup for connection if necessary (e.g., conn.close() if available)
    //     }
    // }
    FfiResultCode::Success
}

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
