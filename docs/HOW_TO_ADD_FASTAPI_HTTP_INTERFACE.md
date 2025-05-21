# How to Add a New FastAPI HTTP Interface to the Python Backend

## 1. Overview

New general-purpose HTTP interfaces, especially those involving Python-specific libraries like FastAPI and LanceDB, should be implemented as part of the Python backend. This document outlines the steps to create and integrate such an interface.

## 2. Create a New Python File

It's recommended to create a new Python file for your FastAPI application. For example, you could place it in `backends/python/server/text_embeddings_server/search_api.py`.

Here's a basic FastAPI app structure to get you started:

```python
from fastapi import FastAPI, Request
from pydantic import BaseModel
import uvicorn

app = FastAPI()

class Item(BaseModel):
    name: str
    price: float

@app.get("/")
async def root():
    return {"message": "Hello from FastAPI"}

@app.post("/items/")
async def create_item(item: Item):
    return item

if __name__ == "__main__":
    # This is for local development/testing
    # In production, the Rust launcher might start this differently
    # or it might be managed by a process manager like gunicorn.
    uvicorn.run(app, host="0.0.0.0", port=8001) # Example port
```

## 3. Define Pydantic Models

Pydantic models are crucial for request and response validation and data serialization. They ensure that the data your API receives and sends is in the correct format.

In the example above, `Item` is a Pydantic model:

```python
class Item(BaseModel):
    name: str
    price: float
```

This model defines that an `Item` object must have a `name` of type `str` and a `price` of type `float`. FastAPI uses these models to automatically validate incoming request bodies and to serialize response data.

## 4. Create Endpoints

Endpoints are defined using FastAPI decorators like `@app.get`, `@app.post`, `@app.put`, `@app.delete`, etc. These decorators associate a URL path and HTTP method with your Python functions.

Example:

*   `@app.get("/")`: Defines a GET endpoint at the root path (`/`).
*   `@app.post("/items/")`: Defines a POST endpoint at the `/items/` path. The `create_item` function expects a request body that conforms to the `Item` Pydantic model.

## 5. Running the FastAPI App

There are multiple ways to run your FastAPI application:

*   **Development/Testing**: You can run the FastAPI app directly using Python:
    ```bash
    python backends/python/server/text_embeddings_server/search_api.py
    ```
    This will start a Uvicorn server on the host and port specified in your `uvicorn.run()` call (e.g., `0.0.0.0:8001`).

*   **Integration with the Main Application (Rust Process)**: For the main application to manage this FastAPI service, the Rust process (`router/src/main.rs`) needs to be modified:
    *   **CLI Argument for Port**: Optionally, add a new CLI argument to the Rust application to specify the port for this new FastAPI service. This allows dynamic port allocation if needed.
    *   **Launch as Subprocess**: The Rust application would launch the Python FastAPI script (e.g., `search_api.py`) as a separate subprocess. It would be responsible for managing the lifecycle of this subprocess.
    *   **Reverse Proxy (Optional)**: The main Axum server in Rust could act as a reverse proxy, routing certain requests to this FastAPI service. This is an advanced setup and not covered in this basic guide. Alternatively, the FastAPI service can run on its own port and be accessed directly by clients or other internal services.

## 6. Configuration

Configuration for your FastAPI application, such as database paths (e.g., LanceDB path), model paths, or other parameters, should be passed via CLI arguments to the Python script or through environment variables.

If you need to add new CLI arguments to your FastAPI script (e.g., `search_api.py`), you would typically use a library like `argparse` or `typer` within that script to parse them.

Example: Modifying `search_api.py` to accept a LanceDB path:

```python
# In search_api.py
import argparse
from fastapi import FastAPI
# ... other imports ...

app = FastAPI()

# ... your Pydantic models and endpoints ...

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="FastAPI Search API Server")
    parser.add_argument("--port", type=int, default=8001, help="Port to run the server on")
    parser.add_argument("--lancedb-path", type=str, required=True, help="Path to LanceDB data")
    # Add other arguments as needed
    args = parser.parse_args()

    # You can now use args.lancedb_path in your application logic
    # For example, initialize your LanceDB connection here

    uvicorn.run(app, host="0.0.0.0", port=args.port)
```

When launching this script, you would then pass the arguments:
`python search_api.py --port 8001 --lancedb-path data/my_lancedb`

## 7. Integration with the main `cli.py`

To manage the FastAPI service alongside other Python backend services, you can add a new command to the main CLI entrypoint, typically `backends/python/server/text_embeddings_server/cli.py`. This allows you to start the FastAPI server using the existing CLI structure.

Example of adding a `serve-search` command to `cli.py`:

```python
# In backends/python/server/text_embeddings_server/cli.py
# ... other imports
import typer
from typing_extensions import Annotated
from pathlib import Path
import subprocess
import logging # Assuming you have a logger configured

logger = logging.getLogger(__name__) # Or your specific logger instance
app = typer.Typer() # Assuming 'app' is your main Typer object

# ... other existing commands ...

@app.command()
def serve_search(
    # model_path: Annotated[Path, typer.Option(help="Path to the model checkpoint.")], # Example existing arg if needed by search_api.py
    lancedb_path: Annotated[str, typer.Option(help="Path to LanceDB data")] = "data/lancedb_ask_data",
    search_api_port: Annotated[int, typer.Option(help="Port for the search API FastAPI server")] = 8001
    # Add other necessary arguments that search_api.py expects
):
    """
    Launches the FastAPI server for search functionalities.
    """
    logger.info(f"Starting Search API FastAPI server on port {search_api_port} using LanceDB at {lancedb_path}...")

    # Construct the command to run your FastAPI app script
    # Ensure paths and the python executable are correct for your environment
    # For example, using sys.executable for the Python interpreter
    import sys
    cmd = [
        sys.executable, # Path to current python interpreter
        "backends/python/server/text_embeddings_server/search_api.py", # Relative path to your new FastAPI file
        "--port", str(search_api_port),
        "--lancedb-path", lancedb_path,
        # "--model-path", str(model_path), # If search_api.py needs it
        # Pass other arguments as required by search_api.py
    ]

    logger.info(f"Executing command: {' '.join(cmd)}")

    # In a real scenario, you'd manage this subprocess more robustly:
    # - Capture stdout/stderr for logging
    # - Handle process termination and errors
    # - Consider using asyncio.create_subprocess_exec for async applications
    process = subprocess.Popen(cmd)
    logger.info(f"Search API server process started with PID: {process.pid}.")

    # Note: This is a simple way to launch. For production, consider
    # how this integrates with the main Rust launcher and process management.
    # The Rust launcher might directly start search_api.py instead of going through this cli.py command,
    # passing necessary configurations directly.
```

**Explanation:**

*   A new command `serve-search` is added to the Typer application.
*   It takes arguments like `lancedb_path` and `search_api_port`.
*   It constructs a command list (`cmd`) to execute the `search_api.py` script with the necessary CLI arguments.
*   `subprocess.Popen` is used to start the FastAPI server as a background process.

The main Rust application (`router/src/main.rs`) would then be modified to:
1.  Potentially include a new CLI flag (e.g., `--enable-search-api`).
2.  If this flag is present, the Rust app would execute the Python CLI command `python -m backends.python.server.text_embeddings_server serve-search --search-api-port <port> --lancedb-path <path>` or directly execute the `search_api.py` script with the appropriate arguments. The choice depends on the desired level of indirection and whether other setup in `cli.py` is beneficial.

This approach centralizes the launching logic within the Python CLI, making it easier to manage Python-specific services.
Remember to adjust paths and arguments according to your project structure and the specific needs of your FastAPI application.
