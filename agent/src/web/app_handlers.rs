use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::AppState;
use crate::agent::DoubaoClient;
use crate::vectordb;

// ══════════════════════════════════════════════════════════════════════════════
// Common Types
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
}

fn not_found(msg: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 1: News Feed (市场新闻)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize, Default)]
pub struct NewsListQuery {
    pub market: Option<String>,
    pub category: Option<String>,
    pub page: Option<i64>,
    pub size: Option<i64>,
}

#[derive(Serialize)]
pub struct NewsListResponse {
    pub items: Vec<NewsItem>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[derive(Serialize)]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub market: String,
    pub category: String,
    pub impact_type: String,
    pub severity: i64,
    pub urgency: i64,
    pub confidence: i64,
    pub source_urls: serde_json::Value,
    pub analysis: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct NewsDetailResponse {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub market: String,
    pub category: String,
    pub impact_type: String,
    pub severity: i64,
    pub urgency: i64,
    pub confidence: i64,
    pub source_urls: serde_json::Value,
    pub analysis: String,
    pub created_at: String,
    pub raw_sources: Vec<RawSourceDetail>,
}

#[derive(Serialize, Clone)]
pub struct RawSourceDetail {
    pub title: String,
    pub source_url: String,
    pub content: String,
}

/// GET /app/news — paginated news list with optional market/category filter
pub async fn list_news(
    State(state): State<AppState>,
    Query(query): Query<NewsListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let size = query.size.unwrap_or(20).max(1).min(100);
    let offset = (page - 1) * size;

    // Build dynamic WHERE clause
    let mut conditions = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(ref market) = query.market {
        conditions.push("e.market = ?");
        bind_values.push(market.clone());
    }
    if let Some(ref category) = query.category {
        conditions.push("e.category = ?");
        bind_values.push(category.clone());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM events e {}", where_clause);
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for v in &bind_values {
        count_query = count_query.bind(v);
    }
    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to count news");
            internal_error("Database error")
        })?;

    // Fetch page
    let select_sql = format!(
        r#"SELECT e.id, e.title, e.summary, e.market, e.category, e.impact_type,
                  e.severity, e.urgency, e.confidence, e.source_urls, e.analysis, e.created_at
           FROM events e {}
           ORDER BY e.created_at DESC
           LIMIT ? OFFSET ?"#,
        where_clause
    );
    let mut select_query = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, i64, String, String, String)>(&select_sql);
    for v in &bind_values {
        select_query = select_query.bind(v);
    }
    select_query = select_query.bind(size).bind(offset);

    let rows = select_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to query news");
            internal_error("Database error")
        })?;

    let items: Vec<NewsItem> = rows
        .into_iter()
        .map(|r| NewsItem {
            id: r.0,
            title: r.1,
            summary: r.2,
            market: r.3,
            category: r.4,
            impact_type: r.5,
            severity: r.6,
            urgency: r.7,
            confidence: r.8,
            source_urls: serde_json::from_str(&r.9).unwrap_or(serde_json::json!([])),
            analysis: r.10,
            created_at: r.11,
        })
        .collect();

    Ok(Json(NewsListResponse {
        items,
        total,
        page,
        size,
    }))
}

