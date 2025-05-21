import sys
import typer

from pathlib import Path
from loguru import logger
from typing import Optional
from enum import Enum

app = typer.Typer()


class Dtype(str, Enum):
    float32 = "float32"
    float16 = "float16"
    bloat16 = "bfloat16"


@app.command()
def serve(
    model_path: Path,
    dtype: Dtype = "float32",
    uds_path: Path = "/tmp/text-embeddings-server",
    logger_level: str = "INFO",
    json_output: bool = False,
    otlp_endpoint: Optional[str] = None,
    otlp_service_name: str = "text-embeddings-inference.server",
    pool: str = "cls",
):
    # Remove default handler
    logger.remove()
    logger.add(
        sys.stdout,
        format="{message}",
        filter="text_embeddings_server",
        level=logger_level,
        serialize=json_output,
        backtrace=True,
        diagnose=False,
    )

    # Import here after the logger is added to log potential import exceptions
    from text_embeddings_server import server
    from text_embeddings_server.utils.tracing import setup_tracing

    # Setup OpenTelemetry distributed tracing
    if otlp_endpoint is not None:
        setup_tracing(otlp_endpoint=otlp_endpoint, otlp_service_name=otlp_service_name)

    # Downgrade enum into str for easier management later on
    dtype = None if dtype is None else dtype.value
    server.serve(model_path, dtype, uds_path, pool)


# New command to serve the search API
import subprocess
# sys is already imported

@app.command()
def serve_search_api(
    model_path: Path = typer.Option(..., help="Path to the SentenceTransformer model for search.", rich_help_panel="Search API Options", exists=True, file_okay=False, dir_okay=True, readable=True),
    lancedb_data_path: Path = typer.Option("data/lancedb_ask_data_search", help="Path to the LanceDB data directory for search.", rich_help_panel="Search API Options", exists=True, file_okay=False, dir_okay=True, writable=True),
    search_api_port: int = typer.Option(8001, help="Port for the search API FastAPI server.", rich_help_panel="Search API Options"),
    host: str = typer.Option("0.0.0.0", help="Host for the search API server.", rich_help_panel="Search API Options"),
    logger_level: str = typer.Option("INFO", help="Logging level.", rich_help_panel="Search API Options"),
    json_output: bool = typer.Option(False, help="Output logs in JSON format.", rich_help_panel="Search API Options"),
    otlp_endpoint: Optional[str] = typer.Option(None, help="OTLP endpoint for telemetry.", rich_help_panel="Search API Options"),
    otlp_service_name: str = typer.Option("text-embeddings-search-api", help="Service name for OTLP.", rich_help_panel="Search API Options"),
):
    """
    Launches the FastAPI server for the search API.
    """
    # Create a dedicated logger for the search API, or reconfigure the global one.
    # For simplicity, let's use a logger that filters by a specific name.
    # Note: loguru's handler removal and addition can be tricky if not managed carefully.
    # A more robust approach might involve passing a logger instance or using contexts.
    
    # Attempt to remove any existing default handler to avoid duplicate logs if this command is run multiple times
    # or if the global logger was already configured. This is a best-effort.
    try:
        logger.remove(0) 
    except ValueError: # No handler with id 0 (default)
        pass

    logger.add(
        sys.stdout,
        format="{time} {level} {message} [service:search_api]", # Add a tag for clarity
        filter=lambda record: record["extra"].get("name") == "search_api_logger_instance", # Custom filter
        level=logger_level,
        serialize=json_output,
        backtrace=True,
        diagnose=False,
    )
    search_api_logger = logger.bind(name="search_api_logger_instance") # Bind to make 'name' available in extra

    search_api_logger.info(f"Attempting to start Search API FastAPI server on {host}:{search_api_port}")
    search_api_logger.info(f"  Model Path: {model_path.resolve()}")
    search_api_logger.info(f"  LanceDB Data Path: {lancedb_data_path.resolve()}")
    search_api_logger.info(f"  Logging Level: {logger_level}")
    search_api_logger.info(f"  JSON Output: {json_output}")
    search_api_logger.info(f"  OTLP Endpoint: {otlp_endpoint}")
    search_api_logger.info(f"  OTLP Service Name: {otlp_service_name}")

    if otlp_endpoint:
        try:
            from text_embeddings_server.utils.tracing import setup_tracing as setup_search_api_tracing
            setup_search_api_tracing(otlp_endpoint=otlp_endpoint, otlp_service_name=otlp_service_name)
            search_api_logger.info("Search API OTLP tracing configured.")
        except ImportError:
            search_api_logger.warning("Could not import 'setup_tracing'. OTLP will not be configured for search API.")
        except Exception as e:
            search_api_logger.error(f"Failed to setup OTLP tracing for search API: {e}")


    # Construct the command to run search_api.py's 'main' Typer command
    # Path(__file__).parent gives the directory of the current cli.py script
    search_api_script_path = Path(__file__).parent / "search_api.py"
    
    cmd = [
        sys.executable, # Path to current python interpreter
        str(search_api_script_path.resolve()),
        "main", # The Typer command defined in search_api.py
        "--host", host,
        "--port", str(search_api_port),
        "--model-path", str(model_path.resolve()),
        "--lancedb-data-path", str(lancedb_data_path.resolve()),
        # Note: search_api.py does not currently handle OTLP args directly via its CLI.
        # If it needs to, its 'main' Typer command would need to accept them.
        # For now, OTLP setup is done in this cli.py launcher context if enabled.
    ]
    
    search_api_logger.info(f"Executing command: {' '.join(cmd)}")

    try:
        # Using Popen to run in the background.
        # For debugging, you might want to capture stdout/stderr:
        # process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        # stdout, stderr = process.communicate()
        # search_api_logger.info(f"Search API stdout: {stdout.decode()}")
        # if stderr:
        #    search_api_logger.error(f"Search API stderr: {stderr.decode()}")
        process = subprocess.Popen(cmd)
        search_api_logger.info(f"Search API server process started with PID: {process.pid}. This process runs in the background.")
        # If this CLI command should wait for the server to finish (e.g., for direct execution and termination):
        # process.wait()
        # search_api_logger.info("Search API server process terminated.")
    except FileNotFoundError:
        search_api_logger.error(f"Failed to start Search API server: The script {search_api_script_path} was not found. Searched at resolved path: {search_api_script_path.resolve()}")
        raise
    except Exception as e:
        search_api_logger.error(f"Failed to start Search API server: {e}")
        raise

if __name__ == "__main__":
    app()
