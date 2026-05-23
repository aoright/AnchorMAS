use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{broadcast, Notify};
use tokio::time::{sleep, Duration};
use anyhow::Result;

use crate::config::Config;
use super::{
    RawDocument, FilteredEvent, AnalyzedEvent, DoubaoClient,
    deduplicate_events, find_historical_duplicate,
    analyst, filter, verifier, evolution,
};

#[derive(Debug, Clone)]
pub enum AgentMessage {
    RawArticleAdded(RawDocument),
    FilteredEventAdded(FilteredEvent),
    AnalysisReady(FilteredEvent),
    AnalysisCompleted(AnalyzedEvent),
    PeerReviewCompleted {
        event: AnalyzedEvent,
        peer_reviewer: String,
        peer_comments: String,
    },
    VerifierVerdict {
        event: AnalyzedEvent,
        approved: bool,
        critique_notes: String,
        confidence_adjustment: i32,
    },
    RefinementCompleted(AnalyzedEvent),
    ConsensusReached(AnalyzedEvent),
    PlaybookUpdated {
        role_id: String,
        new_guidelines: String,
    },
    AllScoutingDone,
}

pub struct Blackboard {
    pub tx: broadcast::Sender<AgentMessage>,
    pub active_count: Arc<AtomicUsize>,
    pub idle_notify: Arc<Notify>,
}

impl Blackboard {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            tx,
            active_count: Arc::new(AtomicUsize::new(0)),
            idle_notify: Arc::new(Notify::new()),
        }
    }

    pub fn increment_work(&self, n: usize) {
        if n == 0 {
            return;
        }
        self.active_count.fetch_add(n, Ordering::SeqCst);
    }

    pub fn decrement_work(&self) {
        let prev = self.active_count.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            self.idle_notify.notify_one();
        }
    }
}

// ─── Agent Task Runners ──────────────────────────────────────────────────────

/// Gatekeeper Agent Task:
/// Listens for RawArticleAdded, buffers them, runs filter_batch concurrently, and emits FilteredEventAdded.
pub async fn start_gatekeeper(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
    config: Arc<Config>,
) {
    let mut buffer = Vec::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));

    loop {
        let sleep_fut = sleep(Duration::from_millis(500));
        tokio::pin!(sleep_fut);

        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(AgentMessage::RawArticleAdded(doc)) => {
                        buffer.push(doc);
                        if buffer.len() >= 10 {
                            let batch = std::mem::take(&mut buffer);
                            let blackboard_clone = blackboard.clone();
                            let client_clone = client.clone();
                            let pool_clone = pool.clone();
                            let config_clone = config.clone();
                            let sem_clone = semaphore.clone();
                            tokio::spawn(async move {
                                let _permit = sem_clone.acquire_owned().await.unwrap();
                                process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                            });
                        }
                    }
                    Ok(AgentMessage::AllScoutingDone) => {
                        if !buffer.is_empty() {
                            let batch = std::mem::take(&mut buffer);
                            let blackboard_clone = blackboard.clone();
                            let client_clone = client.clone();
                            let pool_clone = pool.clone();
                            let config_clone = config.clone();
                            let sem_clone = semaphore.clone();
                            tokio::spawn(async move {
                                let _permit = sem_clone.acquire_owned().await.unwrap();
                                process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                            });
                        }
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::warn!("Gatekeeper task lagged behind broadcast channel");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut sleep_fut, if !buffer.is_empty() => {
                let batch = std::mem::take(&mut buffer);
                let blackboard_clone = blackboard.clone();
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let config_clone = config.clone();
                let sem_clone = semaphore.clone();
                tokio::spawn(async move {
                    let _permit = sem_clone.acquire_owned().await.unwrap();
                    process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                });
            }
        }
    }
    tracing::info!("Gatekeeper task finished scheduling all batches");
}

