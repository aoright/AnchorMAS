pub mod analyst;
pub mod filter;
pub mod harvester;
pub mod synthesizer;
pub mod verifier;
pub mod evolution;
pub mod blackboard;
pub mod tracker;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::{mpsc, broadcast};
use tokio::time::sleep;
use uuid::Uuid;

use crate::config::Config;

// ─── Shared Types ───────────────────────────────────────────────────────────

/// A raw document harvested from an external source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawDocument {
    pub source_url: String,
    pub title: String,
    pub content: String,
    pub raw_language: String,
    pub timestamp: String,
}

/// An event that has been filtered and classified by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredEvent {
    pub id: String,
    pub market: String,
    pub category: String,
    pub title: String,
    pub summary: String,
    pub source_urls: Vec<String>,
}

/// An event with full analysis scores and detailed analysis text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedEvent {
    pub id: String,
    pub market: String,
    pub category: String,
    pub title: String,
    pub summary: String,
    pub source_urls: Vec<String>,
    pub impact_type: String,
    pub severity: i32,
    pub urgency: i32,
    pub confidence: i32,
    pub analysis: String,
}

/// Status and notes for a specific market region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketStatus {
    pub status: String,
    pub notes: String,
}

/// The final strategic briefing produced by the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicBriefing {
    pub id: String,
    pub date: String,
    pub overview: String,
    pub heatmap: HashMap<String, MarketStatus>,
    pub events: Vec<AnalyzedEvent>,
    pub recommendations: Vec<String>,
}


/// Pipeline stage currently being executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStep {
    Harvester,
    Filter,
    Analyst,
    Verifier,
    Synthesizer,
}

impl PipelineStep {
    pub fn as_str(self) -> &'static str {
        match self {
            PipelineStep::Harvester => "harvester",
            PipelineStep::Filter => "filter",
            PipelineStep::Analyst => "analyst",
            PipelineStep::Verifier => "verifier",
            PipelineStep::Synthesizer => "synthesizer",
        }
    }
}

/// Incremental pipeline progress emitted after each stage boundary.
#[derive(Debug, Clone, Default)]
pub struct PipelineProgress {
    pub current_step: Option<PipelineStep>,
    pub raw_count: Option<usize>,
    pub filtered_count: Option<usize>,
    pub analyzed_count: Option<usize>,
    pub verified_count: Option<usize>,
    pub message: Option<String>,
    pub processed_count: Option<usize>,
    pub total_count: Option<usize>,
    pub output_count: Option<usize>,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub completed_batches: Option<usize>,
    pub failed_batches: Option<usize>,
    pub last_error: Option<String>,
}

// ─── LLM Client ─────────────────────────────────────────────────────────────

/// Client for OpenAI-compatible LLM APIs (DashScope, Volcengine Ark, etc.).
#[derive(Debug, Clone)]
pub struct DoubaoClient {
    pub api_key: String,
    pub endpoint_id: String,
    pub api_url: String,
    pub client: reqwest::Client,
}

/// Request body for the Volcengine Ark API.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

/// A single message in the chat conversation.
#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Response from the chat completions API.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

