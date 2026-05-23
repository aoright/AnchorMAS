pub mod handlers;
pub mod app_handlers;

use axum::Router;
use qdrant_client::Qdrant;
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::config::Config;

/// Pipeline execution status.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineStatus {
    pub status: String,       // "idle" | "running" | "completed" | "error"
    pub current_step: Option<String>,
    pub last_run: Option<String>,
    pub error_message: Option<String>,
    pub stats: PipelineStats,
    pub progress: PipelineProgressDetails,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PipelineStats {
    pub raw_count: usize,
    pub filtered_count: usize,
    pub analyzed_count: usize,
    pub verified_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PipelineProgressDetails {
    pub message: Option<String>,
    pub processed_count: usize,
    pub total_count: usize,
    pub output_count: usize,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub completed_batches: usize,
    pub failed_batches: usize,
    pub last_error: Option<String>,
    pub updated_at: Option<String>,
}

impl Default for PipelineStatus {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            current_step: None,
            last_run: None,
            error_message: None,
            stats: PipelineStats::default(),
            progress: PipelineProgressDetails::default(),
        }
    }
}

impl PipelineStatus {
    pub async fn from_database(pool: &SqlitePool) -> Self {
        let raw_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM raw_articles")
            .fetch_one(pool)
            .await
            .unwrap_or(0) as usize;

        let latest_briefing =
            sqlx::query_as::<_, (String, String)>(
                "SELECT id, created_at FROM briefings ORDER BY created_at DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .unwrap_or(None);

        if let Some((briefing_id, created_at)) = latest_briefing {
            let event_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM events WHERE briefing_id = ?",
            )
            .bind(&briefing_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0) as usize;

            Self {
                status: "completed".to_string(),
                current_step: None,
                last_run: Some(created_at.clone()),
                error_message: None,
                stats: PipelineStats {
                    raw_count,
                    filtered_count: event_count,
                    analyzed_count: event_count,
                    verified_count: event_count,
                },
                progress: PipelineProgressDetails {
                    message: Some("Loaded latest briefing from database".to_string()),
                    processed_count: event_count,
                    total_count: event_count,
                    output_count: event_count,
                    updated_at: Some(created_at),
                    ..PipelineProgressDetails::default()
                },
            }
        } else {
            Self {
                status: "idle".to_string(),
                stats: PipelineStats {
                    raw_count,
                    ..PipelineStats::default()
                },
                progress: PipelineProgressDetails {
                    message: if raw_count > 0 {
                        Some("Loaded raw articles from database".to_string())
                    } else {
                        None
                    },
                    output_count: raw_count,
                    ..PipelineProgressDetails::default()
                },
                ..PipelineStatus::default()
            }
        }
    }
}

/// Shared application state accessible by all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    pub qdrant: Option<Arc<Qdrant>>,
    pub pipeline_status: Arc<RwLock<PipelineStatus>>,
}

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    Router::new()
        // ── Dashboard API (existing) ────────────────────────────────────
        .route("/api/raw-articles", axum::routing::get(handlers::get_raw_articles))
        .route("/api/pipeline/status", axum::routing::get(handlers::get_pipeline_status))
        .route("/api/briefing/latest", axum::routing::get(handlers::get_latest_briefing))
        .route("/api/briefings", axum::routing::get(handlers::list_briefings))
        .route("/api/scan", axum::routing::post(handlers::trigger_scan))
        .route("/api/chat", axum::routing::post(handlers::chat))
        .route("/api/search", axum::routing::post(handlers::search_vectors))
        .route("/api/agent/roles", axum::routing::get(handlers::get_agent_roles))
        .route("/api/agent/evolution-history", axum::routing::get(handlers::get_evolution_history))
        .route("/api/agent/evolve", axum::routing::post(handlers::trigger_evolution))
        .route("/api/bookmarks", axum::routing::post(handlers::post_bookmark).get(handlers::get_bookmarks))
        .route("/api/bookmarks/:id", axum::routing::delete(handlers::delete_bookmark))
        .route("/api/bookmarks/:id/evidence-chain", axum::routing::get(handlers::get_evidence_chain))
        // ── App Frontend API (new) ──────────────────────────────────────
        // News
        .route("/app/news", axum::routing::get(app_handlers::list_news))
        .route("/app/news/:id", axum::routing::get(app_handlers::get_news_detail))
        // Briefings
        .route("/app/briefings", axum::routing::get(app_handlers::list_briefings))
        .route("/app/briefings/latest", axum::routing::get(app_handlers::get_latest_briefing))
        .route("/app/briefings/:id", axum::routing::get(app_handlers::get_briefing_by_id))
        // Chat Sessions
        .route("/app/chat/sessions", axum::routing::get(app_handlers::list_sessions).post(app_handlers::create_session))
        .route("/app/chat/sessions/:id", axum::routing::delete(app_handlers::delete_session))
        .route("/app/chat/sessions/:id/messages", axum::routing::get(app_handlers::get_session_messages).post(app_handlers::send_message))
        // Bookmarks
        .route("/app/bookmarks", axum::routing::get(app_handlers::list_bookmarks).post(app_handlers::create_bookmark))
        .route("/app/bookmarks/:id", axum::routing::delete(app_handlers::delete_bookmark))
        .route("/app/bookmarks/:id/evidence", axum::routing::get(app_handlers::get_evidence_chain))
        // Settings
        .route("/app/settings", axum::routing::get(app_handlers::get_settings).put(app_handlers::update_settings))
        // TTS
        .route("/app/tts", axum::routing::post(app_handlers::synthesize_speech))
        .layer(cors)
        .with_state(state)
}

