use search_ffi_types::{
    FfiResultCode, SearchEngineConfigFfi, SearchEngineHandle, SearchRequestObjectFfi,
    SearchResultItemFfi,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use thiserror::Error;
use tracing::{error, info};
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use lancedb::{Error as LanceDbErrorExt}; // Renamed to avoid conflict with FfiError::LanceDbError if any
use tokio::runtime::{Runtime, Builder as RuntimeBuilder};
use futures::stream::TryStreamExt;
use arrow_array::{RecordBatch, array::{StringArray, Float32Array}};
use lancedb::query::QueryBase;
use lancedb::query::ExecutableQuery;
use std::cell::RefCell;

// Global Tokio runtime for executing async LanceDB operations
static TOKIO_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    RuntimeBuilder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime for LanceDB FFI")
});

thread_local! {
    /// Holds the last error produced by an FFI call on the current thread.
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

/// Sets the last error for the current thread. The message is stored in a CString.
fn set_last_error(err: FfiError) {
    error!("FFI Error: {}", err); // Log the error for debugging purposes.
    let error_message = CString::new(err.to_string()).unwrap_or_else(|_| {
        // This fallback should rarely happen.
        CString::new("Error message contained null bytes and could not be converted.").unwrap()
    });
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(error_message);
    });
}


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
/// Caller is responsible for freeing the C string using `lancedb_ffi_free_string`.
fn string_to_c_char(s: String) -> Result<*mut c_char, FfiError> {
    CString::new(s).map(|cs| cs.into_raw()).map_err(FfiError::from)
}


/// Frees a C string that was allocated by Rust and passed to C.
/// `s_ptr`: Pointer to the C string to be freed.
#[no_mangle]
pub unsafe extern "C" fn lancedb_ffi_free_string(s_ptr: *mut c_char) {
    if !s_ptr.is_null() {
        drop(CString::from_raw(s_ptr));
    }
}

/// Retrieves the last error message from the current thread.
/// The caller owns the returned string and must free it with `lancedb_ffi_free_string`.
/// Returns a null pointer if there is no error.
#[no_mangle]
pub unsafe extern "C" fn lancedb_ffi_get_last_error() -> *mut c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow_mut()
            .take()
            .map_or(ptr::null_mut(), |s| s.into_raw())
    })
}

// The outdated test module below is being removed as it refers to obsolete FFI functions.
// --- New Object-Based API Implementation ---

/// The actual implementation behind the opaque `SearchEngineHandle`.
pub struct SearchEngine {
    connection: Arc<lancedb::connection::Connection>,
    table_cache: RwLock<HashMap<String, Arc<lancedb::Table>>>,
    runtime_handle: tokio::runtime::Handle,
}

