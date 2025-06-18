use std::env;
use std::path::PathBuf;

fn main() {
    // Get the root directory of the lancedb_ffi crate.
    let lancedb_ffi_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .join("lancedb_ffi");

    // Determine the current build profile (debug/release).
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let lancedb_ffi_target_dir = lancedb_ffi_root.join("target").join(&profile);

    // Tell cargo to search for the library in the lancedb_ffi target directory.
    println!(
        "cargo:rustc-link-search=native={}",
        lancedb_ffi_target_dir.display()
    );

    // Tell cargo to link against the lancedb_ffi static library.
    println!("cargo:rustc-link-lib=static=lancedb_ffi");
}