async fn process_gatekeeper_batch(
    blackboard: &Blackboard,
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    config: &Config,
    batch: Vec<RawDocument>,
) {
    let batch_len = batch.len();
    tracing::info!("Gatekeeper filtering batch of {} raw documents", batch_len);
    match filter::filter_batch(client, pool, &batch).await {
        Ok(events) => {
            tracing::info!("Gatekeeper filtered batch of {}: found {} events", batch_len, events.len());
            // Decrement work count for the raw documents resolved
            for _ in 0..batch_len {
                blackboard.decrement_work();
            }
            // Perform intra-batch event semantic de-duplication
            let events = deduplicate_events(config, events).await;
            // Increment and emit for the filtered events created
            blackboard.increment_work(events.len());
            for event in events {
                let _ = blackboard.tx.send(AgentMessage::FilteredEventAdded(event));
            }
        }
        Err(e) => {
            tracing::error!("Gatekeeper failed to filter batch of {}: {}", batch_len, e);
            for _ in 0..batch_len {
                blackboard.decrement_work();
            }
        }
    }
}

/// De-duplicator Agent Task:
/// Listens for FilteredEventAdded, checks historical duplicates in Qdrant,
/// and either merges them (ConsensusReached) or emits AnalysisReady for unique events.
pub async fn start_deduplicator(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    pool: sqlx::SqlitePool,
    qdrant: Option<qdrant_client::Qdrant>,
    config: Arc<Config>,
) {
    loop {
        match rx.recv().await {
            Ok(AgentMessage::FilteredEventAdded(event)) => {
                let pool_clone = pool.clone();
                let qdrant_clone = qdrant.clone();
                let config_clone = config.clone();
                let blackboard_clone = blackboard.clone();

                tokio::spawn(async move {
                    let mut found_historical = false;

                    if let Some(ref qclient) = qdrant_clone {
                        let collection = &config_clone.qdrant_collection;
                        if let Some((hist_id, mut hist_urls, analysis)) = find_historical_duplicate(qclient, collection, &event, &config_clone).await {
                            tracing::info!(event_id = %event.id, hist_id = %hist_id, "Found historical duplicate event, merging and rolling forward");
                            
                            for url in &event.source_urls {
                                if !hist_urls.contains(url) {
                                    hist_urls.push(url.clone());
                                }
                            }

                            // Fetch current metadata from SQLite
                            let row_opt: Option<(String, String, String, String, String, i64, i64, i64)> = sqlx::query_as(
                                "SELECT market, category, title, summary, impact_type, severity, urgency, confidence FROM events WHERE id = ?"
                            )
                            .bind(&hist_id)
                            .fetch_optional(&pool_clone)
                            .await
                            .unwrap_or_default();

                            if let Some((market, category, title, summary, impact_type, severity, urgency, confidence)) = row_opt {
                                let updated_event = AnalyzedEvent {
                                    id: hist_id,
                                    market,
                                    category,
                                    title,
                                    summary,
                                    source_urls: hist_urls,
                                    impact_type,
                                    severity: severity as i32,
                                    urgency: urgency as i32,
                                    confidence: confidence as i32,
                                    analysis,
                                };
                                let _ = blackboard_clone.tx.send(AgentMessage::ConsensusReached(updated_event));
                                blackboard_clone.decrement_work();
                                found_historical = true;
                            }
                        }
                    }

                    if !found_historical {
                        let _ = blackboard_clone.tx.send(AgentMessage::AnalysisReady(event));
                    }
                });
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
    tracing::info!("De-duplicator task shut down");
}

/// Analyst Agent Coordinator Task:
/// Listens for AnalysisReady, dispatches specialized analysis concurrently.
pub async fn start_analyst_coordinator(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Ok(AgentMessage::AnalysisReady(event)) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    tracing::info!(event_id = %event.id, category = %event.category, "Analyst starting analysis");
                    match analyst::analyze_single_event(&client_clone, &pool_clone, &event).await {
                        Ok(analyzed) => {
                            let _ = blackboard_clone.tx.send(AgentMessage::AnalysisCompleted(analyzed));
                        }
                        Err(e) => {
                            tracing::error!("Analyst failed for event {}: {}, using defaults", event.id, e);
                            let fallback = AnalyzedEvent {
                                id: event.id.clone(),
                                market: event.market.clone(),
                                category: event.category.clone(),
                                title: event.title.clone(),
                                summary: event.summary.clone(),
                                source_urls: event.source_urls.clone(),
                                impact_type: "Attention".to_string(),
                                severity: 2,
                                urgency: 2,
                                confidence: 2,
                                analysis: "Analysis unavailable due to API error.".to_string(),
                            };
                            let _ = blackboard_clone.tx.send(AgentMessage::AnalysisCompleted(fallback));
                        }
                    }
                });
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
    tracing::info!("Analyst coordinator task shut down");
}

/// Peer Analyst Reviewer Task:
/// Listens for AnalysisCompleted, runs cross-domain review, and emits PeerReviewCompleted.
pub async fn start_peer_reviewer(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Ok(AgentMessage::AnalysisCompleted(event)) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    // Map primary category to related peer analyst role
                    let peer_role_id = match event.category.as_str() {
                        "Competition" => "analyst_product",
                        "Product" => "analyst_competition",
                        "Platform" => "analyst_social",
                        "Regulation" => "analyst_competition",
                        _ => "analyst_platform", // Social
                    };

                    tracing::info!(event_id = %event.id, peer = %peer_role_id, "Peer reviewer starting cross-domain review");
                    let review_comments = match analyst::peer_review_event(&client_clone, &pool_clone, &event, peer_role_id).await {
                        Ok(comments) => comments,
                        Err(e) => {
                            tracing::error!("Peer review failed for event {}: {}", event.id, e);
                            "No peer comments available due to review error.".to_string()
                        }
                    };

                    let _ = blackboard_clone.tx.send(AgentMessage::PeerReviewCompleted {
                        event,
                        peer_reviewer: peer_role_id.to_string(),
                        peer_comments: review_comments,
                    });
                });
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
    tracing::info!("Peer reviewer task shut down");
}

