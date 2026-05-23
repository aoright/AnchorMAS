use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, ScalarQuantizationBuilder,
    SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
    Value as QdrantValue,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use uuid::Uuid;

use crate::agent::{AnalyzedEvent, RawDocument};
use crate::config::Config;

const EMBEDDING_DIM: u64 = 1024;

/// Initialize Qdrant client and ensure the collection exists.
pub async fn init_qdrant(config: &Config) -> Result<Qdrant> {
    let client = Qdrant::from_url(&config.qdrant_url)
        .build()
        .context("Failed to connect to Qdrant")?;

    let collections = client
        .list_collections()
        .await
        .context("Failed to list Qdrant collections")?;

    let exists = collections
        .collections
        .iter()
        .any(|c| c.name == config.qdrant_collection);

    if !exists {
        tracing::info!(
            collection = %config.qdrant_collection,
            "Creating Qdrant collection"
        );
        client
            .create_collection(
                CreateCollectionBuilder::new(&config.qdrant_collection)
                    .vectors_config(VectorParamsBuilder::new(EMBEDDING_DIM, Distance::Cosine))
                    .quantization_config(ScalarQuantizationBuilder::default()),
            )
            .await
            .context("Failed to create Qdrant collection")?;
    }

    tracing::info!(
        collection = %config.qdrant_collection,
        "Qdrant initialized"
    );
    Ok(client)
}

/// Generate a deterministic pseudo-embedding from text hash.
/// In production, replace with a real embedding model API call.
pub fn pseudo_embedding(text: &str) -> Vec<f32> {
    let mut embedding = vec![0.0f32; EMBEDDING_DIM as usize];
    let bytes = text.as_bytes();
    for (i, &byte) in bytes.iter().enumerate() {
        let idx = i % (EMBEDDING_DIM as usize);
        embedding[idx] += (byte as f32 - 128.0) / 128.0;
    }
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in embedding.iter_mut() {
            *v /= norm;
        }
    }
    embedding
}

#[derive(serde::Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(serde::Deserialize)]
struct EmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

async fn get_embeddings_internal(config: &Config, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut results = vec![None; texts.len()];

    // Chunk in batches of 10 to stay within DashScope request limits safely
    for (batch_idx, chunk) in texts.chunks(10).enumerate() {
        let payload = EmbeddingRequest {
            model: config.embedding_model.clone(),
            input: chunk.to_vec(),
        };

        let response = client
            .post(&config.embedding_api_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", config.ark_api_key))
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            anyhow::bail!("DashScope Embedding API error ({}): {}", status, err_body);
        }

        let resp: EmbeddingResponse = response.json().await?;
        for data in resp.data {
            let absolute_idx = batch_idx * 10 + data.index;
            if absolute_idx < results.len() {
                results[absolute_idx] = Some(data.embedding);
            }
        }
    }

    let mut embeddings = Vec::new();
    for (i, opt) in results.into_iter().enumerate() {
        if let Some(emb) = opt {
            embeddings.push(emb);
        } else {
            tracing::warn!("Embedding at index {} was missing in response, using pseudo-embedding", i);
            embeddings.push(pseudo_embedding(&texts[i]));
        }
    }

    Ok(embeddings)
}

/// Fetch embeddings for a slice of texts with a graceful fallback to pseudo-embeddings.
pub async fn get_embeddings(config: &Config, texts: &[String]) -> Vec<Vec<f32>> {
    match get_embeddings_internal(config, texts).await {
        Ok(embs) => embs,
        Err(e) => {
            tracing::error!("Failed to fetch real embeddings from DashScope: {}. Falling back to pseudo-embeddings.", e);
            texts.iter().map(|t| pseudo_embedding(t)).collect()
        }
    }
}

fn str_val(s: &str) -> QdrantValue {
    QdrantValue::from(s.to_string())
}

fn int_val(i: i32) -> QdrantValue {
    QdrantValue::from(i as i64)
}

/// Store raw documents in Qdrant with their embeddings.
pub async fn store_documents(
    client: &Qdrant,
    collection: &str,
    docs: &[RawDocument],
    config: &Config,
) -> Result<()> {
    if docs.is_empty() {
        return Ok(());
    }

    let texts: Vec<String> = docs
        .iter()
        .map(|doc| format!("{} {}", doc.title, doc.content))
        .collect();

    let embeddings = get_embeddings(config, &texts).await;

    let points: Vec<PointStruct> = docs
        .iter()
        .zip(embeddings)
        .map(|(doc, embedding)| {
            let id = Uuid::new_v4().to_string();

            let mut payload: HashMap<String, QdrantValue> = HashMap::new();
            payload.insert("title".to_string(), str_val(&doc.title));
            payload.insert("content".to_string(), str_val(&doc.content));
            payload.insert("source_url".to_string(), str_val(&doc.source_url));
            payload.insert("raw_language".to_string(), str_val(&doc.raw_language));
            payload.insert("timestamp".to_string(), str_val(&doc.timestamp));
            payload.insert("doc_type".to_string(), str_val("raw_article"));

            PointStruct::new(id, embedding, payload)
        })
        .collect();

    client
        .upsert_points(UpsertPointsBuilder::new(collection, points).wait(true))
        .await
        .context("Failed to upsert documents to Qdrant")?;

    tracing::info!(count = docs.len(), "Documents stored in Qdrant");
    Ok(())
}