impl SearchEngine {
    /// Opens a table, using a cache if possible. This is thread-safe.
    fn get_table(&self, name: &str) -> Result<Arc<lancedb::Table>, FfiError> {
        // First, check with a read lock, which is cheap and can be shared.
        let read_guard = self.table_cache.read().unwrap();
        if let Some(table) = read_guard.get(name) {
            return Ok(Arc::clone(table));
        }
        // Drop the read lock so a write lock can be acquired.
        drop(read_guard);

        // If not found, acquire a write lock. This is exclusive.
        let mut write_guard = self.table_cache.write().unwrap();
        // We must check again, as another thread might have acquired the write
        // lock and inserted the table while we were waiting.
        if let Some(table) = write_guard.get(name) {
            return Ok(Arc::clone(table));
        }

        // The table is definitely not in the cache, and we have the lock.
        // Let's open it and put it in the cache.
        let conn = Arc::clone(&self.connection);
        match self
            .runtime_handle
            .block_on(conn.open_table(name).execute())
        {
            Ok(opened_table) => {
                let table_arc = Arc::new(opened_table);
                write_guard
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
        &self,
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
        set_last_error(FfiError::NullArgument("config_ptr".to_string()));
        return ptr::null_mut();
    }
    let config = &*config_ptr;

    let db_uri = match c_char_to_string(config.db_uri, "db_uri") {
        Ok(uri) => uri,
        Err(e) => {
            error!("FFI Error in search_engine_new: Invalid db_uri: {}", e);
            set_last_error(e);
            return ptr::null_mut();
        }
    };

    let connection = match TOKIO_RUNTIME.block_on(lancedb::connect(&db_uri).execute()) {
        Ok(conn) => Arc::new(conn),
        Err(e) => {
            let ffi_error = FfiError::InitializationFailed(format!(
                "Failed to connect to LanceDB at {}: {}",
                db_uri, e
            ));
            error!("FFI Error in search_engine_new: {}", ffi_error);
            set_last_error(ffi_error);
            return ptr::null_mut();
        }
    };

    let engine = SearchEngine {
        connection,
        table_cache: RwLock::new(HashMap::new()),
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
        let err = FfiError::NullArgument("A required pointer argument is null.".to_string());
        set_last_error(err);
        error!("FFI Error in search_engine_search_sync: A required pointer argument is null.");
        return FfiResultCode::NullArgument;
    }

    // Initialize output parameters to safe values
    *results_out = ptr::null_mut();
    *num_results_out = 0;

    let engine = &*(engine_ptr as *mut SearchEngine);
    let request = &*request_ptr;

    // --- Input Conversion ---
    macro_rules! get_string {
        ($ptr:expr, $name:expr) => {
            match c_char_to_string($ptr, $name) {
                Ok(s) => s,
                Err(e) => {
                    set_last_error(e);
                    return FfiResultCode::from(&e);
                }
            }
        };
    }

    let table_name = get_string!(request.table_name, "table_name");
    let id_column = get_string!(request.id_column, "id_column");
    let source_column = get_string!(request.source_column, "source_column");
    let distance_column = get_string!(request.distance_column, "distance_column");
    let ask_method_code_column = get_string!(request.ask_method_code_column, "ask_method_code_column");

    if request.embedding_ptr.is_null() || request.embedding_dim == 0 {
        let err = FfiError::NullArgument("embedding_ptr is null or embedding_dim is 0".to_string());
        set_last_error(err);
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
                let conversion_params = ColumnConversionParams {
                    id_column: &id_column,
                    source_column: &source_column,
                    distance_column: &distance_column,
                    ask_method_code_column: &ask_method_code_column,
                };
                match convert_batch_to_ffi_items(&batch, conversion_params) {
                    Ok(items) => ffi_results.extend(items),
                    Err(e) => {
                        set_last_error(e);
                        // In case of partial success, we should free what we've allocated so far
                        // before returning an error.
                        for item in ffi_results {
                            if !item.id.is_null() { drop(CString::from_raw(item.id)); }
                            if !item.metadata_json.is_null() { drop(CString::from_raw(item.metadata_json)); }
                        }
                        return FfiResultCode::from(&FfiError::InternalError("Batch conversion failed".to_string()));
                    }
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
            set_last_error(e);
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


struct ColumnConversionParams<'a> {
    id_column: &'a str,
    source_column: &'a str,
    distance_column: &'a str,
    ask_method_code_column: &'a str,
}

// Helper function to convert a RecordBatch to FFI items.
// This should be adapted from your existing logic.
fn convert_batch_to_ffi_items(batch: &RecordBatch, params: ColumnConversionParams) -> Result<Vec<SearchResultItemFfi>, FfiError> {
    let mut items = Vec::with_capacity(batch.num_rows());

    macro_rules! get_column {
        ($name:expr) => {
            batch.column_by_name($name).ok_or_else(|| {
                FfiError::InternalError(format!("Column '{}' not found", $name))
            })?
        };
    }

    let id_array_arc = get_column!(params.id_column);
    let source_table_array_arc = get_column!(params.source_column);
    let similarities_array_arc = get_column!(params.distance_column);
    let ask_method_code_array_arc = get_column!(params.ask_method_code_column);

    let id_values: Vec<String> =
        if let Some(string_array) = id_array_arc.as_any().downcast_ref::<StringArray>() {
            string_array.iter().map(|v| v.unwrap_or("").to_string()).collect()
        } else if let Some(int_array) = id_array_arc.as_any().downcast_ref::<arrow_array::Int64Array>() {
            int_array.iter().map(|v| v.unwrap_or(0).to_string()).collect()
        } else {
            return Err(FfiError::InternalError(format!("Type mismatch for '{}'", params.id_column)));
        };

    let source_table_array = source_table_array_arc.as_any().downcast_ref::<StringArray>()
        .ok_or_else(|| FfiError::InternalError(format!("Type mismatch for '{}'", params.source_column)))?;
    let similarities_array = similarities_array_arc.as_any().downcast_ref::<Float32Array>()
        .ok_or_else(|| FfiError::InternalError(format!("Type mismatch for '{}'", params.distance_column)))?;
    let ask_method_code_array = ask_method_code_array_arc.as_any().downcast_ref::<StringArray>()
        .ok_or_else(|| FfiError::InternalError(format!("Type mismatch for '{}'", params.ask_method_code_column)))?;

    for i in 0..batch.num_rows() {
        let id_str = id_values[i].clone();
        let source_table_str = source_table_array.value(i).to_string();
        let distance_val = similarities_array.value(i);
        let ask_method_code_str = ask_method_code_array.value(i).to_string();

        let c_id = string_to_c_char(id_str.clone())
            .map_err(|e| FfiError::InternalError(format!("FFI: Failed to convert ID '{}' to CString: {}", id_str, e)))?;
        
        let metadata_str = format!(
            "{{\"source\": \"{}\", \"ask_method_code\": \"{}\"}}",
            source_table_str, ask_method_code_str
        );
        let c_metadata = string_to_c_char(metadata_str.clone())
            .map_err(|e| FfiError::InternalError(format!("FFI: Failed to convert metadata '{}' to CString: {}", metadata_str, e)))?;

        items.push(SearchResultItemFfi {
            id: c_id,
            distance: distance_val,
            metadata_json: c_metadata,
        });
    }

    Ok(items)
}