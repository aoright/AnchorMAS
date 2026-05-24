use axum::{
    extract::{Query, State, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::AppState;
use crate::agent::{self, DoubaoClient};
use crate::vectordb;

// ---- Response Types --------------------------------------------------------

#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct RawArticleResponse {
    id: String,
    source_url: String,
    title: String,
    content: String,
    raw_language: String,
    market: String,
    created_at: String,
    pipeline_status: Option<String>,
}

#[derive(Serialize)]
struct BriefingListItem {
    id: String,
    date: String,
    overview: String,
    event_count: i64,
    created_at: String,
}

#[derive(Serialize)]
struct BriefingResponse {
    id: String,
    date: String,
    overview: String,
    heatmap: serde_json::Value,
    recommendations: serde_json::Value,
    events: Vec<EventResponse>,
    created_at: String,
}

#[derive(Serialize, Clone)]
struct RawSourceResponse {
    title: String,
    source_url: String,
    content: String,
}

#[derive(Serialize)]
struct EventResponse {
    id: String,
    market: String,
    category: String,
    title: String,
    summary: String,
    impact_type: String,
    severity: i64,
    urgency: i64,
    confidence: i64,
    source_urls: serde_json::Value,
    analysis: String,
    #[serde(default)]
    raw_sources: Vec<RawSourceResponse>,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    message: String,
    briefing_id: String,
}


#[derive(Serialize)]
struct ScanResponse {
    status: String,
    message: String,
}

#[derive(Deserialize, Default)]
pub struct ScanQuery {
    force: Option<bool>,
    synthesize_only: Option<bool>,
    hours: Option<u32>,
}

struct CachedBriefingSummary {
    created_at: String,
    raw_count: usize,
    event_count: usize,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    query: String,
    limit: Option<u64>,
    doc_type: Option<String>,
    market: Option<String>,
    category: Option<String>,
}

// ---- Handlers --------------------------------------------------------------

/// GET /api/raw-articles
/// Returns all raw scraped articles from SQLite, ordered by creation time.
pub async fn get_raw_articles(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, RawArticleRow>(
        r#"SELECT id, COALESCE(resolved_url, source_url) AS source_url, title, content, raw_language, market, created_at, pipeline_status
           FROM raw_articles
           ORDER BY created_at DESC
           LIMIT 500"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query raw articles");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    let articles: Vec<RawArticleResponse> = rows
        .into_iter()
        .map(|row| RawArticleResponse {
            id: row.id,
            source_url: row.source_url,
            title: row.title,
            content: row.content,
            raw_language: row.raw_language,
            market: row.market,
            created_at: row.created_at,
            pipeline_status: row.pipeline_status,
        })
        .collect();

    Ok(Json(articles))
}

/// GET /api/pipeline/status
/// Returns the current pipeline execution status.
pub async fn get_pipeline_status(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let status = state.pipeline_status.read().await;
    Json(status.clone())
}

/// GET /api/briefing/latest
pub async fn get_latest_briefing(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let briefing = sqlx::query_as::<_, BriefingRow>(
        r#"SELECT id, date, overview, heatmap_json, recommendations_json, created_at
           FROM briefings
           ORDER BY created_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query latest briefing");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    let briefing = briefing.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No briefings found".to_string(),
            }),
        )
    })?;

    Ok(Json(build_briefing_response(&state, briefing).await?))
}

/// GET /api/briefings/:id
pub async fn get_briefing_by_id(
    State(state): State<AppState>,
    Path(briefing_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let briefing = sqlx::query_as::<_, BriefingRow>(
        r#"SELECT id, date, overview, heatmap_json, recommendations_json, created_at
           FROM briefings
           WHERE id = ?"#,
    )
    .bind(&briefing_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, briefing_id = %briefing_id, "Failed to query briefing");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    let briefing = briefing.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Briefing not found".to_string(),
            }),
        )
    })?;

    Ok(Json(build_briefing_response(&state, briefing).await?))
}

async fn build_briefing_response(
    state: &AppState,
    briefing: BriefingRow,
) -> Result<BriefingResponse, (StatusCode, Json<ErrorResponse>)> {
    let event_rows = sqlx::query_as::<_, EventRow>(
        r#"SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, analysis
           FROM events e
           JOIN briefing_events be ON be.event_id = e.id
           WHERE be.briefing_id = ?
           ORDER BY be.position ASC, e.severity DESC, e.urgency DESC"#,
    )
    .bind(&briefing.id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query events");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    // Extract unique source URLs from all event rows to fetch raw articles
    let mut unique_urls = std::collections::HashSet::new();
    for row in &event_rows {
        if let Ok(urls) = serde_json::from_str::<Vec<String>>(&row.source_urls) {
            for url in urls {
                unique_urls.insert(url);
            }
        }
    }

    let mut raw_articles_map = std::collections::HashMap::new();
    if !unique_urls.is_empty() {
        let urls_vec: Vec<String> = unique_urls.into_iter().collect();
        for chunk in urls_vec.chunks(500) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "SELECT id, COALESCE(resolved_url, source_url) AS source_url, title, content, raw_language FROM raw_articles WHERE COALESCE(resolved_url, source_url) IN ("
            );
            let mut separated = query_builder.separated(", ");
            for url in chunk {
                separated.push_bind(url);
            }
            separated.push_unseparated(")");

            let query = query_builder.build_query_as::<(String, String, String, String, String)>();
            if let Ok(rows) = query.fetch_all(&state.pool).await {
                for (_id, source_url, title, content, _raw_language) in rows {
                    raw_articles_map.insert(
                        source_url.clone(),
                        RawSourceResponse {
                            title,
                            source_url,
                            content,
                        },
                    );
                }
            }
        }
    }

    let events: Vec<EventResponse> = event_rows
        .into_iter()
        .map(|row| {
            let urls_val = serde_json::from_str(&row.source_urls).unwrap_or(serde_json::json!([]));
            let mut raw_sources = Vec::new();
            if let Some(urls_arr) = urls_val.as_array() {
                for url_val in urls_arr {
                    if let Some(url_str) = url_val.as_str() {
                        if let Some(raw_src) = raw_articles_map.get(url_str) {
                            raw_sources.push(raw_src.clone());
                        }
                    }
                }
            }

            EventResponse {
                id: row.id,
                market: row.market,
                category: row.category,
                title: row.title,
                summary: row.summary,
                impact_type: row.impact_type,
                severity: row.severity,
                urgency: row.urgency,
                confidence: row.confidence,
                source_urls: urls_val,
                analysis: row.analysis,
                raw_sources,
            }
        })
        .collect();

    let heatmap: serde_json::Value =
        serde_json::from_str(&briefing.heatmap_json).unwrap_or(serde_json::json!({}));
    let recommendations: serde_json::Value =
        serde_json::from_str(&briefing.recommendations_json).unwrap_or(serde_json::json!([]));

    let response = BriefingResponse {
        id: briefing.id,
        date: briefing.date,
        overview: briefing.overview,
        heatmap,
        recommendations,
        events,
        created_at: briefing.created_at,
    };

    Ok(response)
}