/// Critic/Verifier Task:
/// Listens for PeerReviewCompleted and RefinementCompleted. Runs fact checks, adjust confidence, and decides approved/rejected.
pub async fn start_critic(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Ok(AgentMessage::PeerReviewCompleted { event, peer_reviewer: _, peer_comments }) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Critic starting pass 1 fact check");
                    match verifier::run_critic_pass(&client_clone, &pool_clone, &event, &doc_content, Some(&peer_comments)).await {
                        Ok((approved, critique_notes, conf_adj)) => {
                            if approved {
                                tracing::info!(event_id = %event.id, "Critic approved analysis on pass 1");
                                let mut final_event = event.clone();
                                final_event.confidence = conf_adj.clamp(1, 5);
                                let _ = blackboard_clone.tx.send(AgentMessage::ConsensusReached(final_event));
                                blackboard_clone.decrement_work();
                            } else {
                                tracing::info!(event_id = %event.id, critique = %critique_notes, "Critic rejected analysis on pass 1, triggering refinement");
                                
                                // Write feedback to the target domain analyst
                                let target_analyst = format!("analyst_{}", event.category.to_lowercase());
                                log_feedback(&pool_clone, "critic", &target_analyst, Some(&event.id), &critique_notes).await;

                                // If the document content is empty or contains promotional ad spam, log feedback to filter
                                if critique_notes.contains("仅") || critique_notes.contains("缺少正文") || critique_notes.contains("广告") || critique_notes.contains("脑补") {
                                    let filter_feedback = format!(
                                        "标题：{}。该事件被核查判定为信息极度匮乏或属于广告噪音，过滤特工不应放行。请收紧过滤标准。",
                                        event.title
                                    );
                                    log_feedback(&pool_clone, "critic", "filter", Some(&event.id), &filter_feedback).await;
                                }

                                let _ = blackboard_clone.tx.send(AgentMessage::VerifierVerdict {
                                    event,
                                    approved: false,
                                    critique_notes,
                                    confidence_adjustment: conf_adj,
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!("Critic pass 1 failed for event {}: {}, bypassing", event.id, e);
                            let _ = blackboard_clone.tx.send(AgentMessage::ConsensusReached(event));
                            blackboard_clone.decrement_work();
                        }
                    }
                });
            }
            Ok(AgentMessage::RefinementCompleted(event)) => {
                // Critic Pass 2 (final approval/rejection)
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Critic starting pass 2 fact check");
                    match verifier::run_critic_pass(&client_clone, &pool_clone, &event, &doc_content, None).await {
                        Ok((approved, critique_notes, conf_adj)) => {
                            let mut final_event = event.clone();
                            final_event.confidence = conf_adj.clamp(1, 5);
                            if !approved {
                                tracing::warn!(event_id = %event.id, "Critic still rejected analysis on pass 2, proceeding with warnings");
                                final_event.analysis = format!("{}\n[核查警告] 再次核查仍未完全通过：{}", final_event.analysis, critique_notes);
                                
                                // Critic still rejected analyst's output on pass 2. Log feedback to all roles to improve coordination.
                                let target_analyst = format!("analyst_{}", event.category.to_lowercase());
                                log_feedback(&pool_clone, "critic", &target_analyst, Some(&event.id), &format!("二轮核查失败意见: {}", critique_notes)).await;
                                log_feedback(&pool_clone, "refiner", "critic", Some(&event.id), "修正特工已针对首轮意见进行修改，但监督官在二轮给出了不同的核查要求或标准过于严苛。建议监督官在首轮给出完整且一致的意见。").await;
                                log_feedback(&pool_clone, "critic", "refiner", Some(&event.id), &format!("修正特工未能在二轮修改中完全解决首轮提出的核查意见。未通过原因为：{}", critique_notes)).await;
                            } else {
                                tracing::info!(event_id = %event.id, "Critic approved analysis on pass 2");
                            }
                            let _ = blackboard_clone.tx.send(AgentMessage::ConsensusReached(final_event));
                            blackboard_clone.decrement_work();
                        }
                        Err(e) => {
                            tracing::error!("Critic pass 2 failed for event {}: {}, proceeding with original", event.id, e);
                            let _ = blackboard_clone.tx.send(AgentMessage::ConsensusReached(event));
                            blackboard_clone.decrement_work();
                        }
                    }
                });
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
    tracing::info!("Critic task shut down");
}