/// GET /app/news/:id — full news detail with raw article content
pub async fn get_news_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, i64, String, String, String)>(
        r#"SELECT id, title, summary, market, category, impact_type,
                  severity, urgency, confidence, source_urls, analysis, created_at
           FROM events WHERE id = ?"#,
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query news detail");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("News not found"))?;

    let source_urls_val: serde_json::Value =
        serde_json::from_str(&row.9).unwrap_or(serde_json::json!([]));

    // Fetch raw articles by source_urls
    let mut raw_sources = Vec::new();
    if let Some(urls) = source_urls_val.as_array() {
        let url_strings: Vec<&str> = urls.iter().filter_map(|v| v.as_str()).collect();
        if !url_strings.is_empty() {
            for url_chunk in url_strings.chunks(50) {
                let mut qb = sqlx::QueryBuilder::new(
                    "SELECT source_url, title, content FROM raw_articles WHERE source_url IN (",
                );
                let mut sep = qb.separated(", ");
                for url in url_chunk {
                    sep.push_bind(*url);
                }
                sep.push_unseparated(")");

                let q = qb.build_query_as::<(String, String, String)>();
                if let Ok(rows) = q.fetch_all(&state.pool).await {
                    for (source_url, title, content) in rows {
                        raw_sources.push(RawSourceDetail {
                            title,
                            source_url,
                            content,
                        });
                    }
                }
            }
        }
    }

    Ok(Json(NewsDetailResponse {
        id: row.0,
        title: row.1,
        summary: row.2,
        market: row.3,
        category: row.4,
        impact_type: row.5,
        severity: row.6,
        urgency: row.7,
        confidence: row.8,
        source_urls: source_urls_val,
        analysis: row.10,
        created_at: row.11,
        raw_sources,
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 2: Briefings (简报)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize, Default)]
pub struct BriefingListQuery {
    pub page: Option<i64>,
    pub size: Option<i64>,
}

#[derive(Serialize)]
pub struct BriefingListItem {
    pub id: String,
    pub date: String,
    pub overview: serde_json::Value,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct BriefingListResponse {
    pub items: Vec<BriefingListItem>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[derive(Serialize)]
pub struct BriefingDetailResponse {
    pub id: String,
    pub date: String,
    pub overview: serde_json::Value,
    pub heatmap: serde_json::Value,
    pub recommendations: serde_json::Value,
    pub events: Vec<NewsItem>,
    pub created_at: String,
}

#[derive(Deserialize, Default)]
pub struct BriefingDetailQuery {
    pub market: Option<String>,
}

/// GET /app/briefings — paginated briefing list
pub async fn list_briefings(
    State(state): State<AppState>,
    Query(query): Query<BriefingListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let size = query.size.unwrap_or(20).max(1).min(100);
    let offset = (page - 1) * size;

    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM briefings")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to count briefings");
            internal_error("Database error")
        })?;

    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        r#"SELECT id, date, overview, created_at
           FROM briefings
           ORDER BY created_at DESC
           LIMIT ? OFFSET ?"#,
    )
    .bind(size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query briefings");
        internal_error("Database error")
    })?;

    let items: Vec<BriefingListItem> = rows
        .into_iter()
        .map(|r| BriefingListItem {
            id: r.0,
            date: r.1,
            overview: serde_json::from_str(&r.2).unwrap_or(serde_json::json!({})),
            created_at: r.3,
        })
        .collect();

    Ok(Json(BriefingListResponse {
        items,
        total,
        page,
        size,
    }))
}

/// GET /app/briefings/latest — latest briefing with full JSON
pub async fn get_latest_briefing(
    State(state): State<AppState>,
    Query(query): Query<BriefingDetailQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let briefing = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        r#"SELECT id, date, overview, heatmap_json, recommendations_json, created_at
           FROM briefings ORDER BY created_at DESC LIMIT 1"#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query latest briefing");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("No briefings found"))?;

    build_briefing_response(&state, briefing, query.market.as_deref()).await
}

/// GET /app/briefings/:id — specific briefing detail
pub async fn get_briefing_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BriefingDetailQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let briefing = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        r#"SELECT id, date, overview, heatmap_json, recommendations_json, created_at
           FROM briefings WHERE id = ?"#,
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query briefing");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("Briefing not found"))?;

    build_briefing_response(&state, briefing, query.market.as_deref()).await
}

