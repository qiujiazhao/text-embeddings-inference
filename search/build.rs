use std::env;
use std::path::PathBuf;

fn main() {
    // 获取 lancedb_ffi crate 的根目录
    let lancedb_ffi_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .join("lancedb_ffi");

    // 根据当前的构建配置（debug/release）确定 lancedb_ffi 的 target 目录
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let lancedb_ffi_target_dir = lancedb_ffi_root.join("target").join(profile);

    // 告诉 cargo 在 lancedb_ffi 的 target 目录中查找库
    println!("cargo:rustc-link-search=native={}", lancedb_ffi_target_dir.display());

    // 告诉 cargo 链接 lancedb_ffi 静态库
    // Cargo 会自动根据操作系统添加 "lib" 前缀和 ".a" 或 ".lib" 后缀
    println!("cargo:rustc-link-lib=static=lancedb_ffi");

    // 如果 lancedb_ffi/Cargo.toml 或其任何源文件发生变化，重新运行此构建脚本
    println!("cargo:rerun-if-changed={}/Cargo.toml", lancedb_ffi_root.display());
    println!("cargo:rerun-if-changed={}/src/lib.rs", lancedb_ffi_root.display());
    // 也可以考虑监视 lancedb_ffi 的 target 目录中的静态库文件本身
    // 注意：静态库的文件名可能因操作系统而异（例如 liblancedb_ffi.a 或 lancedb_ffi.lib）
    // 为了简单起见，我们先监视目录，或者更精确地监视已知的文件名。
    // 假设是 .a 文件，对于 macOS/Linux
    let lib_file_path = lancedb_ffi_target_dir.join("liblancedb_ffi.a");
    if lib_file_path.exists() {
        println!("cargo:rerun-if-changed={}", lib_file_path.display());
    }

    // 确保 lancedb_ffi 已经被编译
    // 这通常由依赖关系处理，但如果作为独立步骤编译 lancedb_ffi，
    // 确保在编译 search 之前 lancedb_ffi 已经构建完成。
}
