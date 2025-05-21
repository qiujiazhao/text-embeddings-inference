import pytest
from httpx import AsyncClient
from unittest.mock import patch, AsyncMock, MagicMock
import pyarrow as pa

# Import the FastAPI app from search_api.py
# We need to adjust the sys.path or use relative imports if structure allows
# For this subtask, assume search_api.py can be imported.
# If not, the worker might need to adjust sys.path or this instruction
# needs to be adapted based on the project's Python import conventions.
# One common way: add backends/python/server to PYTHONPATH or sys.path
import sys
from pathlib import Path
# Add the server directory to sys.path to allow direct import of search_api
# Corrected path: text_embeddings_server is at the same level as tests within server/
server_dir = Path(__file__).parent.parent / "text_embeddings_server"
sys.path.insert(0, str(server_dir.resolve()))

# Now that sys.path is adjusted, try importing
try:
    from search_api import app, SearchQuery, SearchResponse, lancedb_uri as global_lancedb_uri_module, model as global_model_module, cli_app
except ImportError as e:
    print(f"Failed to import search_api: {e}")
    print(f"sys.path: {sys.path}")
    # If running this test file directly, ensure PYTHONPATH is set or search_api.py is accessible.
    # This might indicate an issue with the sys.path adjustment or file location.
    raise

# Fixture to override lancedb_uri and model_path_for_sentence_transformer for testing
@pytest.fixture(autouse=True)
def override_config():
    # Access and modify the lancedb_uri from the imported search_api module
    # This requires search_api.lancedb_uri to be a mutable global or handled differently.
    # Assuming search_api.lancedb_uri is the one to modify.
    # To avoid NameError if the module variable isn't directly assignable,
    # we might need to patch it within the module if it's imported as `from search_api import lancedb_uri`
    # For now, let's assume we can modify it via the module itself, or it's handled by how it's set.
    
    original_lancedb_uri = global_lancedb_uri_module # Store original from module
    
    # Ensure cli_app.model_path_for_sentence_transformer exists or set a default test one
    original_model_path = None
    if hasattr(cli_app, 'model_path_for_sentence_transformer'):
        original_model_path = cli_app.model_path_for_sentence_transformer
    else:
        # If it's not set, we're setting it for the test, so no original to restore in that specific case
        pass

    # Modify for testing
    # How lancedb_uri is used in search_api.py matters. If it's read once at module load,
    # this fixture needs to run before search_api.py fully initializes its db connection logic,
    # or the variables it uses must be patchable.
    # The provided search_api.py sets lancedb_uri from a global which is updated by main().
    # For tests, we directly modify this global in the search_api module.
    import search_api
    search_api.lancedb_uri = "data/test_lancedb"
    cli_app.model_path_for_sentence_transformer = "/test/model" 
    
    yield # Test runs here
    
    # Restore original values
    search_api.lancedb_uri = original_lancedb_uri
    if original_model_path is not None:
         cli_app.model_path_for_sentence_transformer = original_model_path
    elif hasattr(cli_app, 'model_path_for_sentence_transformer'): # if it was set by test but not originally
        delattr(cli_app, 'model_path_for_sentence_transformer')


@pytest.fixture
async def client():
    # The lifespan function in search_api.py will run on app startup.
    # It uses cli_app.model_path_for_sentence_transformer and the global lancedb_uri.
    # The override_config fixture ensures these are set to test values before the app starts.
    async with AsyncClient(app=app, base_url="http://test") as ac:
        yield ac