/// GET /api/briefings
pub async fn list_briefings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, BriefingListRow>(
        r#"SELECT b.id, b.date, b.overview, b.created_at, COUNT(be.event_id) AS event_count
           FROM briefings b
           LEFT JOIN briefing_events be ON be.briefing_id = b.id
           GROUP BY b.id, b.date, b.overview, b.created_at
           ORDER BY b.created_at DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query briefings");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    let items: Vec<BriefingListItem> = rows
        .into_iter()
        .map(|row| BriefingListItem {
            id: row.id,
            date: row.date,
            overview: row.overview,
            event_count: row.event_count,
            created_at: row.created_at,
        })
        .collect();

    Ok(Json(items))
}

/// POST /api/scan
/// Triggers the intelligence pipeline as a background task.
/// Updates PipelineStatus in real-time.
pub async fn trigger_scan(
    State(state): State<AppState>,
    Query(query): Query<ScanQuery>,
) -> impl IntoResponse {
    // Atomically check-and-set to prevent TOCTOU race condition
    {
        let mut status = state.pipeline_status.write().await;
        if status.status == "running" {
            return (
                StatusCode::CONFLICT,
                Json(ScanResponse {
                    status: "conflict".to_string(),
                    message: "Pipeline is already running".to_string(),
                }),
            );
        }

        if !query.force.unwrap_or(false) && !query.synthesize_only.unwrap_or(false) && query.hours.is_none() {
            // Check for cached briefing (non-force, non-synthesize-only mode)
            match get_latest_briefing_summary(&state.pool).await {
                Ok(Some(summary)) => {
                    status.status = "completed".to_string();
                    status.current_step = None;
                    status.last_run = Some(summary.created_at.clone());
                    status.error_message = None;
                    status.stats = super::PipelineStats {
                        raw_count: summary.raw_count,
                        filtered_count: summary.event_count,
                        analyzed_count: summary.event_count,
                        verified_count: summary.event_count,
                    };
                    status.progress = super::PipelineProgressDetails {
                        message: Some(
                            "Loaded cached briefing from database. Use force=true to rescan."
                                .to_string(),
                        ),
                        processed_count: summary.event_count,
                        total_count: summary.event_count,
                        output_count: summary.event_count,
                        updated_at: Some(summary.created_at),
                        ..super::PipelineProgressDetails::default()
                    };

                    return (
                        StatusCode::OK,
                        Json(ScanResponse {
                            status: "cached".to_string(),
                            message: "Loaded cached briefing from database".to_string(),
                        }),
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Failed to inspect cached briefing");
                }
            }

            match get_raw_article_count(&state.pool).await {
                Ok(raw_count) if raw_count > 0 => {
                    status.status = "idle".to_string();
                    status.current_step = None;
                    status.error_message = None;
                    status.stats = super::PipelineStats {
                        raw_count,
                        ..super::PipelineStats::default()
                    };
                    status.progress = super::PipelineProgressDetails {
                        message: Some(
                            "Loaded cached raw articles from database. Use Force Rescan to run the pipeline."
                                .to_string(),
                        ),
                        output_count: raw_count,
                        updated_at: Some(chrono::Utc::now().to_rfc3339()),
                        ..super::PipelineProgressDetails::default()
                    };

                    return (
                        StatusCode::OK,
                        Json(ScanResponse {
                            status: "cached_raw".to_string(),
                            message: "Loaded cached raw articles from database".to_string(),
                        }),
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Failed to inspect cached raw articles");
                }
            }
        }

        // Set status to running while still holding the write lock
        status.status = "running".to_string();
        status.current_step = Some("harvester".to_string());
        status.error_message = None;
        status.stats = super::PipelineStats::default();
        status.progress = super::PipelineProgressDetails {
            message: Some("Starting harvest".to_string()),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            ..super::PipelineProgressDetails::default()
        };
    }

    let config = state.config.clone();
    let pool = state.pool.clone();
    let qdrant = state.qdrant.clone();
    let pipeline_status = state.pipeline_status.clone();

    let force = query.force.unwrap_or(false);
    let synthesize_only = query.synthesize_only.unwrap_or(false);
    let hours = query.hours;

    tokio::spawn(async move {
        tracing::info!("Background scan pipeline started");

        let progress_status = pipeline_status.clone();
        let report_progress = move |progress: agent::PipelineProgress| {
            let progress_status = progress_status.clone();
            async move {
                let mut status = progress_status.write().await;
                if let Some(step) = progress.current_step {
                    let step = step.as_str();
                    if status.current_step.as_deref() != Some(step) {
                        status.progress = super::PipelineProgressDetails::default();
                    }
                    status.current_step = Some(step.to_string());
                }
                if let Some(raw_count) = progress.raw_count {
                    status.stats.raw_count = raw_count;
                }
                if let Some(filtered_count) = progress.filtered_count {
                    status.stats.filtered_count = filtered_count;
                }
                if let Some(analyzed_count) = progress.analyzed_count {
                    status.stats.analyzed_count = analyzed_count;
                }
                if let Some(verified_count) = progress.verified_count {
                    status.stats.verified_count = verified_count;
                }
                if let Some(message) = progress.message {
                    status.progress.message = Some(message);
                }
                if let Some(processed_count) = progress.processed_count {
                    status.progress.processed_count = processed_count;
                }
                if let Some(total_count) = progress.total_count {
                    status.progress.total_count = total_count;
                }
                if let Some(output_count) = progress.output_count {
                    status.progress.output_count = output_count;
                }
                if let Some(batch_index) = progress.batch_index {
                    status.progress.batch_index = Some(batch_index);
                }
                if let Some(batch_total) = progress.batch_total {
                    status.progress.batch_total = Some(batch_total);
                }
                if let Some(completed_batches) = progress.completed_batches {
                    status.progress.completed_batches = completed_batches;
                }
                if let Some(failed_batches) = progress.failed_batches {
                    status.progress.failed_batches = failed_batches;
                }
                if let Some(last_error) = progress.last_error {
                    status.progress.last_error = Some(last_error);
                }
                status.progress.updated_at = Some(chrono::Utc::now().to_rfc3339());
            }
        };

        match agent::run_pipeline_with_progress(&config, &pool, qdrant.as_deref(), force, synthesize_only, hours, report_progress).await {
            Ok(briefing) => {
                let mut status = pipeline_status.write().await;
                status.status = "completed".to_string();
                status.current_step = None;
                status.last_run = Some(chrono::Utc::now().to_rfc3339());
                status.stats.analyzed_count = briefing.events.len();
                status.stats.verified_count = briefing.events.len();
                status.progress.message = Some("Pipeline completed".to_string());
                status.progress.updated_at = status.last_run.clone();

                tracing::info!(
                    briefing_id = %briefing.id,
                    events = briefing.events.len(),
                    "Background scan pipeline completed"
                );
            }
            Err(e) => {
                let mut status = pipeline_status.write().await;
                status.status = "error".to_string();
                status.error_message = Some(format!("{}", e));
                status.last_run = Some(chrono::Utc::now().to_rfc3339());
                status.progress.message = Some("Pipeline failed".to_string());
                status.progress.last_error = status.error_message.clone();
                status.progress.updated_at = status.last_run.clone();

                tracing::error!(error = %e, "Background scan pipeline failed");
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(ScanResponse {
            status: "accepted".to_string(),
            message: "Scan pipeline started in background".to_string(),
        }),
    )
}

/// POST /api/chat
pub async fn chat(
    State(state): State<AppState>,
    Json(payload): Json<ChatRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let event_rows = sqlx::query_as::<_, EventRow>(
        r#"SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, analysis
           FROM events e
           JOIN briefing_events be ON be.event_id = e.id
           WHERE be.briefing_id = ?
           ORDER BY be.position ASC, e.severity DESC, e.urgency DESC"#,
    )
    .bind(&payload.briefing_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query events for chat");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?;

    let briefing = sqlx::query_as::<_, BriefingRow>(
        "SELECT id, date, overview, heatmap_json, recommendations_json, created_at FROM briefings WHERE id = ?"
    )
    .bind(&payload.briefing_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query briefing for chat");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Database error".to_string(),
            }),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Briefing not found".to_string(),
            }),
        )
    })?;

    let history_rows = sqlx::query_as::<_, ChatHistoryRow>(
        "SELECT user_message, ai_response FROM chat_history WHERE briefing_id = ? ORDER BY created_at ASC LIMIT 10"
    )
    .bind(&payload.briefing_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut history_context = String::new();
    for row in history_rows {
        history_context.push_str(&format!("User: {}\nAssistant: {}\n", row.user_message, row.ai_response));
    }

    let mut events_summary = Vec::new();
    for event in event_rows {
        events_summary.push(format!(
            "- [{}] {} (Category: {}, Severity: {}, Urgency: {}): {}",
            event.market, event.title, event.category, event.severity, event.urgency, event.summary
        ));
    }

    // RAG: Query Qdrant for matching historical events to add context
    let mut rag_context = String::new();
    if let Some(qdrant) = &state.qdrant {
        let collection = &state.config.qdrant_collection;
        if let Ok(search_results) = vectordb::search_similar(
            qdrant,
            collection,
            &payload.message,
            5,
            Some("analyzed_event".to_string()),
            None,
            None,
            &state.config,
        )
        .await
        {
            let mut matched_events = Vec::new();
            for item in search_results {
                if let (Some(title), Some(summary)) = (item.get("title").and_then(|v| v.as_str()), item.get("summary").and_then(|v| v.as_str())) {
                    let market = item.get("market").and_then(|v| v.as_str()).unwrap_or("Global");
                    let category = item.get("category").and_then(|v| v.as_str()).unwrap_or("General");
                    let analysis = item.get("analysis").and_then(|v| v.as_str()).unwrap_or("");
                    matched_events.push(format!(
                        "- [{}] {} (Category: {}): {}\n  Analysis: {}",
                        market, title, category, summary, analysis
                    ));
                }
            }
            if !matched_events.is_empty() {
                rag_context.push_str("\nRelevant Historical Events (RAG Context):\n");
                rag_context.push_str(&matched_events.join("\n"));
                rag_context.push_str("\n");
            }
        }
    }

    let system_prompt = format!(
        r#"你是一个高级珠宝行业战略咨询专家。请结合今日简报内容、检索到的相关历史背景和用户的对话历史，回答用户的问题。
今日简报核心信息如下：

Briefing ({date}):
Overview: {overview}
Heatmap: {heatmap}
Recommendations: {recommendations}

Events:
{events}

{rag_context}

{history}
Answer professionally. If the question is outside the briefing scope, state so honestly."#,
        date = briefing.date,
        overview = briefing.overview,
        heatmap = briefing.heatmap_json,
        recommendations = briefing.recommendations_json,
        events = events_summary.join("\n"),
        rag_context = rag_context,
        history = if history_context.is_empty() {
            String::new()
        } else {
            format!("Recent conversation:\n{}\n", history_context)
        },
    );

    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);
    let raw_stream = doubao
        .chat_stream(&system_prompt, &payload.message)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Doubao chat API call failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "AI service unavailable".to_string(),
                }),
            )
        })?;

    use futures_util::StreamExt;
    
    let pool_clone = state.pool.clone();
    let briefing_id = payload.briefing_id.clone();
    let user_message = payload.message.clone();

    let raw_stream = Box::pin(raw_stream);

    let s = futures_util::stream::unfold(
        (raw_stream, String::new(), false, pool_clone, briefing_id, user_message),
        move |(mut stream, mut full_response, mut done, pool, briefing_id, user_message)| async move {
            if done {
                return None;
            }
            match stream.next().await {
                Some(Ok(text)) => {
                    full_response.push_str(&text);
                    let bytes = axum::body::Bytes::from(text);
                    Some((Ok::<_, std::io::Error>(bytes), (stream, full_response, done, pool, briefing_id, user_message)))
                }
                Some(Err(e)) => {
                    done = true;
                    let err = std::io::Error::new(std::io::ErrorKind::Other, e);
                    Some((Err(err), (stream, full_response, done, pool, briefing_id, user_message)))
                }
                None => {
                    // Spawn background task to save to DB
                    let full_resp_clone = full_response.clone();
                    tokio::spawn(async move {
                        let chat_id = Uuid::new_v4().to_string();
                        let _ = sqlx::query(
                            r#"INSERT INTO chat_history (id, briefing_id, user_message, ai_response)
                               VALUES (?, ?, ?, ?)"#,
                        )
                        .bind(&chat_id)
                        .bind(&briefing_id)
                        .bind(&user_message)
                        .bind(&full_resp_clone)
                        .execute(&pool)
                        .await;
                    });
                    None
                }
            }
        }
    );

    let body = axum::body::Body::from_stream(s);
    
    let response = axum::response::Response::builder()
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(body)
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to build stream response");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to build stream response".to_string(),
                }),
            )
        })?;

    Ok(response)
}

