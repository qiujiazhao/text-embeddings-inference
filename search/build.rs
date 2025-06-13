use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Get the root directory of the lancedb_ffi crate.
    let lancedb_ffi_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .join("lancedb_ffi");

    // Determine the current build profile (debug/release).
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let lancedb_ffi_target_dir = lancedb_ffi_root.join("target").join(&profile);

    // --- Automatically build the FFI dependency ---
    // This makes the build process robust by ensuring the FFI lib is always
    // compiled with the same profile as the main crate.
    println!("cargo:rerun-if-changed={}/src", lancedb_ffi_root.display());
    println!("cargo:rerun-if-changed={}/Cargo.toml", lancedb_ffi_root.display());

    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if profile == "release" {
        cmd.arg("--release");
    }
    // Set the working directory for the build command.
    cmd.current_dir(&lancedb_ffi_root);

    // Execute the build command.
    let status = cmd.status().unwrap_or_else(|e| {
        panic!("Failed to execute cargo build for lancedb_ffi: {}", e);
    });

    if !status.success() {
        panic!("lancedb_ffi build failed with status: {}", status);
    }
    // --- End of automatic build ---

    // Tell cargo to search for the library in the lancedb_ffi target directory.
    println!(
        "cargo:rustc-link-search=native={}",
        lancedb_ffi_target_dir.display()
    );

    // Tell cargo to link against the lancedb_ffi static library.
    println!("cargo:rustc-link-lib=static=lancedb_ffi");
}