async fn fetch_raw_content(pool: &sqlx::SqlitePool, event: &AnalyzedEvent) -> String {
    let source_url = event.source_urls.first().cloned().unwrap_or_default();
    let raw_content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM raw_articles WHERE source_url = ?"
    )
    .bind(&source_url)
    .fetch_optional(pool)
    .await
    .unwrap_or_default();

    match raw_content {
        Some(content) if !content.trim().is_empty() => content,
        _ => event.summary.clone(),
    }
}

/// Refiner & Real-time Evolution Agent Task:
/// Listens for VerifierVerdict (approved=false), refines the analysis, runs prompt evolution immediately, and emits RefinementCompleted.
pub async fn start_refiner(
    blackboard: Arc<Blackboard>,
    mut rx: broadcast::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Ok(AgentMessage::VerifierVerdict { event, approved: false, critique_notes, confidence_adjustment: _ }) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Refiner starting refinement pass");
                    match verifier::run_refinement_pass(&client_clone, &pool_clone, &event, &critique_notes, &doc_content).await {
                        Ok(refined_event) => {
                            tracing::info!(event_id = %event.id, "Refinement complete. Triggering real-time evolution.");
                            
                            // Real-time Evolution: Run prompt evolution agent on feedback log immediately
                            match evolution::evolve_from_feedback_log(&pool_clone, &client_clone).await {
                                Ok(updates) => {
                                    for update in updates {
                                        tracing::info!(role = %update.target_role_id, "Real-time playbook evolution succeeded!");
                                        let _ = blackboard_clone.tx.send(AgentMessage::PlaybookUpdated {
                                            role_id: update.target_role_id,
                                            new_guidelines: update.new_guidelines,
                                        });
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Real-time playbook evolution run failed: {}", e);
                                }
                            }

                            let _ = blackboard_clone.tx.send(AgentMessage::RefinementCompleted(refined_event));
                        }
                        Err(e) => {
                            tracing::error!("Refinement failed for event {}: {}, returning original", event.id, e);
                            let mut fallback = event.clone();
                            fallback.analysis = format!("{}\n[核查失败] 修正特工异常：{}", fallback.analysis, e);
                            let _ = blackboard_clone.tx.send(AgentMessage::RefinementCompleted(fallback));
                        }
                    }
                });
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
    tracing::info!("Refiner task shut down");
}

pub async fn log_feedback(
    pool: &sqlx::SqlitePool,
    sender: &str,
    receiver: &str,
    event_id: Option<&str>,
    feedback: &str,
) {
    let id = uuid::Uuid::new_v4().to_string();
    let _ = sqlx::query(
        "INSERT INTO agent_feedback_log (id, sender, receiver, event_id, feedback) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(sender)
    .bind(receiver)
    .bind(event_id)
    .bind(feedback)
    .execute(pool)
    .await;
}