async fn build_briefing_response(
    state: &AppState,
    briefing: (String, String, String, String, String, String),
    market_filter: Option<&str>,
) -> Result<impl IntoResponse, ApiError> {
    let (id, date, overview, heatmap_json, recommendations_json, created_at) = briefing;

    // Load events for this briefing
    let events = if let Some(market) = market_filter {
        sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, i64, String, String, String)>(
            r#"SELECT id, title, summary, market, category, impact_type,
                      severity, urgency, confidence, source_urls, analysis, created_at
               FROM events WHERE briefing_id = ? AND market = ?
               ORDER BY severity DESC, urgency DESC"#,
        )
        .bind(&id)
        .bind(market)
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, i64, String, String, String)>(
            r#"SELECT id, title, summary, market, category, impact_type,
                      severity, urgency, confidence, source_urls, analysis, created_at
               FROM events WHERE briefing_id = ?
               ORDER BY severity DESC, urgency DESC"#,
        )
        .bind(&id)
        .fetch_all(&state.pool)
        .await
    }
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query briefing events");
        internal_error("Database error")
    })?;

    let event_items: Vec<NewsItem> = events
        .into_iter()
        .map(|r| NewsItem {
            id: r.0,
            title: r.1,
            summary: r.2,
            market: r.3,
            category: r.4,
            impact_type: r.5,
            severity: r.6,
            urgency: r.7,
            confidence: r.8,
            source_urls: serde_json::from_str(&r.9).unwrap_or(serde_json::json!([])),
            analysis: r.10,
            created_at: r.11,
        })
        .collect();

    Ok(Json(BriefingDetailResponse {
        id,
        date,
        overview: serde_json::from_str(&overview).unwrap_or(serde_json::json!({})),
        heatmap: serde_json::from_str(&heatmap_json).unwrap_or(serde_json::json!({})),
        recommendations: serde_json::from_str(&recommendations_json)
            .unwrap_or(serde_json::json!([])),
        events: event_items,
        created_at,
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 3: Chat with Sessions (对话)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct ChatSessionResponse {
    pub id: String,
    pub title: String,
    pub context_type: String,
    pub context_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub title: Option<String>,
    pub context_type: Option<String>, // "free" | "news" | "briefing"
    pub context_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChatMessageResponse {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
}

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub user_message: ChatMessageResponse,
    pub ai_message: ChatMessageResponse,
}

/// GET /app/chat/sessions — list all chat sessions
pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>, String, String)>(
        r#"SELECT id, title, context_type, context_id, created_at, updated_at
           FROM chat_sessions ORDER BY updated_at DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to list chat sessions");
        internal_error("Database error")
    })?;

    let sessions: Vec<ChatSessionResponse> = rows
        .into_iter()
        .map(|r| ChatSessionResponse {
            id: r.0,
            title: r.1,
            context_type: r.2,
            context_id: r.3,
            created_at: r.4,
            updated_at: r.5,
        })
        .collect();

    Ok(Json(sessions))
}

/// POST /app/chat/sessions — create a new chat session
pub async fn create_session(
    State(state): State<AppState>,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let id = Uuid::new_v4().to_string();
    let context_type = payload.context_type.unwrap_or_else(|| "free".to_string());
    let title = payload.title.unwrap_or_else(|| {
        match context_type.as_str() {
            "news" => "新闻对话".to_string(),
            "briefing" => "简报对话".to_string(),
            _ => "自由对话".to_string(),
        }
    });

    sqlx::query(
        r#"INSERT INTO chat_sessions (id, title, context_type, context_id) VALUES (?, ?, ?, ?)"#,
    )
    .bind(&id)
    .bind(&title)
    .bind(&context_type)
    .bind(&payload.context_id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to create chat session");
        internal_error("Database error")
    })?;

    let session = sqlx::query_as::<_, (String, String, String, Option<String>, String, String)>(
        "SELECT id, title, context_type, context_id, created_at, updated_at FROM chat_sessions WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to fetch created session");
        internal_error("Database error")
    })?;

    Ok((
        StatusCode::CREATED,
        Json(ChatSessionResponse {
            id: session.0,
            title: session.1,
            context_type: session.2,
            context_id: session.3,
            created_at: session.4,
            updated_at: session.5,
        }),
    ))
}

/// DELETE /app/chat/sessions/:id — delete a chat session
pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Delete messages first (CASCADE should handle but be explicit)
    let _ = sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await;

    let result = sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to delete session");
            internal_error("Database error")
        })?;

    if result.rows_affected() == 0 {
        return Err(not_found("Session not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// GET /app/chat/sessions/:id/messages — get chat history for a session
pub async fn get_session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify session exists
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_sessions WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to check session");
        internal_error("Database error")
    })?;

    if exists == 0 {
        return Err(not_found("Session not found"));
    }

    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        r#"SELECT id, session_id, role, content, created_at
           FROM chat_messages WHERE session_id = ?
           ORDER BY created_at ASC"#,
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query messages");
        internal_error("Database error")
    })?;

    let messages: Vec<ChatMessageResponse> = rows
        .into_iter()
        .map(|r| ChatMessageResponse {
            id: r.0,
            session_id: r.1,
            role: r.2,
            content: r.3,
            created_at: r.4,
        })
        .collect();

    Ok(Json(messages))
}

