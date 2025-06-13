use std::os::raw::c_char;

/// FFI 兼容的单个搜索结果项。
/// 注意：字符串字段同样由 FFI 边界的另一端管理内存。
#[repr(C)]
#[derive(Debug)]
pub struct SearchResultItemFfi {
    /// C 风格的字符串，表示结果的 ID。
    pub id: *mut c_char,
    /// 结果的距离或得分（根据查询类型而定）。
    pub distance: f32,
    /// C 风格的字符串，表示元数据（例如 JSON 格式）。
    pub metadata_json: *mut c_char,
}

/// FFI 函数返回的结果代码枚举。
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum FfiResultCode {
    Success = 0,
    NullArgument = -1,
    InvalidArgument = -2,
    Utf8Error = -3,
    JsonError = -4,
    LanceDbError = -5,
    InitializationFailed = -6,
    SearchFailed = -7,
    MemoryAllocationFailed = -8,
    InternalError = -99,
}

// --- New Object-Based API Types ---

/// An opaque type that represents a `SearchEngine` instance across the FFI boundary.
#[repr(C)]
pub struct SearchEngineHandle {
    _private: [u8; 0],
}

/// Configuration struct for creating a `SearchEngine`.
#[repr(C)]
#[derive(Debug)]
pub struct SearchEngineConfigFfi {
    /// C-style string pointing to the database URI.
    pub db_uri: *const c_char,
}

/// Request struct for the object-based search API.
#[repr(C)]
#[derive(Debug)]
pub struct SearchRequestObjectFfi {
    pub embedding_ptr: *const f32,
    pub embedding_dim: u32,
    pub top_k: u32,
    pub table_name: *const c_char,
    pub id_column: *const c_char,
    pub source_column: *const c_char,
    pub distance_column: *const c_char,
    pub ask_method_code_column: *const c_char,
}

extern "C" {
    //
    // Search Engine Lifecycle
    //

    pub fn search_engine_new(config_ptr: *const SearchEngineConfigFfi) -> *mut SearchEngineHandle;
    pub fn search_engine_drop(engine_ptr: *mut SearchEngineHandle);

    //
    // Search Operations
    //

    pub fn search_engine_search_sync(
        engine_ptr: *mut SearchEngineHandle,
        request_ptr: *const SearchRequestObjectFfi,
        results_out: *mut *mut SearchResultItemFfi,
        num_results_out: *mut usize,
    ) -> FfiResultCode;

    pub fn free_search_results_ffi(results: *mut SearchResultItemFfi, num_results: usize);

    //
    // Error Handling
    //

    /// Retrieves the last error message from the current thread.
    ///
    /// The caller owns the returned string and must free it with `lancedb_ffi_free_string`.
    /// Returns a null pointer if there is no error.
    pub fn lancedb_ffi_get_last_error() -> *mut c_char;

    /// Frees a C string that was allocated by the FFI layer (e.g., via `lancedb_ffi_get_last_error`).
    pub fn lancedb_ffi_free_string(s: *mut c_char);
}
