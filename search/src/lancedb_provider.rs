use crate::search_trait::{SearchService, SearchServiceError, SearchServiceRequest, SearchResponse};
use async_trait::async_trait;
use lancedb::{
    connect,
    schema::Schema as LanceSchema,
    table::AddDataOptions,
    Connection,
    Table
};
use arrow_schema::{DataType, Field, Schema as ArrowSchema};
use arrow_array::{Float32Array, RecordBatch, StringArray, Int64Array, types::Float32Type, FixedSizeListArray};
use std::sync::Arc;

const DEFAULT_VECTOR_FIELD_NAME: &str = "vector";
const DEFAULT_ID_FIELD_NAME: &str = "id";
const DEFAULT_SOURCE_FIELD_NAME: &str = "source";
const DEFAULT_ASK_METHOD_CODE_FIELD_NAME: &str = "ask_method_code";

pub struct LanceDbSearchProvider {
    conn: Connection,
    table_name: String,
    // We might need embedding_dim if we are creating the table or validating
    // embedding_dim: usize,
}

impl LanceDbSearchProvider {
    pub async fn new(
        db_uri: &str,
        table_name: &str,
        embedding_dim: usize,
    ) -> Result<Self, SearchServiceError> {
        let conn = connect(db_uri)
            .execute()
            .await
            .map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to connect to LanceDB: {}", e)))?;

        let table_names = conn
            .table_names()
            .await
            .map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to list tables: {}", e)))?;

        if !table_names.iter().any(|name| name == table_name) {
            // Table does not exist, create it
            let schema = Arc::new(ArrowSchema::new(vec![
                Field::new(DEFAULT_ID_FIELD_NAME, DataType::Int64, false),
                Field::new(
                    DEFAULT_VECTOR_FIELD_NAME,
                    DataType::FixedSizeList(
                        Arc::new(Field::new("item", DataType::Float32, true)),
                        embedding_dim as i32,
                    ),
                    true,
                ),
                Field::new(DEFAULT_SOURCE_FIELD_NAME, DataType::Utf8, true),
                Field::new(DEFAULT_ASK_METHOD_CODE_FIELD_NAME, DataType::Utf8, true),
            ]));

            // Create an empty RecordBatch to define the schema for lancedb
            let empty_batch = RecordBatch::new_empty(schema.clone());
            
            conn.create_table(table_name, Box::new(vec![empty_batch]))
                .await
                .map_err(|e| {
                    SearchServiceError::ExternalServiceError(format!("Failed to create table '{}': {}", table_name, e))
                })?;
            tracing::info!("Created LanceDB table '{}' with embedding dimension {}", table_name, embedding_dim);
            
            // Temporarily add some test data if the table was just created
            let temp_self_for_add = Self {
                conn: conn.clone(), // Clone connection for this temporary operation
                table_name: table_name.to_string(),
            };
            temp_self_for_add.add_data(
                1,
                vec![0.1; embedding_dim], // vector of 0.1s
                "Source A - Item 1",
                "ASK_CODE_001"
            ).await.map_err(|e| SearchServiceError::InternalError(format!("Failed to add test data 1: {}", e)))?;
            
            temp_self_for_add.add_data(
                2,
                vec![0.5; embedding_dim], // vector of 0.5s
                "Source B - Item 2",
                "ASK_CODE_002"
            ).await.map_err(|e| SearchServiceError::InternalError(format!("Failed to add test data 2: {}", e)))?;

            temp_self_for_add.add_data(
                3,
                vec![0.9; embedding_dim], // vector of 0.9s
                "Source C - Item 3",
                "ASK_CODE_003"
            ).await.map_err(|e| SearchServiceError::InternalError(format!("Failed to add test data 3: {}", e)))?;
            tracing::info!("Added temporary test data to table '{}'", table_name);

        } else {
            tracing::info!("Opened existing LanceDB table '{}'", table_name);
        }

        Ok(Self {
            conn,
            table_name: table_name.to_string(),
            // embedding_dim,
        })
    }

