# How to Add a New Rust CLI Option

## 1. Overview

Command-Line Interface (CLI) options for the main Rust application are defined in the `router/src/main.rs` file. The application utilizes the `clap` crate for parsing these arguments. This document provides a step-by-step guide on how to add a new CLI option.

## 2. Modifying `router/src/main.rs`

Adding a new CLI option involves modifying the `Args` struct and updating the `main` function to pass the parsed argument.

### Add a Field to `Args` Struct

New CLI arguments must be added as fields to the `Args` struct. This struct uses `clap` attributes to define how each argument is parsed.

**Example**: Adding a boolean flag `--new-feature-enabled` and an option `--new-feature-value` that takes a string:

```rust
// In router/src/main.rs

/// App Configuration
#[derive(Parser, Redact)]
#[clap(author, version, about, long_about = None)]
struct Args {
    // ... existing fields ...

    /// Enable the new feature.
    #[clap(long, env)] // Defines a --new-feature-enabled flag, also takes value from NEW_FEATURE_ENABLED env var
    new_feature_enabled: bool,

    /// Provide a value for the new feature.
    #[clap(default_value = "default_value_for_new_arg", long, env)] // Defines --new-feature-value, also from NEW_FEATURE_VALUE env var
    new_feature_value: String, // For arguments that take a value

    // Example of an optional argument with a value
    /// Optional configuration for another feature.
    // #[clap(long, env)]
    // optional_feature_config: Option<String>,
}
```

**Common `clap` attributes**:

*   `#[clap(long)]`: Creates a long flag (e.g., `--new-feature-enabled`).
*   `#[clap(short = 'n')]`: Creates a short flag (e.g., `-n`). Can be combined with `long`.
*   `#[clap(env = "NEW_FEATURE_ENABLED")]`: Allows the argument to be set via an environment variable (e.g., `NEW_FEATURE_ENABLED=true`). If only `env` is specified, `clap` will derive the environment variable name from the field name (e.g. `NEW_FEATURE_ENABLED` for `new_feature_enabled`).
*   `#[clap(default_value = "some_value")]`: Provides a default value if the argument is not supplied.
*   `Option<T>`: If the field type is `Option<String>`, `Option<i32>`, etc., the argument becomes optional. Without it, `clap` will expect the argument to be present unless a `default_value` is provided or it's a boolean flag (which defaults to `false`).

For a comprehensive list of attributes and their usage, please refer to the [official `clap` documentation](https://docs.rs/clap/).

### Pass the Argument in `main()`

Once the argument is added to the `Args` struct, it will be parsed by `Args::parse()`. You then need to pass this value to the relevant parts of your application, typically the `text_embeddings_router::run` function.

**Example**:

```rust
// In router/src/main.rs

fn main() -> Result<()> { // Assuming Result from anyhow or similar
    // Setup logging, etc.
    // ...

    let args: Args = Args::parse();

    // ... other existing code ...

    // Pass the new arguments to the run function
    text_embeddings_router::run(
        // ... existing arguments ...
        args.json_output, // Example existing argument
        args.new_feature_enabled, // Pass the new boolean flag
        args.new_feature_value,   // Pass the new argument with a value
        // ... other arguments ...
    )
    .await?;

    Ok(())
}
```

### Modify `text_embeddings_router::run` (if applicable)

If the new CLI argument influences the core router logic or needs to be passed to a backend service (like a Python server), you must update the signature of the `text_embeddings_router::run` function (and potentially other functions that call it). This function is typically located in `router/src/lib.rs` or a similar core module.

**Example (conceptual)**:

```rust
// In router/src/lib.rs (or where `run` is defined)

pub async fn run(
    // ... existing parameters ...
    json_output: bool, // Example existing parameter
    new_feature_enabled: bool, // New parameter for the boolean flag
    new_feature_value: String, // New parameter for the value
    // ... other parameters ...
) -> Result<()> { // Assuming Result from anyhow or similar
    // ... function body ...

    if new_feature_enabled {
        println!("New feature is enabled with value: {}", new_feature_value);
        // Implement logic related to the new feature.
        // This might involve configuring services, conditional logic,
        // or passing this value to a Python subprocess.
    }

    // ... rest of the function body ...
    Ok(())
}
```

Remember to update any calling sites of `text_embeddings_router::run` to pass the new argument(s).

## 3. Passing Arguments to Python Backend (if applicable)

If a CLI argument parsed by the Rust application is intended for a Python backend script, the Rust code must explicitly pass this value when it launches the Python subprocess.

This is commonly done by:

1.  **Adding it as a command-line argument to the Python script**:
    When constructing the `std::process::Command` to launch the Python script, use the `.arg()` or `.args()` methods to append the value.

    ```rust
    // In Rust, when preparing to launch a Python script
    use std::process::Command;

    let python_executable = "python"; // Or specific path to interpreter
    let script_path = "path/to/your/script.py";
    let lancedb_path_value = "/data/mydb"; // Value from parsed CLI arg

    let mut cmd = Command::new(python_executable);
    cmd.arg(script_path);
    cmd.arg("--lancedb-path");
    cmd.arg(lancedb_path_value);
    // Add other arguments for the Python script as needed

    // To pass the new_feature_value:
    // cmd.arg("--new-feature-value-for-python");
    // cmd.arg(new_feature_value_from_rust_cli);

    // Execute the command
    // let status = cmd.status().expect("Failed to execute Python script");
    ```

2.  **Setting an environment variable for the Python process**:
    Use the `.env()` method of `std::process::Command`.

    ```rust
    // In Rust
    use std::process::Command;

    let python_executable = "python";
    let script_path = "path/to/your/script.py";
    let lancedb_path_value = "/data/mydb";

    let mut cmd = Command::new(python_executable);
    cmd.arg(script_path);
    cmd.env("LANCEDB_PATH", lancedb_path_value);

    // To pass new_feature_value as an environment variable:
    // cmd.env("NEW_FEATURE_VALUE_FOR_PYTHON", new_feature_value_from_rust_cli);

    // Execute the command
    // let status = cmd.status().expect("Failed to execute Python script");
    ```

The Python script would then use a library like `argparse` (for CLI arguments) or `os.environ` (for environment variables) to access these values. Refer to `docs/HOW_TO_ADD_FASTAPI_HTTP_INTERFACE.md` for examples of argument parsing in Python.

For more details on `std::process::Command`, see the [official Rust documentation](https://doc.rust-lang.org/std/process/Command.html).

## 4. Updating Documentation

After adding a new CLI option, it's crucial to update any relevant user-facing documentation. This includes:

*   `docs/source/en/cli_arguments.md`: This file should list all available CLI arguments, their purpose, and how to use them.
*   Any other READMEs or guides that mention CLI usage.

Keeping documentation up-to-date ensures users are aware of new capabilities and how to configure the application.
```