impl DoubaoClient {
    /// Create a new LLM client with the given API key, model name, and API URL.
    pub fn new(api_key: &str, endpoint_id: &str, api_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            api_key: api_key.to_string(),
            endpoint_id: endpoint_id.to_string(),
            api_url: api_url.to_string(),
            client,
        }
    }

    /// Send a chat completion request.
    /// Returns the assistant's reply content. Enforces JSON mode if json_mode is true.
    pub async fn chat(&self, system_prompt: &str, user_prompt: &str, json_mode: bool) -> Result<String> {
        let mut final_system_prompt = system_prompt.to_string();
        if json_mode {
            let lower = final_system_prompt.to_lowercase();
            if !lower.contains("json") {
                final_system_prompt.push_str("\n\nPlease output the results in JSON format.");
            }
        }

        let response_format = if json_mode {
            Some(ResponseFormat {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let request_body = ChatRequest {
            model: self.endpoint_id.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: final_system_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            temperature: 0.3,
            response_format,
        };

        let response = self
            .client
            .post(&self.api_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error body".to_string());
            anyhow::bail!(
                "LLM API returned status {}: {}",
                status,
                error_body
            );
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse LLM API response")?;

        let content = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(content)
    }
}

// ─── Pipeline Orchestrator ──────────────────────────────────────────────────

/// Run the complete intelligence pipeline:
/// 1. Harvest data from RSS feeds and Reddit
/// 2. Filter & classify via Doubao
/// 3. Analyze with specialized prompts (parallel by category)
/// 4. Verify facts
/// 5. Synthesize strategic briefing
/// 6. Persist to SQLite & Qdrant
/// Reports stage-level progress after each stage boundary.
pub async fn run_pipeline_with_progress<F, Fut>(
    config: &Config,
    pool: &SqlitePool,
    qdrant: Option<&qdrant_client::Qdrant>,
    force: bool,
    mut progress: F,
) -> Result<StrategicBriefing>
where
    F: FnMut(PipelineProgress) -> Fut,
    Fut: Future<Output = ()>,
{
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let doubao = DoubaoClient::new(&config.ark_api_key, &config.ark_endpoint_id, &config.llm_api_url);

    // Step 1: Harvest
    progress(PipelineProgress {
        current_step: Some(PipelineStep::Harvester),
        message: Some("Harvesting RSS feeds and Reddit".to_string()),
        ..PipelineProgress::default()
    })
    .await;
    tracing::info!("Step 1/5: Harvesting data...");
    let raw_docs = harvester::harvest_with_progress(&http_client, pool, |p| {
        progress(PipelineProgress {
            current_step: Some(PipelineStep::Harvester),
            message: Some(p.message),
            processed_count: Some(p.processed_count),
            total_count: Some(p.total_count),
            output_count: Some(p.output_count),
            failed_batches: Some(p.failed_count),
            last_error: p.last_error,
            ..PipelineProgress::default()
        })
    })
    .await;
    tracing::info!(count = raw_docs.len(), "Harvesting complete");

    let (new_raw_docs, _updated_existing_count, cache_message) = if force {
        let cached_docs = load_recent_raw_articles(pool, 500).await?;
        let message = format!(
            "Force rescan: processing all {} raw articles from database",
            cached_docs.len()
        );
        (cached_docs, 0, message)
    } else if raw_docs.is_empty() {
        if let Ok(Some(briefing)) = get_latest_briefing_from_db(pool).await {
            progress(PipelineProgress {
                current_step: Some(PipelineStep::Synthesizer),
                raw_count: Some(count_raw_articles(pool).await.unwrap_or(0)),
                analyzed_count: Some(briefing.events.len()),
                verified_count: Some(briefing.events.len()),
                message: Some("No new articles found; loaded latest briefing from database".to_string()),
                ..PipelineProgress::default()
            })
            .await;
            tracing::info!("No new raw documents; returning latest briefing from database");
            return Ok(briefing);
        }

        let cached_docs = load_recent_raw_articles(pool, 500).await?;
        if cached_docs.is_empty() {
            progress(PipelineProgress {
                current_step: Some(PipelineStep::Synthesizer),
                raw_count: Some(0),
                message: Some("No new or cached documents available".to_string()),
                ..PipelineProgress::default()
            })
            .await;
            tracing::warn!("No documents harvested and no cached raw articles found");
            let briefing = StrategicBriefing {
                id: Uuid::new_v4().to_string(),
                date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
                overview: "今日未能采集到有效数据，请检查数据源连接状态。".to_string(),
                heatmap: HashMap::new(),
                events: Vec::new(),
                recommendations: vec!["检查RSS数据源连接状态。".to_string()],
            };
            save_briefing(pool, &briefing).await?;
            return Ok(briefing);
        }

        let message = format!(
            "No new articles found; continuing from {} cached raw articles",
            cached_docs.len()
        );
        (cached_docs, 0, message)
    } else {
        let updated_existing_count =
            update_existing_raw_articles_if_content_longer(pool, &raw_docs).await?;

        // Identify new documents in a single batch check
        let urls_to_check: Vec<String> = raw_docs.iter().map(|d| d.source_url.clone()).collect();
        let new_urls = filter_new_urls(pool, &urls_to_check).await;
        let mut new_raw_docs = Vec::new();
        for doc in &raw_docs {
            if new_urls.contains(&doc.source_url) {
                new_raw_docs.push(doc.clone());
            }
        }

        tracing::info!(
            total = raw_docs.len(),
            new_count = new_raw_docs.len(),
            updated_existing_count,
            "De-duplicated raw documents against database cache"
        );

        // Save only new articles to SQLite and Qdrant
        if !new_raw_docs.is_empty() {
            save_raw_articles(pool, &new_raw_docs).await?;

            if let Some(qclient) = qdrant {
                let collection = &config.qdrant_collection;
                if let Err(e) =
                    crate::vectordb::store_documents(qclient, collection, &new_raw_docs, config)
                        .await
                {
                    tracing::error!(error = %e, "Failed to store raw documents in Qdrant");
                }
            }
        }

        let message = format!(
            "Found {} harvested articles, {} are new and will be filtered; updated {} existing articles with longer content",
            raw_docs.len(),
            new_raw_docs.len(),
            updated_existing_count
        );
        (new_raw_docs, updated_existing_count, message)
    };

    if new_raw_docs.is_empty() {
        tracing::warn!("No new raw documents harvested after de-duplication");
        if let Ok(Some(briefing)) = get_latest_briefing_from_db(pool).await {
            tracing::info!("Returning latest briefing from database");
            return Ok(briefing);
        }
        
        let briefing = StrategicBriefing {
            id: Uuid::new_v4().to_string(),
            date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            overview: "今日未采集到新的珠宝行业新闻，且没有历史简报。".to_string(),
            heatmap: HashMap::new(),
            events: Vec::new(),
            recommendations: vec!["持续监控各市场动态。".to_string()],
        };
        save_briefing(pool, &briefing).await?;
        return Ok(briefing);
    }

    // Now, run the Blackboard Multi-Agent System!
    tracing::info!("Initializing Blackboard MAS for {} raw documents", new_raw_docs.len());
    
    let blackboard = Arc::new(blackboard::Blackboard::new());
    
    // Set initial work count for Raw articles
    blackboard.increment_work(new_raw_docs.len());

    // Spawn agent task runners in parallel!
    let config_arc = Arc::new(config.clone());
    
    // Subscribe receivers first to avoid tokio::sync::broadcast race conditions
    let gatekeeper_rx = blackboard.tx.subscribe();
    let dedup_rx = blackboard.tx.subscribe();
    let analyst_rx = blackboard.tx.subscribe();
    let peer_rx = blackboard.tx.subscribe();
    let critic_rx = blackboard.tx.subscribe();
    let refiner_rx = blackboard.tx.subscribe();

    // 1. Gatekeeper Task
    let gatekeeper_bb = blackboard.clone();
    let gatekeeper_client = doubao.clone();
    let gatekeeper_pool = pool.clone();
    let gatekeeper_config = config_arc.clone();
    tokio::spawn(async move {
        blackboard::start_gatekeeper(gatekeeper_bb, gatekeeper_rx, gatekeeper_client, gatekeeper_pool, gatekeeper_config).await;
    });

    // 2. De-duplicator Task (Re-enabled with Qdrant client support!)
    let dedup_bb = blackboard.clone();
    let dedup_pool = pool.clone();
    let dedup_qdrant = qdrant.cloned();
    let dedup_config = config_arc.clone();
    tokio::spawn(async move {
        blackboard::start_deduplicator(dedup_bb, dedup_rx, dedup_pool, dedup_qdrant, dedup_config).await;
    });

    // 3. Analyst Coordinator Task
    let analyst_bb = blackboard.clone();
    let analyst_client = doubao.clone();
    let analyst_pool = pool.clone();
    tokio::spawn(async move {
        blackboard::start_analyst_coordinator(analyst_bb, analyst_rx, analyst_client, analyst_pool).await;
    });

    // 4. Peer Reviewer Task
    let peer_bb = blackboard.clone();
    let peer_client = doubao.clone();
    let peer_pool = pool.clone();
    tokio::spawn(async move {
        blackboard::start_peer_reviewer(peer_bb, peer_rx, peer_client, peer_pool).await;
    });

    // 5. Critic Task
    let critic_bb = blackboard.clone();
    let critic_client = doubao.clone();
    let critic_pool = pool.clone();
    tokio::spawn(async move {
        blackboard::start_critic(critic_bb, critic_rx, critic_client, critic_pool).await;
    });

    // 6. Refiner (and Real-time Evolution) Task
    let refiner_bb = blackboard.clone();
    let refiner_client = doubao.clone();
    let refiner_pool = pool.clone();
    tokio::spawn(async move {
        blackboard::start_refiner(refiner_bb, refiner_rx, refiner_client, refiner_pool).await;
    });

    // Coordinator & Progress Reporter Loop on main thread
    let mut rx = blackboard.tx.subscribe();
    
    // Broadcast all raw articles to kick off the pipeline!
    for doc in &new_raw_docs {
        let _ = blackboard.tx.send(blackboard::AgentMessage::RawArticleAdded(doc.clone()));
    }
    let _ = blackboard.tx.send(blackboard::AgentMessage::AllScoutingDone);

    let mut consensus_events = Vec::new();
    let mut raw_processed = 0;
    let mut events_filtered = 0;
    let mut events_analyzed = 0;
    let mut events_verified = 0;
    let mut evolutions_run = 0;

    progress(PipelineProgress {
        current_step: Some(PipelineStep::Filter),
        raw_count: Some(new_raw_docs.len()),
        processed_count: Some(0),
        total_count: Some(new_raw_docs.len()),
        message: Some(cache_message),
        ..PipelineProgress::default()
    })
    .await;

    // Loop until active_count reaches 0
    loop {
        let count = blackboard.active_count.load(Ordering::SeqCst);
        if count == 0 {
            tracing::info!("Blackboard active work count reached 0, completing orchestration loop");
            break;
        }

        tokio::select! {
            msg_res = rx.recv() => {
                match msg_res {
                    Ok(blackboard::AgentMessage::RawArticleAdded(_)) => {
                        raw_processed += 1;
                        let msg = format!("MAS Blackboard: Processing raw article {} / {}", raw_processed, new_raw_docs.len());
                        progress(PipelineProgress {
                            current_step: Some(PipelineStep::Filter),
                            processed_count: Some(raw_processed),
                            total_count: Some(new_raw_docs.len()),
                            message: Some(msg),
                            ..PipelineProgress::default()
                        }).await;
                    }
                    Ok(blackboard::AgentMessage::FilteredEventAdded(event)) => {
                        events_filtered += 1;
                        let msg = format!("MAS Blackboard: Filtered and classified event: '{}'", event.title);
                        progress(PipelineProgress {
                            current_step: Some(PipelineStep::Analyst),
                            filtered_count: Some(events_filtered),
                            message: Some(msg),
                            ..PipelineProgress::default()
                        }).await;
                    }
                    Ok(blackboard::AgentMessage::AnalysisCompleted(event)) => {
                        events_analyzed += 1;
                        let msg = format!("MAS Blackboard: Domain analysis completed for: '{}'", event.title);
                        progress(PipelineProgress {
                            current_step: Some(PipelineStep::Verifier),
                            analyzed_count: Some(events_analyzed),
                            message: Some(msg),
                            ..PipelineProgress::default()
                        }).await;
                    }
                    Ok(blackboard::AgentMessage::ConsensusReached(event)) => {
                        events_verified += 1;
                        consensus_events.push(event.clone());
                        let msg = format!("MAS Blackboard: Fact-check consensus reached for event: '{}'", event.title);
                        progress(PipelineProgress {
                            current_step: Some(PipelineStep::Verifier),
                            verified_count: Some(events_verified),
                            message: Some(msg),
                            ..PipelineProgress::default()
                        }).await;
                    }
                    Ok(blackboard::AgentMessage::PlaybookUpdated { role_id, new_guidelines }) => {
                        evolutions_run += 1;
                        tracing::info!(role = %role_id, "Main orchestrator received playbook update event");
                        let msg = format!("MAS Blackboard: Mutated rules for '{}' (Total mutations: {}) -> {}", role_id, evolutions_run, new_guidelines);
                        progress(PipelineProgress {
                            current_step: Some(PipelineStep::Verifier),
                            message: Some(msg),
                            ..PipelineProgress::default()
                        }).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::warn!("Orchestrator broadcast channel closed unexpectedly");
                        break;
                    }
                    _ => {}
                }
            }
            _ = sleep(Duration::from_millis(100)) => {
                // Periodically check if idle count reached 0 in case broadcast gets missed
                if blackboard.active_count.load(Ordering::SeqCst) == 0 {
                    break;
                }
            }
        }
    }

    // Step 5: Synthesize briefing
    tracing::info!("Step 5/5: Synthesizing strategic briefing from {} consensus events...", consensus_events.len());
    progress(PipelineProgress {
        current_step: Some(PipelineStep::Synthesizer),
        verified_count: Some(consensus_events.len()),
        processed_count: Some(consensus_events.len()),
        total_count: Some(consensus_events.len()),
        output_count: Some(consensus_events.len()),
        message: Some(format!("Synthesizing final daily strategic briefing from {} consensus events", consensus_events.len())),
        ..PipelineProgress::default()
    })
    .await;

    // Run final all-agent feedback evolution pass before synthesizing briefing
    if let Err(e) = evolution::evolve_from_feedback_log(pool, &doubao).await {
        tracing::error!("Final all-agent feedback evolution pass failed: {}", e);
    }

    let briefing = if consensus_events.is_empty() {
        StrategicBriefing {
            id: Uuid::new_v4().to_string(),
            date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            overview: "今日未发现值得关注的重大珠宝行业事件。".to_string(),
            heatmap: HashMap::new(),
            events: Vec::new(),
            recommendations: vec!["持续监控各市场动态。".to_string()],
        }
    } else {
        synthesizer::synthesize_briefing(&doubao, pool, &consensus_events).await?
    };

    // Audit the synthesized briefing quality
    if !consensus_events.is_empty() {
        if let Err(e) = synthesizer::audit_briefing(&doubao, pool, &briefing).await {
            tracing::error!("Briefing quality audit failed: {}", e);
        }
        // Run a post-briefing evolution pass to process any synthesizer or evolution feedback logged at the end
        if let Err(e) = evolution::evolve_from_feedback_log(pool, &doubao).await {
            tracing::error!("Post-briefing evolution pass failed: {}", e);
        }
    }

    // Save briefing to SQLite
    save_briefing(pool, &briefing).await?;
    save_events(pool, &briefing.id, &briefing.events).await?;

    // Index analyzed events in Qdrant if available
    if let Some(qclient) = qdrant {
        let collection = &config.qdrant_collection;
        if let Err(e) = crate::vectordb::store_events(qclient, collection, &briefing.events, &briefing.id, config).await {
            tracing::error!(error = %e, "Failed to store events in Qdrant");
        }
    }

    // Run prospective tracking on newly generated events to match against active bookmarks
    if let Err(e) = tracker::run_prospective_tracking(&doubao, pool, qdrant, &briefing.events, config).await {
        tracing::error!(error = %e, "Failed to run prospective tracking on daily briefing events");
    }

    progress(PipelineProgress {
        current_step: Some(PipelineStep::Synthesizer),
        message: Some("Pipeline MAS complete!".to_string()),
        ..PipelineProgress::default()
    })
    .await;

    tracing::info!("Pipeline Blackboard MAS complete!");
    Ok(briefing)
}

// ─── Database Persistence Helpers ───────────────────────────────────────────

async fn save_raw_articles(pool: &SqlitePool, docs: &[RawDocument]) -> Result<()> {
    for doc in docs {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"INSERT OR IGNORE INTO raw_articles (id, source_url, title, content, raw_language)
               VALUES (?, ?, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(&doc.source_url)
        .bind(&doc.title)
        .bind(&doc.content)
        .bind(&doc.raw_language)
        .execute(pool)
        .await
        .context("Failed to save raw article")?;
    }
    tracing::info!(count = docs.len(), "Raw articles saved");
    Ok(())
}

async fn update_existing_raw_articles_if_content_longer(
    pool: &SqlitePool,
    docs: &[RawDocument],
) -> Result<usize> {
    let mut updated = 0;
    for doc in docs {
        let content_len = doc.content.chars().count() as i64;
        if content_len == 0 {
            continue;
        }

        let result = sqlx::query(
            r#"UPDATE raw_articles
               SET title = ?, content = ?, raw_language = ?
               WHERE source_url = ? AND length(content) < ?"#,
        )
        .bind(&doc.title)
        .bind(&doc.content)
        .bind(&doc.raw_language)
        .bind(&doc.source_url)
        .bind(content_len)
        .execute(pool)
        .await
        .context("Failed to update existing raw article content")?;

        updated += result.rows_affected() as usize;
    }

    if updated > 0 {
        tracing::info!(count = updated, "Existing raw articles updated with longer content");
    }
    Ok(updated)
}

async fn count_raw_articles(pool: &SqlitePool) -> Result<usize> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM raw_articles")
        .fetch_one(pool)
        .await
        .context("Failed to count raw articles")?;
    Ok(count.max(0) as usize)
}

async fn load_recent_raw_articles(pool: &SqlitePool, limit: i64) -> Result<Vec<RawDocument>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        r#"SELECT source_url, title, content, raw_language, created_at
           FROM raw_articles
           ORDER BY created_at DESC
           LIMIT ?"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to load cached raw articles")?;

    Ok(rows
        .into_iter()
        .map(|(source_url, title, content, raw_language, timestamp)| RawDocument {
            source_url,
            title,
            content,
            raw_language,
            timestamp,
        })
        .collect())
}

async fn save_briefing(pool: &SqlitePool, briefing: &StrategicBriefing) -> Result<()> {
    let heatmap_json = serde_json::to_string(&briefing.heatmap)?;
    let recommendations_json = serde_json::to_string(&briefing.recommendations)?;

    sqlx::query(
        r#"INSERT OR REPLACE INTO briefings (id, date, overview, heatmap_json, recommendations_json)
           VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind(&briefing.id)
    .bind(&briefing.date)
    .bind(&briefing.overview)
    .bind(&heatmap_json)
    .bind(&recommendations_json)
    .execute(pool)
    .await
    .context("Failed to save briefing")?;

    tracing::info!(id = %briefing.id, "Briefing saved to database");
    Ok(())
}

async fn save_events(pool: &SqlitePool, briefing_id: &str, events: &[AnalyzedEvent]) -> Result<()> {
    for event in events {
        let source_urls_json = serde_json::to_string(&event.source_urls)?;
        sqlx::query(
            r#"INSERT OR REPLACE INTO events
               (id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, briefing_id, analysis)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&event.id)
        .bind(&event.market)
        .bind(&event.category)
        .bind(&event.title)
        .bind(&event.summary)
        .bind(&event.impact_type)
        .bind(event.severity)
        .bind(event.urgency)
        .bind(event.confidence)
        .bind(&source_urls_json)
        .bind(briefing_id)
        .bind(&event.analysis)
        .execute(pool)
        .await
        .context("Failed to save event")?;
    }
    tracing::info!(count = events.len(), "Events saved to database");
    Ok(())
}

async fn get_latest_briefing_from_db(pool: &SqlitePool) -> Result<Option<StrategicBriefing>> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, date, overview, heatmap_json, recommendations_json FROM briefings ORDER BY created_at DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id, date, overview, heatmap_json, recommendations_json)) = row {
        let heatmap = serde_json::from_str(&heatmap_json).unwrap_or_default();
        let recommendations = serde_json::from_str(&recommendations_json).unwrap_or_default();

        let event_rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, i64, String, String)>(
            "SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, analysis FROM events WHERE briefing_id = ?"
        )
        .bind(&id)
        .fetch_all(pool)
        .await?;

        let events = event_rows
            .into_iter()
            .map(|(ev_id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, analysis)| {
                let urls = serde_json::from_str(&source_urls).unwrap_or_default();
                AnalyzedEvent {
                    id: ev_id,
                    market,
                    category,
                    title,
                    summary,
                    source_urls: urls,
                    impact_type,
                    severity: severity as i32,
                    urgency: urgency as i32,
                    confidence: confidence as i32,
                    analysis,
                }
            })
            .collect();

        Ok(Some(StrategicBriefing {
            id,
            date,
            overview,
            heatmap,
            events,
            recommendations,
        }))
    } else {
        Ok(None)
    }
}