/// POST /app/chat/sessions/:id/messages — send a message and get AI response
pub async fn send_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // 1. Fetch session info
    let session = sqlx::query_as::<_, (String, String, String, Option<String>, String, String)>(
        "SELECT id, title, context_type, context_id, created_at, updated_at FROM chat_sessions WHERE id = ?",
    )
    .bind(&session_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to fetch session");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("Session not found"))?;

    let context_type = &session.2;
    let context_id = session.3.as_deref();

    // 2. Build context based on context_type
    let mut context_parts = Vec::new();

    match context_type.as_str() {
        "news" => {
            if let Some(event_id) = context_id {
                if let Ok(event) = sqlx::query_as::<_, (String, String, String, String, String, String)>(
                    "SELECT title, summary, market, category, analysis, source_urls FROM events WHERE id = ?"
                )
                .bind(event_id)
                .fetch_optional(&state.pool)
                .await
                {
                    if let Some(e) = event {
                        context_parts.push(format!(
                            "【关联新闻】\n标题: {}\n摘要: {}\n市场: {}\n分类: {}\n分析: {}\n",
                            e.0, e.1, e.2, e.3, e.4
                        ));
                        // Also fetch raw article content
                        if let Ok(urls) = serde_json::from_str::<Vec<String>>(&e.5) {
                            for url in urls.iter().take(3) {
                                if let Ok(Some(raw)) = sqlx::query_as::<_, (String, String)>(
                                    "SELECT title, content FROM raw_articles WHERE source_url = ?"
                                )
                                .bind(url)
                                .fetch_optional(&state.pool)
                                .await
                                {
                                    if !raw.1.is_empty() {
                                        let snippet = if raw.1.len() > 2000 { &raw.1[..2000] } else { &raw.1 };
                                        context_parts.push(format!("【原文摘录 - {}】\n{}\n", raw.0, snippet));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "briefing" => {
            if let Some(briefing_id) = context_id {
                if let Ok(Some(b)) = sqlx::query_as::<_, (String, String, String, String)>(
                    "SELECT date, overview, heatmap_json, recommendations_json FROM briefings WHERE id = ?"
                )
                .bind(briefing_id)
                .fetch_optional(&state.pool)
                .await
                {
                    context_parts.push(format!(
                        "【关联简报 ({})】\n概要: {}\n热力图: {}\n建议: {}\n",
                        b.0, b.1, b.2, b.3
                    ));
                    // Load events summary
                    if let Ok(events) = sqlx::query_as::<_, (String, String, String, String)>(
                        "SELECT title, summary, market, category FROM events WHERE briefing_id = ? ORDER BY severity DESC LIMIT 10"
                    )
                    .bind(briefing_id)
                    .fetch_all(&state.pool)
                    .await
                    {
                        if !events.is_empty() {
                            let mut event_summary = String::from("【简报关联事件】\n");
                            for ev in events {
                                event_summary.push_str(&format!("- [{}][{}] {}: {}\n", ev.2, ev.3, ev.0, ev.1));
                            }
                            context_parts.push(event_summary);
                        }
                    }
                }
            }
        }
        _ => {} // "free" — no extra context
    }

    // 3. Load recent conversation history (up to 20 messages)
    let history_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT role, content FROM chat_messages WHERE session_id = ? ORDER BY created_at ASC LIMIT 20",
    )
    .bind(&session_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut history = String::new();
    for (role, content) in &history_rows {
        let role_label = if role == "user" { "用户" } else { "助手" };
        history.push_str(&format!("{}: {}\n", role_label, content));
    }

    // 4. RAG: Query Qdrant for relevant context
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
            let mut matched = Vec::new();
            for item in search_results {
                if let (Some(title), Some(summary)) = (
                    item.get("title").and_then(|v| v.as_str()),
                    item.get("summary").and_then(|v| v.as_str()),
                ) {
                    let market = item.get("market").and_then(|v| v.as_str()).unwrap_or("Global");
                    let analysis = item.get("analysis").and_then(|v| v.as_str()).unwrap_or("");
                    matched.push(format!("- [{}] {}: {} (分析: {})", market, title, summary, analysis));
                }
            }
            if !matched.is_empty() {
                rag_context = format!("\n【RAG 检索到的相关历史事件】\n{}\n", matched.join("\n"));
            }
        }
    }

    // 5. Build system prompt
    let system_prompt = format!(
        r#"你是一个高级珠宝行业战略咨询专家，为用户提供专业的行业分析和咨询服务。
请结合以下上下文信息回答用户的问题。

{context}
{rag}
{conv_history}
要求：
- 回答应专业、准确、有深度
- 如果问题超出当前上下文范围，请坦诚告知
- 如果涉及具体数据，请引用来源"#,
        context = if context_parts.is_empty() {
            String::new()
        } else {
            context_parts.join("\n")
        },
        rag = rag_context,
        conv_history = if history.is_empty() {
            String::new()
        } else {
            format!("【对话历史】\n{}\n", history)
        },
    );

    // 6. Call LLM
    let doubao = DoubaoClient::new(
        &state.config.ark_api_key,
        &state.config.ark_endpoint_id,
        &state.config.llm_api_url,
    );
    let ai_response = doubao
        .chat(&system_prompt, &payload.message, false)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "LLM chat failed");
            internal_error("AI service unavailable")
        })?;

    // 7. Save user message
    let user_msg_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO chat_messages (id, session_id, role, content) VALUES (?, ?, 'user', ?)",
    )
    .bind(&user_msg_id)
    .bind(&session_id)
    .bind(&payload.message)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to save user message");
        internal_error("Database error")
    })?;

    // 8. Save AI message
    let ai_msg_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO chat_messages (id, session_id, role, content) VALUES (?, ?, 'assistant', ?)",
    )
    .bind(&ai_msg_id)
    .bind(&session_id)
    .bind(&ai_response)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to save AI message");
        internal_error("Database error")
    })?;

    // 9. Update session timestamp and auto-title if first message
    let msg_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?",
    )
    .bind(&session_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    if msg_count <= 2 {
        // Auto-generate title from first user message (truncated)
        let auto_title = if payload.message.len() > 30 {
            format!("{}...", &payload.message[..payload.message.char_indices().nth(30).map(|(i, _)| i).unwrap_or(payload.message.len())])
        } else {
            payload.message.clone()
        };
        let _ = sqlx::query(
            "UPDATE chat_sessions SET title = ?, updated_at = datetime('now') WHERE id = ? AND title IN ('', '自由对话', '新闻对话', '简报对话')",
        )
        .bind(&auto_title)
        .bind(&session_id)
        .execute(&state.pool)
        .await;
    } else {
        let _ = sqlx::query(
            "UPDATE chat_sessions SET updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&session_id)
        .execute(&state.pool)
        .await;
    }

    // 10. Build response
    let user_msg = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, session_id, role, content, created_at FROM chat_messages WHERE id = ?",
    )
    .bind(&user_msg_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to fetch user message");
        internal_error("Database error")
    })?;

    let ai_msg = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, session_id, role, content, created_at FROM chat_messages WHERE id = ?",
    )
    .bind(&ai_msg_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to fetch AI message");
        internal_error("Database error")
    })?;

    Ok(Json(SendMessageResponse {
        user_message: ChatMessageResponse {
            id: user_msg.0,
            session_id: user_msg.1,
            role: user_msg.2,
            content: user_msg.3,
            created_at: user_msg.4,
        },
        ai_message: ChatMessageResponse {
            id: ai_msg.0,
            session_id: ai_msg.1,
            role: ai_msg.2,
            content: ai_msg.3,
            created_at: ai_msg.4,
        },
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 4: Bookmarks (收藏 + 链路追踪)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
pub struct CreateBookmarkRequest {
    pub event_id: String,
}

#[derive(Serialize)]
pub struct AppBookmarkResponse {
    pub id: String,
    pub event_id: String,
    pub title: String,
    pub summary: String,
    pub market: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub evidence_count: i64,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct AppEvidenceChainResponse {
    pub bookmark: AppBookmarkResponse,
    pub chain: Vec<EvidenceItem>,
}

#[derive(Serialize)]
pub struct EvidenceItem {
    pub event_id: String,
    pub title: String,
    pub summary: String,
    pub market: String,
    pub date: String,
    pub direction: String,
    pub match_score: f64,
    pub relation_description: String,
}

/// GET /app/bookmarks
pub async fn list_bookmarks(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, String, String)>(
        r#"SELECT b.id, b.event_id, e.title, e.summary, e.market, e.category, b.keywords, b.created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           ORDER BY b.created_at DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query bookmarks");
        internal_error("Database error")
    })?;

    let mut bookmarks = Vec::new();
    for r in rows {
        let evidence_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bookmark_evidence_chain WHERE bookmark_id = ?",
        )
        .bind(&r.0)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

        let keywords: Vec<String> = serde_json::from_str(&r.6).unwrap_or_default();
        bookmarks.push(AppBookmarkResponse {
            id: r.0,
            event_id: r.1,
            title: r.2,
            summary: r.3,
            market: r.4,
            category: r.5,
            keywords,
            evidence_count,
            created_at: r.7,
        });
    }

    Ok(Json(bookmarks))
}

/// POST /app/bookmarks
pub async fn create_bookmark(
    State(state): State<AppState>,
    Json(payload): Json<CreateBookmarkRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Check if event exists
    let event = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT title, summary, analysis, market, category FROM events WHERE id = ?",
    )
    .bind(&payload.event_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query event");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("Event not found"))?;

    // Check if already bookmarked
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM bookmarks WHERE event_id = ?",
    )
    .bind(&payload.event_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to check existing bookmark");
        internal_error("Database error")
    })?;

    if let Some(existing_id) = existing {
        // Return existing bookmark
        let keywords_str = sqlx::query_scalar::<_, String>(
            "SELECT keywords FROM bookmarks WHERE id = ?",
        )
        .bind(&existing_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or_else(|_| "[]".to_string());
        let keywords: Vec<String> = serde_json::from_str(&keywords_str).unwrap_or_default();

        let evidence_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bookmark_evidence_chain WHERE bookmark_id = ?",
        )
        .bind(&existing_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

        return Ok((
            StatusCode::OK,
            Json(AppBookmarkResponse {
                id: existing_id,
                event_id: payload.event_id,
                title: event.0,
                summary: event.1,
                market: event.3,
                category: event.4,
                keywords,
                evidence_count,
                created_at: String::new(),
            }),
        ));
    }

    // Extract evidence profile
    let doubao = DoubaoClient::new(
        &state.config.ark_api_key,
        &state.config.ark_endpoint_id,
        &state.config.llm_api_url,
    );
    let keywords = crate::agent::tracker::extract_evidence_profile(&doubao, &event.0, &event.1, &event.2)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to extract evidence profile");
            internal_error("Keyword extraction failed")
        })?;

    let keywords_json = serde_json::to_string(&keywords).unwrap_or_else(|_| "[]".to_string());
    let bookmark_id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO bookmarks (id, event_id, keywords) VALUES (?, ?, ?)")
        .bind(&bookmark_id)
        .bind(&payload.event_id)
        .bind(&keywords_json)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to save bookmark");
            internal_error("Database error")
        })?;

    // Run retrospective tracing in background
    let pool_clone = state.pool.clone();
    let qdrant_clone = state.qdrant.clone();
    let config_clone = state.config.clone();
    let bookmark_id_clone = bookmark_id.clone();
    let event_id_clone = payload.event_id.clone();

    tokio::spawn(async move {
        let doubao = DoubaoClient::new(
            &config_clone.ark_api_key,
            &config_clone.ark_endpoint_id,
            &config_clone.llm_api_url,
        );
        if let Err(e) = crate::agent::tracker::run_retrospective_tracing(
            &doubao,
            &pool_clone,
            qdrant_clone.as_deref(),
            &bookmark_id_clone,
            &event_id_clone,
            &config_clone,
        )
        .await
        {
            tracing::error!("Retrospective tracing failed for bookmark {}: {}", bookmark_id_clone, e);
        }
    });

    Ok((
        StatusCode::CREATED,
        Json(AppBookmarkResponse {
            id: bookmark_id,
            event_id: payload.event_id,
            title: event.0,
            summary: event.1,
            market: event.3,
            category: event.4,
            keywords,
            evidence_count: 0,
            created_at: String::new(),
        }),
    ))
}

