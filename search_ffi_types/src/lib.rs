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
}

/*
// 未来 `lancedb_ffi` crate 将会实现这些 FFI 函数。
// 我们可以在这里（或者在 `lancedb_ffi` crate 中）声明它们，以便 `search` crate 可以链接。
// 例如：
extern "C" {
    /// 初始化搜索引擎。
    /// config_json: 一个 JSON 字符串，包含初始化所需的配置。
    /// 返回 FfiResultCode。
    // pub fn init_search_engine(config_json: *const c_char) -> FfiResultCode;

    /// 执行搜索。
    /// request: 指向 SearchRequestFfi 结构的指针。
    /// response: 指向 SearchResponseFfi 结构的指针，函数将填充此结构。
    /// 返回 FfiResultCode。
    // pub fn perform_search(request: *const SearchRequestFfi, response: *mut SearchResponseFfi) -> FfiResultCode;

    /// 释放由 perform_search 分配的 SearchResponseFfi 结构及其内部数据。
    /// response: 指向需要释放的 SearchResponseFfi 结构的指针。
    // pub fn free_search_response(response: *mut SearchResponseFfi);

    /// 关闭搜索引擎并释放所有相关资源。
    // pub fn shutdown_search_engine() -> FfiResultCode;

    /// 释放由 FFI 函数返回的（且不由 SearchResponseFfi 管理的）C 字符串。
    /// 一般用于释放 error_message 或 SearchResultItemFfi 中的字符串字段（如果它们是单独分配的话）。
    /// 通常，更好的做法是让 free_search_response 负责所有相关内存。
    // pub fn free_ffi_string(s: *mut c_char);
}*/
