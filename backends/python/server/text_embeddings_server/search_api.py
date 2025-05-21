from fastapi import FastAPI, Request
from pydantic import BaseModel
import lancedb
import time
from contextlib import asynccontextmanager
from sentence_transformers import SentenceTransformer
import uvicorn
import typer
from pathlib import Path
from typing import List # For SearchResponse type hint

# CLI app for configuration
cli_app = typer.Typer()

# Global variable to hold the model, will be initialized in lifespan
model = None
# lancedb_uri will be updated by the CLI command before the app starts.
# We use a placeholder here, but it's effectively set by the `main` function.
lancedb_uri: str = "data/lancedb_ask_data" # Default, will be updated by CLI

@asynccontextmanager
async def lifespan(app_instance: FastAPI): # Renamed 'app' to 'app_instance' to avoid conflict
    global model, lancedb_uri
    # Startup: Load model, connect to LanceDB, and initialize table cache
    print("Application startup: Loading model...")
    # model_path is passed via CLI and set on cli_app by the main() function
    # before uvicorn.run is called.
    # This ensures that the correct model_path is available here.
    if not hasattr(cli_app, 'model_path_for_sentence_transformer') or \
       not cli_app.model_path_for_sentence_transformer:
        raise RuntimeError("Model path not configured. Ensure --model-path is provided when starting.")
    
    model = SentenceTransformer(cli_app.model_path_for_sentence_transformer)
    print(f"Model loaded from {cli_app.model_path_for_sentence_transformer}")

    print(f"Application startup: Connecting to LanceDB at {lancedb_uri}...")
    # lancedb_uri is updated by the main() function from CLI args.
    app_instance.state.db_conn = await lancedb.connect_async(lancedb_uri)
    app_instance.state.tables_cache = {}  # Initialize an empty cache for tables
    print("LanceDB connection established and table cache initialized.")
    yield
    # Shutdown: Cleanup (LanceDB async connection might self-manage)
    print("Application shutdown.")

app = FastAPI(lifespan=lifespan)

class SearchQuery(BaseModel):
    question: str
    industry: str
    top_k: int

class SearchResponse(BaseModel):
    id: int
    source: str
    similarity: float
    ask_method_code: str

@app.post("/search")
async def search(request: Request, query: SearchQuery) -> List[SearchResponse]: # Added List type hint
    # Access connection and cache from app.state
    conn = request.app.state.db_conn
    tables_cache = request.app.state.tables_cache

    table_access_start_time = time.time()
    # Table Caching Logic
    if query.industry not in tables_cache:
        print(f"Table for industry '{query.industry}' not in cache. Opening...")
        tables_cache[query.industry] = await conn.open_table(query.industry)
        print(f"Opened and cached table for '{query.industry}'.")
    else:
        print(f"Using cached table for industry '{query.industry}'.")
    table = tables_cache[query.industry]

    print(f"Open table time (cached or new) for '{query.industry}': {time.time() - table_access_start_time:.6f}s")

    search_execution_start_time = time.time()
    # Note: model.encode can be blocking. For high-concurrency, consider running in a thread pool.
    # For this example, we'll keep it simple.
    embedding = model.encode(query.question) 
    
    # Revised search call for clarity and standard async LanceDB pattern
    search_obj = await table.search(embedding) # Returns an AsyncSearch object synchronously
    results = await search_obj.distance_type("cosine").limit(query.top_k).to_arrow() # Await the final operation
    print(f"Search execution time: {time.time() - search_execution_start_time:.6f}s")
    
    response_list = []
    if results.num_rows > 0:   
        ids = results["expand_id"].to_pylist()
        sources = results["source_table"].to_pylist()
        similarities = results["_distance"].to_pylist()
        codes = results["ask_method_code"].to_pylist()

        for i in range(results.num_rows):
            response_list.append(
                SearchResponse(
                    id=ids[i],
                    source=sources[i],
                    similarity=similarities[i],
                    ask_method_code=codes[i]
                )
            )
    return response_list

@cli_app.command()
def main(
    host: str = typer.Option("0.0.0.0", help="Host to run the FastAPI server on."),
    port: int = typer.Option(8001, help="Port to run the FastAPI server on."),
    model_path: Path = typer.Option(..., help="Path to the SentenceTransformer model. This is a required argument.", exists=True, file_okay=False, dir_okay=True, readable=True),
    lancedb_data_path: Path = typer.Option("data/lancedb_ask_data", help="Path to the LanceDB data directory.", exists=True, file_okay=False, dir_okay=True, writable=True)
):
    global lancedb_uri
    lancedb_uri = str(lancedb_data_path.resolve())
    # Store model_path on the cli_app object so lifespan can access it.
    # This is a common pattern when uvicorn.run hides direct arg passing to lifespan.
    cli_app.model_path_for_sentence_transformer = str(model_path.resolve())

    print(f"Starting FastAPI server on {host}:{port}")
    print(f"Using model from: {cli_app.model_path_for_sentence_transformer}")
    print(f"Using LanceDB data from: {lancedb_uri}")
    
    # Note: Uvicorn's reload features might not work perfectly with this Typer setup
    # if changes are made to the CLI part. For app logic, it should be fine.
    uvicorn.run(app, host=host, port=port)

if __name__ == "__main__":
    # This makes the script runnable, e.g., python search_api.py --model-path /path/to/model --lancedb-data-path /path/to/db
    cli_app()
