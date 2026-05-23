use axum::{
    extract::{Query, State},
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
}

#[derive(Serialize)]
struct BriefingListItem {
    id: String,
    date: String,
    overview: String,
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
}

#[derive(Deserialize)]
pub struct ChatRequest {
    message: String,
    briefing_id: String,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
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
        r#"SELECT id, source_url, title, content, raw_language, market, created_at
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
    let briefing_row = sqlx::query_as::<_, BriefingRow>(
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

    let briefing = match briefing_row {
        Some(row) => row,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "No briefings found".to_string(),
                }),
            ));
        }
    };

    let event_rows = sqlx::query_as::<_, EventRow>(
        r#"SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls
           FROM events
           WHERE briefing_id = ?
           ORDER BY severity DESC, urgency DESC"#,
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

    let events: Vec<EventResponse> = event_rows
        .into_iter()
        .map(|row| EventResponse {
            id: row.id,
            market: row.market,
            category: row.category,
            title: row.title,
            summary: row.summary,
            impact_type: row.impact_type,
            severity: row.severity,
            urgency: row.urgency,
            confidence: row.confidence,
            source_urls: serde_json::from_str(&row.source_urls).unwrap_or(serde_json::json!([])),
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

    Ok(Json(response))
}

/// GET /api/briefings
pub async fn list_briefings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, BriefingListRow>(
        r#"SELECT id, date, overview, created_at
           FROM briefings
           ORDER BY created_at DESC"#,
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
    // Check if already running
    {
        let status = state.pipeline_status.read().await;
        if status.status == "running" {
            return (
                StatusCode::CONFLICT,
                Json(ScanResponse {
                    status: "conflict".to_string(),
                    message: "Pipeline is already running".to_string(),
                }),
            );
        }
    }

    if !query.force.unwrap_or(false) {
        match get_latest_briefing_summary(&state.pool).await {
            Ok(Some(summary)) => {
                {
                    let mut status = state.pipeline_status.write().await;
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
                }

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
                {
                    let mut status = state.pipeline_status.write().await;
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
                }

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

    let config = state.config.clone();
    let pool = state.pool.clone();
    let qdrant = state.qdrant.clone();
    let pipeline_status = state.pipeline_status.clone();

    let force = query.force.unwrap_or(false);

    tokio::spawn(async move {
        // Set status to running
        {
            let mut status = pipeline_status.write().await;
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

        match agent::run_pipeline_with_progress(&config, &pool, qdrant.as_deref(), force, report_progress).await {
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
        r#"SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls
           FROM events
           WHERE briefing_id = ?"#,
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
    let ai_response = doubao
        .chat(&system_prompt, &payload.message, false)
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

    let chat_id = Uuid::new_v4().to_string();
    let _ = sqlx::query(
        r#"INSERT INTO chat_history (id, briefing_id, user_message, ai_response)
           VALUES (?, ?, ?, ?)"#,
    )
    .bind(&chat_id)
    .bind(&payload.briefing_id)
    .bind(&payload.message)
    .bind(&ai_response)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to save chat history (non-fatal)");
    });

    Ok(Json(ChatResponse {
        response: ai_response,
        briefing_id: payload.briefing_id,
    }))
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
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events WHERE briefing_id = ?")
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
                tracing::info!("Manual agent evolution complete: {}", summary);
            }
            Err(e) => {
                tracing::error!("Manual agent evolution failed: {}", e);
            }
        }
    });

    Json(EvolveResponse {
        status: "accepted".to_string(),
        message: "Agent evolution triggered in background".to_string(),
    })
}