/// Perform semantic de-duplication of events within the current batch.
/// Clusters events using cosine similarity (threshold = 0.82) on text embeddings.
pub async fn deduplicate_events(
    config: &Config,
    events: Vec<FilteredEvent>,
) -> Vec<FilteredEvent> {
    if events.len() <= 1 {
        return events;
    }

    tracing::info!(count = events.len(), "Running event semantic de-duplication...");

    // Concatenate title and summary to generate representative text
    let texts: Vec<String> = events
        .iter()
        .map(|e| format!("{} {}", e.title, e.summary))
        .collect();

    // Get real or pseudo embeddings
    let embeddings = crate::vectordb::get_embeddings(config, &texts).await;

    let mut merged: Vec<(FilteredEvent, Vec<f32>)> = Vec::new();
    let threshold = 0.82f32; // Cosine similarity threshold for duplicates

    for (event, emb) in events.into_iter().zip(embeddings) {
        let mut found_dup = false;
        for (existing_event, existing_emb) in merged.iter_mut() {
            // Compute cosine similarity (dot product of normalized vectors)
            let similarity: f32 = emb.iter().zip(existing_emb.iter()).map(|(x, y)| x * y).sum();

            if similarity >= threshold && event.category.eq_ignore_ascii_case(&existing_event.category) {
                // Merge source urls
                for url in event.source_urls.clone() {
                    if !existing_event.source_urls.contains(&url) {
                        existing_event.source_urls.push(url);
                    }
                }
                // Keep the longer summary/title for better information density
                if event.summary.len() > existing_event.summary.len() {
                    existing_event.title = event.title.clone();
                    existing_event.summary = event.summary.clone();
                }
                found_dup = true;
                break;
            }
        }

        if !found_dup {
            merged.push((event, emb));
        }
    }

    let result: Vec<FilteredEvent> = merged.into_iter().map(|(e, _)| e).collect();
    tracing::info!(
        original = texts.len(),
        deduplicated = result.len(),
        "Event semantic de-duplication complete"
    );
    result
}