    // Placeholder for a method to add data
    pub async fn add_data(&self, id: i64, vector: Vec<f32>, source: &str, ask_method_code: &str) -> Result<(), SearchServiceError> {
        let table = self.conn.open_table(&self.table_name)
            .await
            .map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to open table '{}': {}", self.table_name, e)))?;

        let schema = table.schema().await.map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to get schema: {}",e)))?;
        let arrow_schema: Arc<ArrowSchema> = Arc::new(schema.try_into().map_err(|e: lancedb::error::Error| SearchServiceError::Unknown(format!("Could not convert LanceSchema to ArrowSchema: {}", e)))?);
        
        let ids = Int64Array::from(vec![id]);
        let vectors = Float32Array::from(vector);
        let sources = StringArray::from(vec![source]);
        let ask_method_codes = StringArray::from(vec![ask_method_code]);

        // We need to ensure the vector is correctly wrapped for FixedSizeList
        // This part is tricky and might need adjustment based on how lancedb expects FixedSizeList data.
        // For now, creating a RecordBatch with a simple Float32Array for the vector part might not directly work
        // if the table schema expects a FixedSizeList. LanceDB's `add_data` might handle this conversion, or we might need to structure it differently.
        // The `openai.rs` example uses `add_embedding` which handles this. Here we are trying to add raw vectors.

        // This is a simplified attempt, actual construction of RecordBatch for FixedSizeList needs care.
        // Let's assume for now that we can create a flat Float32Array and lancedb handles it, or we'll refine this.
        // A more robust way would be to construct the FixedSizeListArray directly.
        let batch = RecordBatch::try_new(
            arrow_schema.clone(), // Use the table's actual schema
            vec![
                Arc::new(ids),
                // Construct FixedSizeListArray for the vector
                Arc::new(FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                    vec![Some(vector)], // Each item in the outer vec is one list/vector
                    arrow_schema.field_with_name(DEFAULT_VECTOR_FIELD_NAME).unwrap().data_type().clone().try_into().unwrap() // Get the FixedSizeList DataType
                ).map_err(|e| SearchServiceError::InternalError(format!("Failed to create FixedSizeListArray for vector: {}", e)))?),
                Arc::new(sources),
                Arc::new(ask_method_codes),
            ],
        ).map_err(|e| SearchServiceError::InternalError(format!("Failed to create RecordBatch: {}", e)))?;

        table.add(Box::new(vec![batch]), None).await.map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to add data: {}", e)))?;
        Ok(())
    }
}

#[async_trait]
impl SearchService for LanceDbSearchProvider {
    async fn search(
        &self,
        request: SearchServiceRequest,
    ) -> Result<Vec<SearchResponse>, SearchServiceError> {
        let table = self.conn.open_table(&self.table_name)
            .await
            .map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to open table '{}': {}", self.table_name, e)))?;

        // Convert Vec<f32> to Float32Array for LanceDB
        let query_vector_data = Float32Array::from(request.question_embedding);
        
        let results = table
            .search(query_vector_data)
            .limit(request.top_k as usize)
            .execute_stream()
            .await
            .map_err(|e| SearchServiceError::ExternalServiceError(format!("Search failed: {}", e)))?;
        
        let record_batches: Vec<RecordBatch> = results
            .collect::<Vec<lancedb::error::Result<RecordBatch>>>()
            .await
            .into_iter()
            .map(|rb_result| rb_result.map_err(|e| SearchServiceError::ExternalServiceError(format!("Failed to collect search result batch: {}", e))))
            .collect::<Result<Vec<RecordBatch>, SearchServiceError>>()?;

        let mut search_responses = Vec::new();

        for batch in record_batches {
            let ids = batch
                .column_by_name(DEFAULT_ID_FIELD_NAME)
                .ok_or_else(|| SearchServiceError::InternalError(format!("Missing '{}' column in search result", DEFAULT_ID_FIELD_NAME)))?
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| SearchServiceError::InternalError(format!("Failed to downcast '{}' column", DEFAULT_ID_FIELD_NAME)))?;
            
            let sources = batch
                .column_by_name(DEFAULT_SOURCE_FIELD_NAME)
                .ok_or_else(|| SearchServiceError::InternalError(format!("Missing '{}' column in search result", DEFAULT_SOURCE_FIELD_NAME)))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| SearchServiceError::InternalError(format!("Failed to downcast '{}' column", DEFAULT_SOURCE_FIELD_NAME)))?;

            let ask_method_codes = batch
                .column_by_name(DEFAULT_ASK_METHOD_CODE_FIELD_NAME)
                .ok_or_else(|| SearchServiceError::InternalError(format!("Missing '{}' column in search result", DEFAULT_ASK_METHOD_CODE_FIELD_NAME)))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| SearchServiceError::InternalError(format!("Failed to downcast '{}' column", DEFAULT_ASK_METHOD_CODE_FIELD_NAME)))?;
            
            // LanceDB search results often include a '_distance' column for similarity/distance.
            // We need to extract this and map it to `similarity`.
            let distances = batch
                .column_by_name("_distance") // Default distance column name
                .ok_or_else(|| SearchServiceError::InternalError("Missing '_distance' column in search result".to_string()))?
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| SearchServiceError::InternalError("Failed to downcast '_distance' column".to_string()))?;

            for i in 0..ids.len() {
                // LanceDB distances are often L2, smaller is better. Similarity is often 0-1, larger is better.
                // A simple conversion could be 1.0 / (1.0 + distance), or 1.0 - distance if distance is normalized.
                // This needs to be adjusted based on the actual distance metric used by LanceDB and desired similarity range.
                // For now, let's assume a simple inverse relationship for demonstration.
                let similarity_score = 1.0 / (1.0 + distances.value(i)); 

                search_responses.push(SearchResponse {
                    id: ids.value(i),
                    source: sources.value(i).to_string(),
                    similarity: similarity_score,
                    ask_method_code: ask_method_codes.value(i).to_string(),
                });
            }
        }
        
        // Ensure we don't return more than top_k results, though .limit() should handle this.
        search_responses.truncate(request.top_k as usize);

        Ok(search_responses)
    }
}
