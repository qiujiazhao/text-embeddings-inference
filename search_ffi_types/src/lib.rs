
use std::os::raw::c_char;

/// FFI 兼容的搜索请求结构体。
/// 注意：所有字符串 (`*const c_char`) 都应该是有效的 UTF-8 编码，
/// 并且由调用者（通常是 C/C++ 或其他语言通过 FFI）负责管理其生命周期。
/// `lancedb_ffi` crate 在接收到这些指针后，应立即将其转换为 Rust 的 String 类型。
#[repr(C)]
#[derive(Debug)]
pub struct SearchRequestFfi {
    /// 指向浮点数数组（嵌入向量）的指针。
    pub embedding_ptr: *const f32,
    /// 嵌入向量的维度。
    pub embedding_dim: u32,
    /// 要返回的顶部结果数量。
    pub top_k: u32,
    /// C 风格的字符串，表示要搜索的表名。
    pub table_name: *const c_char,
    // 可以在这里添加其他参数，例如过滤器（JSON 字符串形式）。
    // pub filters_json: *const c_char,
}

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

/// FFI 兼容的搜索响应结构体。
/// 注意：`results` 是一个指向 `SearchResultItemFfi` 数组的指针。
/// `lancedb_ffi` crate 在创建此结构并填充数据后，需要提供一个相应的释放函数，
/// 以便调用者可以安全地释放分配给 `results` 和其中字符串的内存。
#[repr(C)]
#[derive(Debug)]
pub struct SearchResponseFfi {
    /// 指向 `SearchResultItemFfi` 数组的指针。
    pub results: *mut SearchResultItemFfi,
    /// `results` 数组中的元素数量。
    pub num_results: usize,
    /// C 风格的字符串，表示错误信息。如果操作成功，则为 null。
    pub error_message: *mut c_char,
}

impl Default for SearchResponseFfi {
    fn default() -> Self {
        SearchResponseFfi {
            results: std::ptr::null_mut(),
            num_results: 0,
            error_message: std::ptr::null_mut(),
        }
    }
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