/// Helper function to perform bulk URL verification.
async fn filter_new_urls(pool: &sqlx::SqlitePool, urls: &[String]) -> std::collections::HashSet<String> {
    if urls.is_empty() {
        return std::collections::HashSet::new();
    }

    let mut new_urls = std::collections::HashSet::new();
    for url in urls {
        new_urls.insert(url.clone());
    }

    for chunk in urls.chunks(500) {
        let mut query_builder = sqlx::QueryBuilder::new("SELECT source_url FROM raw_articles WHERE source_url IN (");
        let mut separated = query_builder.separated(", ");
        for url in chunk {
            separated.push_bind(url);
        }
        separated.push_unseparated(")");

        let query = query_builder.build_query_as::<(String,)>();
        match query.fetch_all(pool).await {
            Ok(rows) => {
                for (existing_url,) in rows {
                    new_urls.remove(&existing_url);
                }
            }
            Err(e) => {
                tracing::error!("Failed to check existing URLs in batch: {}", e);
            }
        }
    }

    new_urls
}

/// Search Qdrant for a duplicate event in history.
/// Returns Option<(historical_id, historical_urls, historical_analysis)>.
async fn find_historical_duplicate(
    qdrant: &qdrant_client::Qdrant,
    collection: &str,
    event: &FilteredEvent,
    config: &Config,
) -> Option<(String, Vec<String>, String)> {
    let query_text = format!("{} {}", event.title, event.summary);
    
    let results = crate::vectordb::search_similar(
        qdrant,
        collection,
        &query_text,
        1,
        Some("analyzed_event".to_string()),
        None,
        None,
        config,
    )
    .await
    .ok()?;

    if let Some(best) = results.first() {
        let score = best.get("score")?.as_f64()? as f32;
        if score >= 0.85 {
            let id_raw = best.get("id")?.as_str()?;
            // Qdrant id might be formatted as String or UUID. Trim quotes if it's formatted as debug representation.
            let id = id_raw.trim_matches('"').trim_matches('\\').to_string();

            let analysis = best
                .get("analysis")
                .and_then(|v| v.as_str())
                .unwrap_or("已在之前的简报中分析完毕。")
                .to_string();

            let source_urls_val = best.get("source_urls").cloned().unwrap_or(serde_json::Value::Null);
            let mut source_urls = Vec::new();
            if let serde_json::Value::Array(arr) = source_urls_val {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        source_urls.push(s.to_string());
                    }
                }
            } else if let serde_json::Value::String(s) = source_urls_val {
                if let Ok(arr) = serde_json::from_str::<Vec<String>>(&s) {
                    source_urls = arr;
                } else {
                    source_urls.push(s);
                }
            }

            return Some((id, source_urls, analysis));
        }
    }
    None
}

/// Dynamic prompt helper.
/// Fetches the base system prompt and evolved guidelines for the specified role.
pub async fn get_agent_prompt(pool: &sqlx::SqlitePool, role_id: &str, default_prompt: &str) -> String {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT system_prompt, guidelines FROM agent_playbook WHERE role_id = ?"
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await
    .unwrap_or_default();

    if let Some((system_prompt, guidelines)) = row {
        if guidelines.is_empty() {
            system_prompt
        } else {
            format!("{}\n\n【动态追加的业务进化守则】：\n{}", system_prompt, guidelines)
        }
    } else {
        default_prompt.to_string()
    }
}

