mod common;

use crate::common::start_server_with_config;
use anyhow::Result;
use arrow_array::{
    ArrayRef, FixedSizeListArray, Float32Array, Int32Array, RecordBatch, RecordBatchIterator,
    StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use lancedb::connect;
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use std::sync::Arc;
use tempfile::tempdir;
use text_embeddings_backend::DType;

#[cfg(feature = "http")]
fn build_schema(dimension: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("body", DataType::Utf8, true),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dimension,
            ),
            false,
        ),
    ]))
}

#[cfg(feature = "http")]
fn build_batch(
    schema: &Arc<Schema>,
    ids: &[i32],
    bodies: &[&str],
    vectors: &[Vec<f32>],
) -> RecordBatch {
    assert_eq!(ids.len(), bodies.len());
    assert_eq!(ids.len(), vectors.len());

    let dimension = vectors
        .first()
        .map(|vector| vector.len())
        .unwrap_or_default();

    for vector in vectors {
        assert_eq!(vector.len(), dimension);
    }

    let id_array: ArrayRef = Arc::new(Int32Array::from(ids.to_vec()));
    let body_array: ArrayRef = Arc::new(StringArray::from(bodies.to_vec()));

    let flat: Vec<f32> = vectors.iter().flat_map(|v| v.iter().copied()).collect();
    let values: ArrayRef = Arc::new(Float32Array::from(flat));
    let list_array = FixedSizeListArray::try_new(values, dimension as i32).unwrap();
    let vector_array: ArrayRef = Arc::new(list_array);

    RecordBatch::try_new(schema.clone(), vec![id_array, body_array, vector_array]).unwrap()
}

#[tokio::test]
#[cfg(feature = "http")]
#[serial]
async fn vector_search_with_embedding_query() -> Result<()> {
    let tmpdir = tempdir()?;
    let uri = tmpdir.path().to_str().unwrap().to_string();
    let db = connect(&uri).execute().await?;

    let schema = build_schema(3);
    let batch = build_batch(
        &schema,
        &[1, 2],
        &["near", "far"],
        &[vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]],
    );
    let reader = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema.clone());
    db.create_table("items", Box::new(reader)).execute().await?;

    start_server_with_config(
        "BAAI/bge-large-en-v1.5".to_string(),
        None,
        DType::Float32,
        8092,
        Some(uri.clone()),
        Some("items".to_string()),
        Some("vector".to_string()),
        Some(vec!["id".to_string(), "body".to_string()]),
    )
    .await?;

    let client = reqwest::Client::new();
    let response = client
        .post("http://0.0.0.0:8092/vector_search")
        .json(&json!({
            "embedding": [0.0, 1.0, 0.0],
            "top_k": 2
        }))
        .send()
        .await?;

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await?;
    let results = body
        .get("results")
        .and_then(|value| value.as_array())
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["id"], json!(1));
    assert_eq!(results[0]["body"], json!("near"));

    Ok(())
}

#[tokio::test]
#[cfg(feature = "http")]
#[serial]
async fn vector_search_with_text_query() -> Result<()> {
    let embed_port = 8094;
    start_server_with_config(
        "BAAI/bge-large-en-v1.5".to_string(),
        None,
        DType::Float32,
        embed_port,
        None,
        None,
        None,
        None,
    )
    .await?;

    let embed_client = reqwest::Client::new();
    let embed_response = embed_client
        .post(&format!("http://0.0.0.0:{embed_port}/embed"))
        .json(&json!({ "inputs": "alpha" }))
        .send()
        .await?;

    let embedding: Vec<Vec<f32>> = embed_response.json().await?;
    let query_vector = embedding.into_iter().next().unwrap();
    let dimension = query_vector.len() as i32;

    let tmpdir = tempdir()?;
    let uri = tmpdir.path().to_str().unwrap().to_string();
    let db = connect(&uri).execute().await?;

    let schema = build_schema(dimension);
    let batch = build_batch(&schema, &[1], &["alpha"], &[query_vector.clone()]);
    let reader = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema.clone());
    db.create_table("items", Box::new(reader)).execute().await?;

    start_server_with_config(
        "BAAI/bge-large-en-v1.5".to_string(),
        None,
        DType::Float32,
        8095,
        Some(uri.clone()),
        Some("items".to_string()),
        Some("vector".to_string()),
        Some(vec!["id".to_string(), "body".to_string()]),
    )
    .await?;

    let client = reqwest::Client::new();
    let response = client
        .post("http://0.0.0.0:8095/vector_search")
        .json(&json!({
            "text": "alpha",
            "top_k": 1,
            "with_row_id": true
        }))
        .send()
        .await?;

    assert!(response.status().is_success());
    let headers = response.headers().clone();
    let tokens = headers
        .get("x-compute-tokens")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap();
    assert!(tokens > 0);

    let body: serde_json::Value = response.json().await?;
    let results = body
        .get("results")
        .and_then(|value| value.as_array())
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], json!(1));

    Ok(())
}

#[tokio::test]
#[cfg(feature = "http")]
#[serial]
async fn vector_search_requires_lancedb_configuration() -> Result<()> {
    let port = 8096;
    start_server_with_config(
        "BAAI/bge-large-en-v1.5".to_string(),
        None,
        DType::Float32,
        port,
        None,
        None,
        None,
        None,
    )
    .await?;

    let client = reqwest::Client::new();
    let response = client
        .post(&format!("http://0.0.0.0:{port}/vector_search"))
        .json(&json!({ "embedding": [0.0, 1.0, 0.0] }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FAILED_DEPENDENCY);
    let body: serde_json::Value = response.json().await?;
    assert_eq!(body["error"], json!("LanceDB is not configured"));

    Ok(())
}