# Mock SentenceTransformer
@patch('search_api.SentenceTransformer')
async def test_search_endpoint_success(mock_sentence_transformer_class, client: AsyncClient):
    # Configure the mock SentenceTransformer instance
    mock_model_instance = mock_sentence_transformer_class.return_value
    mock_model_instance.encode.return_value = [0.1, 0.2, 0.3] # Example embedding

    # Mock LanceDB connection and table operations
    mock_db_conn = AsyncMock()
    mock_table = AsyncMock()
    
    # Mock the data that would be returned from LanceDB to_arrow()
    mock_arrow_table = pa.Table.from_arrays([
        pa.array([101, 102], type=pa.int64()), # Explicitly set type
        pa.array(["sourceA", "sourceB"], type=pa.string()),
        pa.array([0.95, 0.89], type=pa.float64()),
        pa.array(["codeA", "codeB"], type=pa.string()),
    ], names=['expand_id', 'source_table', '_distance', 'ask_method_code'])
    
    mock_search_obj = AsyncMock() # This is what table.search returns
    # Configure the chain of calls: search().distance_type().limit().to_arrow()
    mock_search_obj.distance_type.return_value.limit.return_value.to_arrow = AsyncMock(return_value=mock_arrow_table)
    mock_table.search = AsyncMock(return_value=mock_search_obj) # table.search() returns the mock_search_obj
    
    # Patch lancedb.connect_async and the table open process
    with patch('search_api.lancedb.connect_async', return_value=mock_db_conn) as mock_connect:
        mock_db_conn.open_table = AsyncMock(return_value=mock_table) # db_conn.open_table() returns mock_table
        
        search_payload = {
            "question": "What is AI?",
            "industry": "tech",
            "top_k": 2
        }
        
        response = await client.post("/search", json=search_payload)
        
        assert response.status_code == 200
        results = response.json()
        
        assert len(results) == 2
        assert results[0]["id"] == 101
        assert results[0]["source"] == "sourceA"
        # In search_api.py, 'similarity' comes from '_distance'
        assert results[0]["similarity"] == 0.95 
        assert results[0]["ask_method_code"] == "codeA"
        
        assert results[1]["id"] == 102
        assert results[1]["source"] == "sourceB"
        assert results[1]["similarity"] == 0.89
        assert results[1]["ask_method_code"] == "codeB"

        # Verify SentenceTransformer was called
        # The model is loaded in lifespan, so this check might need adjustment
        # if the instance is created there and not per-call.
        # For this test, SentenceTransformer() is called in lifespan.
        mock_sentence_transformer_class.assert_called_once_with("/test/model")
        mock_model_instance.encode.assert_called_once_with("What is AI?")
        
        # Verify LanceDB calls
        mock_connect.assert_called_once_with("data/test_lancedb")
        mock_db_conn.open_table.assert_called_once_with("tech")
        # The argument to search is the embedding list itself
        mock_table.search.assert_called_once_with([0.1, 0.2, 0.3])
        mock_search_obj.distance_type.assert_called_once_with("cosine")
        mock_search_obj.distance_type.return_value.limit.assert_called_once_with(2)


@patch('search_api.SentenceTransformer')
async def test_search_endpoint_empty_results(mock_sentence_transformer_class, client: AsyncClient):
    mock_model_instance = mock_sentence_transformer_class.return_value
    mock_model_instance.encode.return_value = [0.4, 0.5, 0.6]

    mock_db_conn = AsyncMock()
    mock_table = AsyncMock()
    
    empty_arrow_table = pa.Table.from_arrays([
        pa.array([], type=pa.int64()),
        pa.array([], type=pa.string()),
        pa.array([], type=pa.float64()),
        pa.array([], type=pa.string()),
    ], names=['expand_id', 'source_table', '_distance', 'ask_method_code'])
    
    mock_search_obj = AsyncMock()
    mock_search_obj.distance_type.return_value.limit.return_value.to_arrow = AsyncMock(return_value=empty_arrow_table)
    mock_table.search = AsyncMock(return_value=mock_search_obj)

    with patch('search_api.lancedb.connect_async', return_value=mock_db_conn) as mock_connect:
        mock_db_conn.open_table = AsyncMock(return_value=mock_table)
        
        search_payload = {
            "question": "A rare question?",
            "industry": "pharma",
            "top_k": 5
        }
        
        response = await client.post("/search", json=search_payload)
        
        assert response.status_code == 200
        results = response.json()
        assert len(results) == 0

        mock_sentence_transformer_class.assert_called_once_with("/test/model")
        mock_model_instance.encode.assert_called_once_with("A rare question?")
        mock_connect.assert_called_once_with("data/test_lancedb")
        mock_db_conn.open_table.assert_called_once_with("pharma")
        mock_table.search.assert_called_once_with([0.4, 0.5, 0.6])

async def test_search_endpoint_validation_error(client: AsyncClient):
    # Missing 'question'
    search_payload = {
        "industry": "finance",
        "top_k": 3
    }
    response = await client.post("/search", json=search_payload)
    assert response.status_code == 422 # Unprocessable Entity for Pydantic validation errors

    # Invalid 'top_k' type
    search_payload_invalid_top_k = {
        "question": "Test",
        "industry": "finance",
        "top_k": "not-an-int"
    }
    response_invalid_top_k = await client.post("/search", json=search_payload_invalid_top_k)
    assert response_invalid_top_k.status_code == 422

# Add more tests as needed, e.g., for LanceDB connection failures,
# different model outputs, etc.
```