/// Store analyzed events in Qdrant for semantic search.
pub async fn store_events(
    client: &Qdrant,
    collection: &str,
    events: &[AnalyzedEvent],
    briefing_id: &str,
    config: &Config,
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    let texts: Vec<String> = events
        .iter()
        .map(|event| format!("{} {} {}", event.title, event.summary, event.analysis))
        .collect();

    let embeddings = get_embeddings(config, &texts).await;

    let points: Vec<PointStruct> = events
        .iter()
        .zip(embeddings)
        .map(|(event, embedding)| {
            let mut payload: HashMap<String, QdrantValue> = HashMap::new();
            payload.insert("title".to_string(), str_val(&event.title));
            payload.insert("summary".to_string(), str_val(&event.summary));
            payload.insert("analysis".to_string(), str_val(&event.analysis));
            payload.insert("market".to_string(), str_val(&event.market));
            payload.insert("category".to_string(), str_val(&event.category));
            payload.insert("impact_type".to_string(), str_val(&event.impact_type));
            payload.insert("severity".to_string(), int_val(event.severity));
            payload.insert("urgency".to_string(), int_val(event.urgency));
            payload.insert("confidence".to_string(), int_val(event.confidence));
            payload.insert("briefing_id".to_string(), str_val(briefing_id));
            payload.insert("doc_type".to_string(), str_val("analyzed_event"));

            PointStruct::new(event.id.clone(), embedding, payload)
        })
        .collect();

    client
        .upsert_points(UpsertPointsBuilder::new(collection, points).wait(true))
        .await
        .context("Failed to upsert events to Qdrant")?;

    tracing::info!(count = events.len(), "Events stored in Qdrant");
    Ok(())
}

/// Search for semantically similar documents in Qdrant.
pub async fn search_similar(
    client: &Qdrant,
    collection: &str,
    query_text: &str,
    limit: u64,
    doc_type: Option<String>,
    market: Option<String>,
    category: Option<String>,
    config: &Config,
) -> Result<Vec<serde_json::Value>> {
    let query_embedding = get_embeddings(config, &[query_text.to_string()])
        .await
        .pop()
        .unwrap_or_else(|| pseudo_embedding(query_text));

    let mut must_conditions = Vec::new();
    if let Some(dt) = doc_type {
        must_conditions.push(qdrant_client::qdrant::Condition::matches("doc_type", dt));
    }
    if let Some(m) = market {
        must_conditions.push(qdrant_client::qdrant::Condition::matches("market", m));
    }
    if let Some(c) = category {
        must_conditions.push(qdrant_client::qdrant::Condition::matches("category", c));
    }

    let mut search_builder = SearchPointsBuilder::new(collection, query_embedding, limit)
        .with_payload(true);

    if !must_conditions.is_empty() {
        let filter = qdrant_client::qdrant::Filter {
            must: must_conditions,
            ..Default::default()
        };
        search_builder = search_builder.filter(filter);
    }

    let results = client
        .search_points(search_builder)
        .await
        .context("Failed to search Qdrant")?;


    let items: Vec<serde_json::Value> = results
        .result
        .into_iter()
        .map(|point| {
            let mut obj = serde_json::Map::new();
            obj.insert("score".to_string(), serde_json::json!(point.score));
            obj.insert(
                "id".to_string(),
                serde_json::json!(point.id.map(|id| format!("{:?}", id)).unwrap_or_default()),
            );
            for (key, val) in point.payload {
                obj.insert(key, payload_value_to_json(&val));
            }
            serde_json::Value::Object(obj)
        })
        .collect();

    Ok(items)
}

fn payload_value_to_json(val: &QdrantValue) -> serde_json::Value {
    use qdrant_client::qdrant::value::Kind;
    match &val.kind {
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::IntegerValue(i)) => serde_json::json!(i),
        Some(Kind::DoubleValue(d)) => serde_json::json!(d),
        Some(Kind::BoolValue(b)) => serde_json::json!(b),
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::ListValue(list)) => {
            let items: Vec<serde_json::Value> =
                list.values.iter().map(payload_value_to_json).collect();
            serde_json::json!(items)
        }
        Some(Kind::StructValue(s)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &s.fields {
                map.insert(k.clone(), payload_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}