/// DELETE /app/bookmarks/:id
pub async fn delete_bookmark(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let result = sqlx::query("DELETE FROM bookmarks WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to delete bookmark");
            internal_error("Database error")
        })?;

    if result.rows_affected() == 0 {
        return Err(not_found("Bookmark not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// GET /app/bookmarks/:id/evidence
pub async fn get_evidence_chain(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Fetch bookmark
    let bookmark_row = sqlx::query_as::<_, (String, String, String, String, String, String, String, String)>(
        r#"SELECT b.id, b.event_id, e.title, e.summary, e.market, e.category, b.keywords, b.created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           WHERE b.id = ?"#,
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query bookmark");
        internal_error("Database error")
    })?
    .ok_or_else(|| not_found("Bookmark not found"))?;

    let keywords: Vec<String> = serde_json::from_str(&bookmark_row.6).unwrap_or_default();

    // Fetch evidence chain
    let chain_rows = sqlx::query_as::<_, (String, String, String, String, String, String, f64, String, String)>(
        r#"SELECT c.id, c.matched_event_id, e.title, e.summary, e.market, e.created_at, c.match_score, c.match_reason, c.direction
           FROM bookmark_evidence_chain c
           JOIN events e ON c.matched_event_id = e.id
           WHERE c.bookmark_id = ?
           ORDER BY e.created_at ASC"#,
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query evidence chain");
        internal_error("Database error")
    })?;

    // Consolidation logic by title
    use std::collections::HashMap;

    struct MergeHelper {
        item: EvidenceItem,
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

    let mut merge_item = |item: EvidenceItem| {
        let norm_title = clean_title(&item.title);
        let reason = item.relation_description.trim().to_string();

        if let Some(existing) = consolidated.get_mut(&norm_title) {
            if item.direction == "current" {
                existing.item.event_id = item.event_id;
                existing.item.summary = item.summary;
                existing.item.market = item.market;
                existing.item.date = item.date;
                existing.item.direction = item.direction;
                existing.item.match_score = item.match_score;
            } else if existing.item.direction != "current" && item.match_score > existing.item.match_score {
                existing.item.event_id = item.event_id;
                existing.item.summary = item.summary;
                existing.item.market = item.market;
                existing.item.date = item.date;
                existing.item.direction = item.direction;
                existing.item.match_score = item.match_score;
            }

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
    for r in chain_rows {
        merge_item(EvidenceItem {
            event_id: r.1,
            title: r.2,
            summary: r.3,
            market: r.4,
            date: r.5,
            direction: r.8,
            match_score: r.6,
            relation_description: r.7,
        });
    }

    // Add the current bookmarked event itself
    merge_item(EvidenceItem {
        event_id: bookmark_row.1.clone(),
        title: bookmark_row.2.clone(),
        summary: bookmark_row.3.clone(),
        market: bookmark_row.4.clone(),
        date: bookmark_row.7.clone(),
        direction: "current".to_string(),
        match_score: 1.0,
        relation_description: "当前关注新闻事件".to_string(),
    });

    let mut chain: Vec<EvidenceItem> = consolidated
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

    // Sort chronologically
    chain.sort_by(|a, b| a.date.cmp(&b.date));

    let matched_evidence_count = chain.iter().filter(|item| item.direction != "current").count() as i64;

    let bookmark = AppBookmarkResponse {
        id: bookmark_row.0,
        event_id: bookmark_row.1,
        title: bookmark_row.2,
        summary: bookmark_row.3,
        market: bookmark_row.4,
        category: bookmark_row.5,
        keywords,
        evidence_count: matched_evidence_count,
        created_at: bookmark_row.7,
    };

    Ok(Json(AppEvidenceChainResponse { bookmark, chain }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 5: User Settings (设置)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize, Deserialize)]
pub struct UserSettingsResponse {
    pub custom_keywords: Vec<String>,
    pub benchmark_companies: Vec<String>,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub custom_keywords: Option<Vec<String>>,
    pub benchmark_companies: Option<Vec<String>>,
}

/// GET /app/settings
pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    // The existing user_settings table uses key-value format: (key TEXT, value TEXT, updated_at TEXT)
    let kw_row = sqlx::query_as::<_, (String, String)>(
        "SELECT value, updated_at FROM user_settings WHERE key = 'keywords'",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query keywords setting");
        internal_error("Database error")
    })?;

    let bc_row = sqlx::query_as::<_, (String, String)>(
        "SELECT value, updated_at FROM user_settings WHERE key = 'benchmark_companies'",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to query benchmark_companies setting");
        internal_error("Database error")
    })?;

    let keywords: Vec<String> = kw_row
        .as_ref()
        .and_then(|r| serde_json::from_str(&r.0).ok())
        .unwrap_or_default();
    let companies: Vec<String> = bc_row
        .as_ref()
        .and_then(|r| serde_json::from_str(&r.0).ok())
        .unwrap_or_default();
    let updated_at = kw_row
        .map(|r| r.1)
        .or_else(|| bc_row.map(|r| r.1))
        .unwrap_or_default();

    Ok(Json(UserSettingsResponse {
        custom_keywords: keywords,
        benchmark_companies: companies,
        updated_at,
    }))
}

/// PUT /app/settings
pub async fn update_settings(
    State(state): State<AppState>,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(kw) = &payload.custom_keywords {
        let kw_json = serde_json::to_string(kw).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            r#"INSERT INTO user_settings (key, value, updated_at)
               VALUES ('keywords', ?, datetime('now'))
               ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        )
        .bind(&kw_json)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to update keywords");
            internal_error("Database error")
        })?;
    }

    if let Some(bc) = &payload.benchmark_companies {
        let bc_json = serde_json::to_string(bc).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            r#"INSERT INTO user_settings (key, value, updated_at)
               VALUES ('benchmark_companies', ?, datetime('now'))
               ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        )
        .bind(&bc_json)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to update benchmark_companies");
            internal_error("Database error")
        })?;
    }

    // Return updated state
    get_settings(State(state)).await
}