/// POST /api/search
/// Semantic search over Qdrant vectors.
pub async fn search_vectors(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let qdrant = state.qdrant.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Qdrant is not available".to_string(),
            }),
        )
    })?;

    let limit = payload.limit.unwrap_or(10);
    let results = vectordb::search_similar(
        qdrant,
        &state.config.qdrant_collection,
        &payload.query,
        limit,
        payload.doc_type,
        payload.market,
        payload.category,
        &state.config,
    )
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Vector search failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Search failed".to_string(),
            }),
        )
    })?;

    Ok(Json(results))
}

async fn get_latest_briefing_summary(
    pool: &sqlx::SqlitePool,
) -> Result<Option<CachedBriefingSummary>, sqlx::Error> {
    let latest = sqlx::query_as::<_, (String, String)>(
        "SELECT id, created_at FROM briefings ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    let Some((briefing_id, created_at)) = latest else {
        return Ok(None);
    };

    let raw_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM raw_articles")
        .fetch_one(pool)
        .await?
        .max(0) as usize;
    let event_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM briefing_events WHERE briefing_id = ?")
            .bind(&briefing_id)
            .fetch_one(pool)
            .await?
            .max(0) as usize;

    Ok(Some(CachedBriefingSummary {
        created_at,
        raw_count,
        event_count,
    }))
}

async fn get_raw_article_count(pool: &sqlx::SqlitePool) -> Result<usize, sqlx::Error> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM raw_articles")
        .fetch_one(pool)
        .await?
        .max(0) as usize;
    Ok(count)
}

// ---- SQLx Row Types --------------------------------------------------------

#[derive(sqlx::FromRow)]
struct RawArticleRow {
    id: String,
    source_url: String,
    title: String,
    content: String,
    raw_language: String,
    market: String,
    created_at: String,
    pipeline_status: Option<String>,
}

#[derive(sqlx::FromRow)]
struct BriefingRow {
    id: String,
    date: String,
    overview: String,
    heatmap_json: String,
    recommendations_json: String,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct BriefingListRow {
    id: String,
    date: String,
    overview: String,
    event_count: i64,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: String,
    market: String,
    category: String,
    title: String,
    summary: String,
    impact_type: String,
    severity: i64,
    urgency: i64,
    confidence: i64,
    source_urls: String,
    analysis: String,
}

#[derive(sqlx::FromRow, Default)]
struct ChatHistoryRow {
    user_message: String,
    ai_response: String,
}

// ---- Agent Roles & Evolution Handlers ----------------------------------------

#[derive(Serialize)]
pub struct AgentRoleResponse {
    pub role_id: String,
    pub name: String,
    pub system_prompt: String,
    pub guidelines: String,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct AgentEvolutionLogResponse {
    pub id: String,
    pub role_id: String,
    pub old_guidelines: String,
    pub new_guidelines: String,
    pub reasoning: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct EvolveResponse {
    pub status: String,
    pub message: String,
}

/// GET /api/agent/roles
pub async fn get_agent_roles(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, (String, String, String, String, i64, String)>(
        "SELECT role_id, name, system_prompt, guidelines, version, updated_at FROM agent_playbook"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to fetch agent roles: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    let roles: Vec<AgentRoleResponse> = rows
        .into_iter()
        .map(|(role_id, name, system_prompt, guidelines, version, updated_at)| AgentRoleResponse {
            role_id,
            name,
            system_prompt,
            guidelines,
            version,
            updated_at,
        })
        .collect();

    Ok(Json(roles))
}

/// GET /api/agent/evolution-history
pub async fn get_evolution_history(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT id, role_id, old_guidelines, new_guidelines, reasoning, created_at FROM agent_evolution_log ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to fetch agent evolution logs: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    let logs: Vec<AgentEvolutionLogResponse> = rows
        .into_iter()
        .map(|(id, role_id, old_guidelines, new_guidelines, reasoning, created_at)| AgentEvolutionLogResponse {
            id,
            role_id,
            old_guidelines,
            new_guidelines,
            reasoning,
            created_at,
        })
        .collect();

    Ok(Json(logs))
}

/// POST /api/agent/evolve
pub async fn trigger_evolution(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.clone();
    let pool = state.pool.clone();

    // Trigger evolution runner in a separate thread so it doesn't block the HTTP request
    tokio::spawn(async move {
        tracing::info!("Manual agent evolution runner started");
        let client = DoubaoClient::new(&config.ark_api_key, &config.ark_endpoint_id, &config.llm_api_url);
        match crate::agent::evolution::evolve_agents(&pool, &client).await {
            Ok(summary) => {
                tracing::info!("Manual agent evolution (critiques) complete: {}", summary);
            }
            Err(e) => {
                tracing::error!("Manual agent evolution (critiques) failed: {}", e);
            }
        }
        match crate::agent::evolution::evolve_from_feedback_log(&pool, &client).await {
            Ok(updates) => {
                tracing::info!("Manual agent evolution (feedback logs) complete: evolved {} roles", updates.len());
            }
            Err(e) => {
                tracing::error!("Manual agent evolution (feedback logs) failed: {}", e);
            }
        }
    });

    Json(EvolveResponse {
        status: "accepted".to_string(),
        message: "Agent evolution triggered in background".to_string(),
    })
}

// ---- Bookmark & Evidence Chain Handlers -------------------------------------

#[derive(Deserialize)]
pub struct CreateBookmarkRequest {
    pub event_id: String,
}

#[derive(Serialize)]
pub struct BookmarkResponse {
    pub id: String,
    pub event_id: String,
    pub title: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct EvidenceChainResponse {
    pub bookmark: BookmarkResponse,
    pub chain: Vec<EvidenceChainItem>,
}

#[derive(Serialize)]
pub struct EvidenceChainItem {
    pub event_id: String,
    pub title: String,
    pub summary: String,
    pub date: String,
    pub direction: String, // "past" | "current" | "future"
    pub match_score: f64,
    pub relation_description: String,
}

/// POST /api/bookmarks
pub async fn post_bookmark(
    State(state): State<AppState>,
    Json(payload): Json<CreateBookmarkRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // 1. Check if event exists
    let event = sqlx::query!(
        "SELECT title, summary, analysis FROM events WHERE id = ?",
        payload.event_id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query event: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "Event not found".to_string() }),
        )
    })?;

    // 2. Check if already bookmarked
    let existing = sqlx::query!(
        r#"SELECT b.id, b.event_id, e.title, e.summary, b.keywords, b.created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           WHERE b.event_id = ?"#,
        payload.event_id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to check existing bookmark: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    if let Some(row) = existing {
        let keywords: Vec<String> = serde_json::from_str(&row.keywords).unwrap_or_default();
        return Ok((
            StatusCode::OK,
            Json(BookmarkResponse {
                id: row.id.unwrap_or_default(),
                event_id: row.event_id,
                title: row.title,
                summary: row.summary,
                keywords,
                created_at: row.created_at,
            }),
        ));
    }

    // 3. Extract evidence profile keywords using LLM
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);
    let keywords = agent::tracker::extract_evidence_profile(&doubao, &event.title, &event.summary, &event.analysis)
        .await
        .map_err(|e| {
            tracing::error!("Failed to extract evidence profile: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: format!("LLM keyword extraction failed: {}", e) }),
            )
        })?;

    let keywords_json = serde_json::to_string(&keywords).unwrap_or_default();
    let bookmark_id = Uuid::new_v4().to_string();

    // 4. Save bookmark to SQLite
    sqlx::query!(
        "INSERT INTO bookmarks (id, event_id, keywords) VALUES (?, ?, ?)",
        bookmark_id,
        payload.event_id,
        keywords_json
    )
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to save bookmark: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    // 5. Trigger retrospective tracing in background or sync?
    // Since it takes a bit of time (LLM calls for matches), we can run it asynchronously but let's wait a bit or spawn it.
    // Spawning it is better so the user gets a fast response.
    let pool_clone = state.pool.clone();
    let qdrant_clone = state.qdrant.clone();
    let config_clone = state.config.clone();
    let bookmark_id_clone = bookmark_id.clone();
    let event_id_clone = payload.event_id.clone();

    tokio::spawn(async move {
        let doubao = DoubaoClient::new(&config_clone.ark_api_key, &config_clone.ark_endpoint_id, &config_clone.llm_api_url);
        if let Err(e) = agent::tracker::run_retrospective_tracing(
            &doubao,
            &pool_clone,
            qdrant_clone.as_deref(),
            &bookmark_id_clone,
            &event_id_clone,
            &config_clone,
        )
        .await
        {
            tracing::error!("Background retrospective tracing failed for bookmark {}: {}", bookmark_id_clone, e);
        }
    });