// ══════════════════════════════════════════════════════════════════════════════
// Module 6: TTS (语音合成 via CosyVoice)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
pub struct TtsRequest {
    pub text: String,
    pub voice: Option<String>,
}

/// POST /app/tts — text to speech via DashScope CosyVoice WebSocket API
///
/// Since CosyVoice only supports WebSocket, we proxy the request:
/// 1. Connect to DashScope WebSocket endpoint
/// 2. Send synthesis task
/// 3. Collect audio chunks
/// 4. Return as audio/mpeg response
pub async fn synthesize_speech(
    State(state): State<AppState>,
    Json(payload): Json<TtsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    if payload.text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Text cannot be empty".to_string(),
            }),
        ));
    }

    // Truncate very long text to prevent abuse
    let text = if payload.text.len() > 5000 {
        payload.text[..payload.text.char_indices().nth(2000).map(|(i, _)| i).unwrap_or(5000)].to_string()
    } else {
        payload.text.clone()
    };

    let voice = payload
        .voice
        .unwrap_or_else(|| state.config.tts_voice.clone());
    let model = state.config.tts_model.clone();
    let api_key = state.config.ark_api_key.clone();

    // Build WebSocket URL
    let ws_url = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";

    // Connect with auth header
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(ws_url)
        .header("Authorization", format!("bearer {}", api_key))
        .header("Host", "dashscope.aliyuncs.com")
        .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to build WebSocket request");
            internal_error("TTS service configuration error")
        })?;

    let (mut ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to connect to TTS WebSocket");
            internal_error("TTS service unavailable")
        })?;

    let task_id = Uuid::new_v4().to_string().replace('-', "");

    // Send run-task message (DashScope WebSocket protocol)
    let run_task = serde_json::json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "out"
        },
        "payload": {
            "task_group": "audio",
            "task": "tts",
            "function": "SpeechSynthesizer",
            "model": model,
            "parameters": {
                "voice": voice,
                "text_type": "PlainText",
                "format": "mp3",
                "sample_rate": 22050,
                "rate": 1.0,
                "volume": 50
            },
            "input": {
                "text": text
            }
        }
    });

    ws_stream
        .send(Message::Text(run_task.to_string()))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to send TTS task");
            internal_error("TTS task submission failed")
        })?;

    // Collect audio data
    let mut audio_data: Vec<u8> = Vec::new();
    let mut completed = false;

    while let Some(msg_result) = ws_stream.next().await {
        match msg_result {
            Ok(Message::Text(text_msg)) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text_msg) {
                    let event = json
                        .get("header")
                        .and_then(|h| h.get("event"))
                        .and_then(|e| e.as_str())
                        .unwrap_or("");

                    match event {
                        "task-started" => {
                            tracing::debug!("TTS task started: {}", task_id);
                        }
                        "result-generated" => {
                            // Audio data in payload.output.audio (base64)
                            if let Some(audio_b64) = json
                                .get("payload")
                                .and_then(|p| p.get("output"))
                                .and_then(|o| o.get("audio"))
                                .and_then(|a| a.as_str())
                            {
                                if let Ok(chunk) = base64::engine::general_purpose::STANDARD.decode(audio_b64) {
                                    audio_data.extend_from_slice(&chunk);
                                }
                            }
                        }
                        "task-finished" => {
                            completed = true;
                            break;
                        }
                        "task-failed" => {
                            let error_msg = json
                                .get("header")
                                .and_then(|h| h.get("error_message"))
                                .and_then(|e| e.as_str())
                                .unwrap_or("Unknown TTS error");
                            tracing::error!("TTS task failed: {}", error_msg);
                            let _ = ws_stream.close(None).await;
                            return Err(internal_error(&format!("TTS failed: {}", error_msg)));
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Binary(bin_data)) => {
                // Some versions send raw binary audio chunks
                audio_data.extend_from_slice(&bin_data);
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "WebSocket error during TTS");
                break;
            }
            _ => {}
        }
    }

    let _ = ws_stream.close(None).await;

    if audio_data.is_empty() && !completed {
        return Err(internal_error("TTS returned no audio data"));
    }

    // Return audio as response
    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "audio/mpeg"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "inline; filename=\"tts_output.mp3\"",
            ),
        ],
        audio_data,
    ))
}