    // 6. Fetch newly created bookmark row to return
    let created_row = sqlx::query!(
        r#"SELECT b.id, b.event_id, e.title, e.summary, b.created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           WHERE b.id = ?"#,
        bookmark_id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query created bookmark: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(BookmarkResponse {
            id: created_row.id.unwrap_or_default(),
            event_id: created_row.event_id,
            title: created_row.title,
            summary: created_row.summary,
            keywords,
            created_at: created_row.created_at,
        }),
    ))
}

/// GET /api/bookmarks
pub async fn get_bookmarks(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query!(
        r#"SELECT b.id, b.event_id, e.title, e.summary, b.keywords, b.created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           ORDER BY b.created_at DESC"#
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query bookmarks: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?;

    let bookmarks: Vec<BookmarkResponse> = rows
        .into_iter()
        .map(|row| {
            let keywords: Vec<String> = serde_json::from_str(&row.keywords).unwrap_or_default();
            BookmarkResponse {
                id: row.id.unwrap_or_default(),
                event_id: row.event_id,
                title: row.title,
                summary: row.summary,
                keywords,
                created_at: row.created_at,
            }
        })
        .collect();

    Ok(Json(bookmarks))
}

/// DELETE /api/bookmarks/:id
pub async fn delete_bookmark(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query!("DELETE FROM bookmarks WHERE id = ?", id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete bookmark: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: "Database error".to_string() }),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "Bookmark not found".to_string() }),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/bookmarks/:id/evidence-chain
pub async fn get_evidence_chain(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // 1. Fetch bookmark
    let bookmark_row = sqlx::query!(
        r#"SELECT b.id, b.event_id, e.title, e.summary, e.created_at as event_created_at, b.keywords, b.created_at as bookmark_created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           WHERE b.id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query bookmark: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "Database error".to_string() }),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "Bookmark not found".to_string() }),
        )
    })?;

    let keywords: Vec<String> = serde_json::from_str(&bookmark_row.keywords).unwrap_or_default();
    let bookmark = BookmarkResponse {
        id: bookmark_row.id.unwrap_or_default(),
        event_id: bookmark_row.event_id.clone(),
        title: bookmark_row.title.clone(),
        summary: bookmark_row.summary.clone(),
        keywords,
        created_at: bookmark_row.bookmark_created_at,
    };

    #[derive(sqlx::FromRow)]
    struct ChainRecord {
        matched_event_id: String,
        title: String,
        summary: String,
        created_at: String,
        direction: String,
        match_score: f64,
        match_reason: String,
    }

    let query_str = r#"
        SELECT c.matched_event_id, e.title, e.summary, e.created_at, c.direction, c.match_score, c.match_reason
        FROM bookmark_evidence_chain c
        JOIN events e ON c.matched_event_id = e.id
        WHERE c.bookmark_id = ?
    "#;

    // 2. Fetch matched events in the chain
    let mut matched_rows = sqlx::query_as::<_, ChainRecord>(query_str)
        .bind(&id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query evidence chain: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: "Database error".to_string() }),
            )
        })?;

    if matched_rows.is_empty() {
        tracing::info!("Evidence chain is empty for bookmark {}, triggering on-demand retrospective tracing...", id);
        let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);
        // Run it synchronously so we can return the newly found nodes immediately!
        if let Err(e) = agent::tracker::run_retrospective_tracing(
            &doubao,
            &state.pool,
            state.qdrant.as_ref().map(|q| q.as_ref()),
            &id,
            &bookmark_row.event_id,
            &state.config,
        )
        .await
        {
            tracing::error!("On-demand retrospective tracing failed for bookmark {}: {}", id, e);
        } else {
            // Re-fetch matched rows
            if let Ok(rows) = sqlx::query_as::<_, ChainRecord>(query_str)
                .bind(&id)
                .fetch_all(&state.pool)
                .await
            {
                matched_rows = rows;
            }
        }
    }

    // 3. Construct chronological chain with consolidation of duplicates by title
    use std::collections::HashMap;

    struct MergeHelper {
        item: EvidenceChainItem,
        reasons: Vec<String>,
    }

    let mut consolidated: HashMap<String, MergeHelper> = HashMap::new();

    let clean_title = |title: &str| -> String {
        let mut cleaned = title.trim().to_lowercase();
        let suffixes = [
            "网易新闻", "网易", "新浪财经", "新浪网", "新浪", "腾讯新闻", "腾讯",
            "搜狐网", "搜狐新闻", "搜狐", "百度新闻", "百度", "今日头条", "观察者网", "观察者",
            "界面新闻", "界面", "澎湃新闻", "澎湃", "中国珠宝网", "珠宝网"
        ];
        for s in &suffixes {
            cleaned = cleaned.replace(&format!("_{}", s), "");
            cleaned = cleaned.replace(&format!("-{}", s), "");
            cleaned = cleaned.replace(&format!("|{}", s), "");
            cleaned = cleaned.replace(&format!(" {}", s), "");
        }
        cleaned.chars()
            .filter(|c| c.is_alphanumeric() || (*c as u32 >= 0x4e00 && *c as u32 <= 0x9fff))
            .collect::<String>()
    };

    // Helper closure to insert/merge an item
    let mut merge_item = |item: EvidenceChainItem| {
        let norm_title = clean_title(&item.title);
        let reason = item.relation_description.trim().to_string();

        if let Some(existing) = consolidated.get_mut(&norm_title) {
            // Merge logic:
            // 1. If the new item is "current", it overrides the metadata
            if item.direction == "current" {
                existing.item.event_id = item.event_id;
                existing.item.summary = item.summary;
                existing.item.date = item.date;
                existing.item.direction = item.direction;
                existing.item.match_score = item.match_score;
            } else if existing.item.direction != "current" && item.match_score > existing.item.match_score {
                // Otherwise, keep the one with the higher match score for representation
                existing.item.event_id = item.event_id;
                existing.item.summary = item.summary;
                existing.item.date = item.date;
                existing.item.direction = item.direction;
                existing.item.match_score = item.match_score;
            }

            // Append relation description if not already present
            if !reason.is_empty() && !existing.reasons.contains(&reason) {
                if reason == "当前关注新闻事件" {
                    existing.reasons.insert(0, reason);
                } else {
                    existing.reasons.push(reason);
                }
            }
        } else {
            let reasons = if reason.is_empty() {
                vec![]
            } else {
                vec![reason]
            };
            consolidated.insert(norm_title, MergeHelper {
                item,
                reasons,
            });
        }
    };

    // Add matched items
    for row in matched_rows {
        merge_item(EvidenceChainItem {
            event_id: row.matched_event_id,
            title: row.title,
            summary: row.summary,
            date: row.created_at,
            direction: row.direction,
            match_score: row.match_score,
            relation_description: row.match_reason,
        });
    }

    // Add the current bookmarked event itself
    merge_item(EvidenceChainItem {
        event_id: bookmark_row.event_id.clone(),
        title: bookmark_row.title.clone(),
        summary: bookmark_row.summary.clone(),
        date: bookmark_row.event_created_at.clone(),
        direction: "current".to_string(),
        match_score: 1.0,
        relation_description: "当前关注新闻事件".to_string(),
    });

    // Extract consolidated items and finalize their relation descriptions
    let mut chain: Vec<EvidenceChainItem> = consolidated
        .into_iter()
        .map(|(_, mut helper)| {
            helper.reasons.retain(|r| !r.is_empty());
            helper.reasons.dedup();

            let relation_description = if helper.reasons.is_empty() {
                String::new()
            } else if helper.reasons.len() == 1 {
                helper.reasons[0].clone()
            } else {
                if helper.item.direction == "current" {
                    let mut other_reasons = helper.reasons.clone();
                    other_reasons.retain(|r| r != "当前关注新闻事件");
                    if other_reasons.is_empty() {
                        "当前关注新闻事件".to_string()
                    } else {
                        format!("当前关注新闻事件\n关联分析：\n{}", other_reasons.iter().map(|r| format!("• {}", r)).collect::<Vec<_>>().join("\n"))
                    }
                } else {
                    helper.reasons.iter().map(|r| format!("• {}", r)).collect::<Vec<_>>().join("\n")
                }
            };

            helper.item.relation_description = relation_description;
            helper.item
        })
        .collect();

    // Sort chronologically by date/created_at
    chain.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(Json(EvidenceChainResponse { bookmark, chain }))
}

// ─── Agent Parliament Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ProposalInput {
    pub proposer_role_id: String,
    pub title: String,
    pub description: String,
    pub proposal_type: String, // 'constitutional', 'budget', 'merger', 'admission'
}

#[derive(Deserialize)]
pub struct CrossoverInput {
    pub parent_a: String,
    pub parent_b: String,
    pub category: String,
}

pub async fn get_parliament_registry(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rows_res = sqlx::query_as::<_, (String, String, String, Option<String>, i32, i32, i64, i64, String, String, String, String)>(
        r#"SELECT r.role_id, name, status, sponsor_role_id, tasks_completed, tasks_failed, token_cost, compute_credits, faction, r.created_at, last_active_at, last_evolved_at
           FROM agent_parliament_registry r
           JOIN agent_playbook p ON r.role_id = p.role_id
           ORDER BY r.created_at DESC"#
    )
    .fetch_all(&state.pool)
    .await;

    match rows_res {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "role_id": r.0,
                    "name": r.1,
                    "status": r.2,
                    "sponsor_role_id": r.3,
                    "tasks_completed": r.4,
                    "tasks_failed": r.5,
                    "token_cost": r.6,
                    "compute_credits": r.7,
                    "faction": r.8,
                    "created_at": r.9,
                    "last_active_at": r.10,
                    "last_evolved_at": r.11,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn get_parliament_ledger(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rows_res = sqlx::query_as::<_, (String, String, Option<String>, String, String)>(
        "SELECT id, event_type, role_id, details, created_at FROM parliament_ledger ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await;

    match rows_res {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                let parsed_details: serde_json::Value = serde_json::from_str(&r.3).unwrap_or(serde_json::json!({ "raw": r.3 }));
                serde_json::json!({
                    "id": r.0,
                    "event_type": r.1,
                    "role_id": r.2,
                    "details": parsed_details,
                    "created_at": r.4,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn list_proposals(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rows_res = sqlx::query_as::<_, (String, String, String, String, String, String, f64, f64, String, Option<String>)>(
        "SELECT id, proposer_role_id, title, description, proposal_type, status, yes_votes, no_votes, created_at, resolved_at FROM parliament_proposals ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await;

    match rows_res {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "id": r.0,
                    "proposer_role_id": r.1,
                    "title": r.2,
                    "description": r.3,
                    "proposal_type": r.4,
                    "status": r.5,
                    "yes_votes": r.6,
                    "no_votes": r.7,
                    "created_at": r.8,
                    "resolved_at": r.9,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn create_proposal(
    State(state): State<AppState>,
    Json(input): Json<ProposalInput>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);

    match agent::parliament::propose_and_vote(&state.pool, &doubao, &input.proposer_role_id, &input.title, &input.description, &input.proposal_type).await {
        Ok(summary) => (StatusCode::CREATED, Json(serde_json::json!({ "status": "resolved", "summary": summary }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn vote_on_proposal(
    Path(id): Path<String>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "message": format!("Proposal {} is already fully resolved.", id) }))).into_response()
}

pub async fn trigger_parliament_trial(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);

    match agent::parliament::run_stagnation_audit(&state.pool, &doubao).await {
        Ok(log) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "log": log }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn distribute_compute_credits(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match agent::parliament::distribute_weekly_credits(&state.pool).await {
        Ok(msg) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "message": msg }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

pub async fn trigger_crossover(
    State(state): State<AppState>,
    Json(input): Json<CrossoverInput>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);

    match agent::parliament::run_hybrid_crossover(&state.pool, &doubao, &input.parent_a, &input.parent_b, &input.category).await {
        Ok(msg) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "message": msg }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct VetoInput {
    pub role_id: String,
    pub action: String, // "release" | "execute"
}

pub async fn veto_agent(
    State(state): State<AppState>,
    Json(input): Json<VetoInput>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);

    match agent::parliament::handle_human_veto(&state.pool, &doubao, &input.role_id, &input.action).await {
        Ok(msg) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "message": msg }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response()
    }
}

// ─── New Exposed Handlers ──────────────────────────────────────────────────

// 1. Sandbox Probation Check: POST /api/parliament/probation
pub async fn check_probation(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match agent::parliament::check_probation_agents(&state.pool).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "result": result }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 2. Playbook Rules CRUD
#[derive(Debug, Deserialize)]
pub struct RuleQuery {
    pub role_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRuleInput {
    pub role_id: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub version: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRuleInput {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub version: Option<i64>,
    pub status: Option<String>, // 'active', 'deprecated', etc.
}

pub async fn get_rules(
    State(state): State<AppState>,
    Query(query): Query<RuleQuery>,
) -> impl IntoResponse {
    let mut sql = "SELECT rule_id, role_id, content, reasoning, version, status, created_at FROM agent_playbook_rules WHERE 1=1".to_string();
    if query.role_id.is_some() {
        sql.push_str(" AND role_id = ?");
    }
    if query.status.is_some() {
        sql.push_str(" AND status = ?");
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut q = sqlx::query_as::<_, (String, String, String, Option<String>, i64, String, String)>(&sql);
    if let Some(ref r_id) = query.role_id {
        q = q.bind(r_id);
    }
    if let Some(ref stat) = query.status {
        q = q.bind(stat);
    }

    match q.fetch_all(&state.pool).await {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "rule_id": r.0,
                    "role_id": r.1,
                    "content": r.2,
                    "reasoning": r.3,
                    "version": r.4,
                    "status": r.5,
                    "created_at": r.6,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn create_rule(
    State(state): State<AppState>,
    Json(input): Json<CreateRuleInput>,
) -> impl IntoResponse {
    let rule_id = Uuid::new_v4().to_string();
    let version = input.version.unwrap_or(1);
    let created_at = chrono::Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();

    let res = sqlx::query(
        "INSERT INTO agent_playbook_rules (rule_id, role_id, content, reasoning, version, status, created_at) VALUES (?, ?, ?, ?, ?, 'active', ?)"
    )
    .bind(&rule_id)
    .bind(&input.role_id)
    .bind(&input.content)
    .bind(&input.reasoning)
    .bind(version)
    .bind(&created_at)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({
            "rule_id": rule_id,
            "role_id": input.role_id,
            "content": input.content,
            "reasoning": input.reasoning,
            "version": version,
            "status": "active",
            "created_at": created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn update_rule(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UpdateRuleInput>,
) -> impl IntoResponse {
    let mut query_builder = "UPDATE agent_playbook_rules SET ".to_string();
    let mut updates = Vec::new();
    
    if input.content.is_some() { updates.push("content = ?"); }
    if input.reasoning.is_some() { updates.push("reasoning = ?"); }
    if input.version.is_some() { updates.push("version = ?"); }
    if input.status.is_some() { updates.push("status = ?"); }

    if updates.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No fields to update".to_string() })).into_response();
    }

    query_builder.push_str(&updates.join(", "));
    query_builder.push_str(" WHERE rule_id = ?");

    let mut q = sqlx::query(&query_builder);
    if let Some(ref content) = input.content { q = q.bind(content); }
    if let Some(ref reasoning) = input.reasoning { q = q.bind(reasoning); }
    if let Some(version) = input.version { q = q.bind(version); }
    if let Some(ref status) = input.status { q = q.bind(status); }
    q = q.bind(&id);

    match q.execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "updated" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn delete_rule(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM agent_playbook_rules WHERE rule_id = ?").bind(&id).execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "deleted" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn compile_rules(
    Path(role_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match agent::evolution::compile_guidelines(&state.pool, &role_id).await {
        Ok(compiled) => (StatusCode::OK, Json(serde_json::json!({ "role_id": role_id, "compiled_guidelines": compiled }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 3. Regression Test Suite CRUD
#[derive(Debug, Deserialize)]
pub struct RegressionTestInput {
    pub event_id: Option<String>,
    pub category: String,
    pub title: String,
    pub summary: String,
    pub analysis: String,
}

pub async fn get_regression_tests(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, String, String, String, String, String)>("SELECT event_id, category, title, summary, analysis, created_at FROM regression_test_suite ORDER BY created_at DESC").fetch_all(&state.pool).await {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "event_id": r.0,
                    "category": r.1,
                    "title": r.2,
                    "summary": r.3,
                    "analysis": r.4,
                    "created_at": r.5,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn create_regression_test(
    State(state): State<AppState>,
    Json(input): Json<RegressionTestInput>,
) -> impl IntoResponse {
    let event_id = input.event_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let created_at = chrono::Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();

    let res = sqlx::query(
        "INSERT OR REPLACE INTO regression_test_suite (event_id, category, title, summary, analysis, created_at) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&event_id)
    .bind(&input.category)
    .bind(&input.title)
    .bind(&input.summary)
    .bind(&input.analysis)
    .bind(&created_at)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({
            "event_id": event_id,
            "category": input.category,
            "title": input.title,
            "summary": input.summary,
            "analysis": input.analysis,
            "created_at": created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn delete_regression_test(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM regression_test_suite WHERE event_id = ?").bind(&id).execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "deleted" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn trigger_regression_tests_auto_update(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match agent::evolution::auto_update_regression_suite(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "completed", "message": "Regression test suite auto-updated with top verified historical events." }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 4. Sandbox Verification
#[derive(Debug, Deserialize)]
pub struct VerifySandboxInput {
    pub role_id: String,
    pub guidelines: String,
}

pub async fn verify_sandbox(
    State(state): State<AppState>,
    Json(input): Json<VerifySandboxInput>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);

    match agent::evolution::verify_regression_sandbox(&state.pool, &doubao, &input.role_id, &input.guidelines).await {
        Ok(passed) => (StatusCode::OK, Json(serde_json::json!({ "role_id": input.role_id, "passed": passed }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 5. Agent Role CRUD extension
#[derive(Debug, Deserialize)]
pub struct UpdateAgentRoleInput {
    pub name: Option<String>,
    pub system_prompt: Option<String>,
    pub guidelines: Option<String>,
    pub version: Option<i64>,
}

pub async fn get_agent_role_detail(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let row_res = sqlx::query_as::<_, (String, String, String, String, i64, String)>(
        "SELECT role_id, name, system_prompt, guidelines, version, updated_at FROM agent_playbook WHERE role_id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await;

    match row_res {
        Ok(Some((role_id, name, system_prompt, guidelines, version, updated_at))) => {
            (StatusCode::OK, Json(serde_json::json!({
                "role_id": role_id,
                "name": name,
                "system_prompt": system_prompt,
                "guidelines": guidelines,
                "version": version,
                "updated_at": updated_at,
            }))).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Agent role not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn update_agent_role(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UpdateAgentRoleInput>,
) -> impl IntoResponse {
    let mut query_builder = "UPDATE agent_playbook SET ".to_string();
    let mut updates = Vec::new();
    
    if input.name.is_some() { updates.push("name = ?"); }
    if input.system_prompt.is_some() { updates.push("system_prompt = ?"); }
    if input.guidelines.is_some() { updates.push("guidelines = ?"); }
    if input.version.is_some() { updates.push("version = ?"); }
    updates.push("updated_at = datetime('now')");

    query_builder.push_str(&updates.join(", "));
    query_builder.push_str(" WHERE role_id = ?");

    let mut q = sqlx::query(&query_builder);
    if let Some(ref name) = input.name { q = q.bind(name); }
    if let Some(ref system_prompt) = input.system_prompt { q = q.bind(system_prompt); }
    if let Some(ref guidelines) = input.guidelines { q = q.bind(guidelines); }
    if let Some(version) = input.version { q = q.bind(version); }
    q = q.bind(&id);

    match q.execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "updated" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 6. Data Sources Configuration CRUD
#[derive(Debug, Deserialize)]
pub struct CreateDataSourceInput {
    pub url: String,
    pub source_type: String,
    pub language: Option<String>,
    pub is_active: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDataSourceInput {
    pub url: Option<String>,
    pub source_type: Option<String>,
    pub language: Option<String>,
    pub is_active: Option<i64>,
}

pub async fn list_data_sources(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, String, String, String, i64, String)>("SELECT id, url, source_type, language, is_active, created_at FROM data_sources ORDER BY created_at DESC").fetch_all(&state.pool).await {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "id": r.0,
                    "url": r.1,
                    "source_type": r.2,
                    "language": r.3,
                    "is_active": r.4 != 0,
                    "created_at": r.5,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn create_data_source(
    State(state): State<AppState>,
    Json(input): Json<CreateDataSourceInput>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let language = input.language.unwrap_or_else(|| "en".to_string());
    let is_active = input.is_active.unwrap_or(1);
    let created_at = chrono::Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();

    let res = sqlx::query(
        "INSERT INTO data_sources (id, url, source_type, language, is_active, created_at) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&input.url)
    .bind(&input.source_type)
    .bind(&language)
    .bind(is_active)
    .bind(&created_at)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({
            "id": id,
            "url": input.url,
            "source_type": input.source_type,
            "language": language,
            "is_active": is_active != 0,
            "created_at": created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn update_data_source(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<UpdateDataSourceInput>,
) -> impl IntoResponse {
    let mut query_builder = "UPDATE data_sources SET ".to_string();
    let mut updates = Vec::new();

    if input.url.is_some() { updates.push("url = ?"); }
    if input.source_type.is_some() { updates.push("source_type = ?"); }
    if input.language.is_some() { updates.push("language = ?"); }
    if input.is_active.is_some() { updates.push("is_active = ?"); }

    if updates.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No fields to update".to_string() })).into_response();
    }

    query_builder.push_str(&updates.join(", "));
    query_builder.push_str(" WHERE id = ?");

    let mut q = sqlx::query(&query_builder);
    if let Some(ref url) = input.url { q = q.bind(url); }
    if let Some(ref source_type) = input.source_type { q = q.bind(source_type); }
    if let Some(ref language) = input.language { q = q.bind(language); }
    if let Some(is_active) = input.is_active { q = q.bind(is_active); }
    q = q.bind(&id);

    match q.execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "updated" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn delete_data_source(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM data_sources WHERE id = ?").bind(&id).execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "deleted" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 7. Feedback Logging & View API
#[derive(Debug, Deserialize)]
pub struct CreateFeedbackInput {
    pub sender: String,
    pub receiver: String,
    pub event_id: Option<String>,
    pub feedback: String,
    pub is_resolved: Option<i64>,
}

pub async fn list_feedback(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, String, String, Option<String>, String, i64, String)>("SELECT id, sender, receiver, event_id, feedback, is_resolved, created_at FROM agent_feedback_log ORDER BY created_at DESC LIMIT 100").fetch_all(&state.pool).await {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "id": r.0,
                    "sender": r.1,
                    "receiver": r.2,
                    "event_id": r.3,
                    "feedback": r.4,
                    "is_resolved": r.5 != 0,
                    "created_at": r.6,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

pub async fn create_feedback(
    State(state): State<AppState>,
    Json(input): Json<CreateFeedbackInput>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();
    let is_resolved = input.is_resolved.unwrap_or(0);

    let res = sqlx::query(
        "INSERT INTO agent_feedback_log (id, sender, receiver, event_id, feedback, is_resolved, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&input.sender)
    .bind(&input.receiver)
    .bind(&input.event_id)
    .bind(&input.feedback)
    .bind(is_resolved)
    .bind(&created_at)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({
            "id": id,
            "sender": input.sender,
            "receiver": input.receiver,
            "event_id": input.event_id,
            "feedback": input.feedback,
            "is_resolved": is_resolved != 0,
            "created_at": created_at,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 8. Parliament Proposal Votes Retrieval API
pub async fn get_proposal_votes(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, String, String, f64, Option<String>, String)>("SELECT proposal_id, voter_role_id, vote, weight, reason, created_at FROM parliament_votes WHERE proposal_id = ?").bind(&id).fetch_all(&state.pool).await {
        Ok(rows) => {
            let list: Vec<serde_json::Value> = rows.into_iter().map(|r| {
                serde_json::json!({
                    "proposal_id": r.0,
                    "voter_role_id": r.1,
                    "vote": r.2,
                    "weight": r.3,
                    "reason": r.4,
                    "created_at": r.5,
                })
            }).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// 9. Bookmark Tracking Triggers
pub async fn trigger_bookmark_trace(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);
    let qdrant_clone = state.qdrant.clone();
    let pool = state.pool.clone();
    let config = state.config.clone();

    // Fetch associated event_id
    let row_res = sqlx::query!("SELECT event_id FROM bookmarks WHERE id = ?", id)
        .fetch_optional(&pool)
        .await;

    let event_id = match row_res {
        Ok(Some(row)) => row.event_id,
        _ => return (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Bookmark not found".to_string() })).into_response(),
    };

    // Call trace in a separate task so it doesn't block the request
    tokio::spawn(async move {
        tracing::info!("Starting retrospective tracing for bookmark {}", id);
        let qdrant_ref = qdrant_clone.as_deref();
        match agent::tracker::run_retrospective_tracing(&doubao, &pool, qdrant_ref, &id, &event_id, &config).await {
            Ok(count) => tracing::info!("Retrospective tracing finished. Associated {} events.", count),
            Err(e) => tracing::error!("Retrospective tracing failed: {}", e),
        }
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({ "status": "accepted", "message": "Retrospective tracing triggered in background" }))).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ProspectiveTrackInput {
    pub event_ids: Option<Vec<String>>,
}

pub async fn trigger_bookmark_track(
    State(state): State<AppState>,
    Json(input): Json<ProspectiveTrackInput>,
) -> impl IntoResponse {
    let doubao = DoubaoClient::new(&state.config.ark_api_key, &state.config.ark_endpoint_id, &state.config.llm_api_url);
    let qdrant_clone = state.qdrant.clone();
    let pool = state.pool.clone();
    let config = state.config.clone();

    // Fetch the list of event IDs
    let event_ids = match input.event_ids {
        Some(ids) => ids,
        None => {
            // Find latest briefing
            let latest_briefing_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM briefings ORDER BY created_at DESC LIMIT 1"
            )
            .fetch_optional(&pool)
            .await
            .unwrap_or(None);

            if let Some(briefing_id) = latest_briefing_id {
                sqlx::query!("SELECT event_id FROM briefing_events WHERE briefing_id = ?", briefing_id)
                    .fetch_all(&pool)
                    .await
                    .map(|rows| rows.into_iter().map(|r| r.event_id).collect::<Vec<String>>())
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        }
    };

    if event_ids.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No events specified and no briefings exist".to_string() })).into_response();
    }

    // Call track in a separate task so it doesn't block the request
    tokio::spawn(async move {
        tracing::info!("Starting prospective tracking for events {:?}", event_ids);
        let qdrant_ref = qdrant_clone.as_deref();
        
        let mut analyzed_events = Vec::new();
        for id in event_ids {
            let row_res = sqlx::query!(
                "SELECT id, market, category, title, summary, source_urls, impact_type, severity, urgency, confidence, analysis FROM events WHERE id = ?",
                id
            )
            .fetch_optional(&pool)
            .await;
            
            if let Ok(Some(row)) = row_res {
                let source_urls: Vec<String> = serde_json::from_str(&row.source_urls).unwrap_or_default();
                analyzed_events.push(agent::AnalyzedEvent {
                    id: row.id.unwrap_or_else(|| id.clone()),
                    market: row.market,
                    category: row.category,
                    title: row.title,
                    summary: row.summary,
                    source_urls,
                    impact_type: row.impact_type,
                    severity: row.severity as i32,
                    urgency: row.urgency as i32,
                    confidence: row.confidence as i32,
                    analysis: row.analysis,
                });
            }
        }

        match agent::tracker::run_prospective_tracking(&doubao, &pool, qdrant_ref, &analyzed_events, &config).await {
            Ok(count) => tracing::info!("Prospective tracking finished. Tracked {} events.", count),
            Err(e) => tracing::error!("Prospective tracking failed: {}", e),
        }
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({ "status": "accepted", "message": "Prospective tracking triggered in background" }))).into_response()
}

