use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, Notify};
use tokio::time::{sleep, Duration};

use crate::config::Config;
use super::{
    RawDocument, FilteredEvent, AnalyzedEvent, DoubaoClient,
    deduplicate_events, find_historical_duplicate,
    analyst, filter, verifier, evolution,
};

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    pub active_count: Arc<AtomicUsize>,
    pub idle_notify: Arc<Notify>,
    pub llm_semaphore: Arc<tokio::sync::Semaphore>, // Shared LLM concurrency limit
    pub processing_events: Arc<tokio::sync::Mutex<Vec<(FilteredEvent, Vec<f32>)>>>, // Active events currently in pipeline
    pub merged_source_urls: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>, // Merged URLs mapping
    
    // MPSC Senders
    #[allow(dead_code)]
    pub gatekeeper_tx: mpsc::Sender<AgentMessage>,
    pub dedup_tx: mpsc::Sender<AgentMessage>,
    pub analyst_tx: mpsc::Sender<AgentMessage>,
    pub peer_tx: mpsc::Sender<AgentMessage>,
    pub critic_tx: mpsc::Sender<AgentMessage>,
    pub refiner_tx: mpsc::Sender<AgentMessage>,
    pub progress_tx: mpsc::Sender<AgentMessage>,
}

impl Blackboard {
    pub fn new(
        gatekeeper_tx: mpsc::Sender<AgentMessage>,
        dedup_tx: mpsc::Sender<AgentMessage>,
        analyst_tx: mpsc::Sender<AgentMessage>,
        peer_tx: mpsc::Sender<AgentMessage>,
        critic_tx: mpsc::Sender<AgentMessage>,
        refiner_tx: mpsc::Sender<AgentMessage>,
        progress_tx: mpsc::Sender<AgentMessage>,
    ) -> Self {
        Self {
            active_count: Arc::new(AtomicUsize::new(0)),
            idle_notify: Arc::new(Notify::new()),
            llm_semaphore: Arc::new(tokio::sync::Semaphore::new(5)), // Safe 5 concurrent LLM calls
            processing_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            merged_source_urls: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            gatekeeper_tx,
            dedup_tx,
            analyst_tx,
            peer_tx,
            critic_tx,
            refiner_tx,
            progress_tx,
        }
    }

    pub async fn register_event(&self, event: FilteredEvent, embedding: Vec<f32>) {
        let mut processing = self.processing_events.lock().await;
        processing.push((event.clone(), embedding));

        let mut urls = self.merged_source_urls.lock().await;
        urls.insert(event.id, event.source_urls);
    }

    pub async fn check_and_merge_duplicate(&self, event: &FilteredEvent, embedding: &[f32]) -> Option<String> {
        let mut processing = self.processing_events.lock().await;
        let threshold = 0.82f32;

        for (existing_event, existing_emb) in processing.iter_mut() {
            let similarity: f32 = embedding.iter().zip(existing_emb.iter()).map(|(x, y)| x * y).sum();
            if similarity >= threshold && event.category.eq_ignore_ascii_case(&existing_event.category) {
                // Found a concurrent duplicate! Merge URLs.
                let mut urls = self.merged_source_urls.lock().await;
                if let Some(existing_urls) = urls.get_mut(&existing_event.id) {
                    for url in &event.source_urls {
                        if !existing_urls.contains(url) {
                            existing_urls.push(url.clone());
                        }
                    }
                }
                return Some(existing_event.id.clone());
            }
        }
        None
    }

    pub async fn get_merged_urls(&self, event_id: &str) -> Option<Vec<String>> {
        let urls = self.merged_source_urls.lock().await;
        urls.get(event_id).cloned()
    }

    pub async fn remove_event(&self, event_id: &str) {
        let mut processing = self.processing_events.lock().await;
        processing.retain(|(e, _)| e.id != event_id);
    }

    pub fn increment_work(&self, n: usize) {
        if n == 0 {
            return;
        }
        self.active_count.fetch_add(n, Ordering::SeqCst);
    }

    pub fn decrement_work(&self) {
        loop {
            let current = self.active_count.load(Ordering::SeqCst);
            if current == 0 {
                tracing::error!("BUG: decrement_work called when active_count is already 0, ignoring");
                return;
            }
            match self.active_count.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(prev) => {
                    if prev == 1 {
                        self.idle_notify.notify_one();
                    }
                    return;
                }
                Err(_) => continue, // CAS failed, retry
            }
        }
    }
}

// ─── Agent Task Runners ──────────────────────────────────────────────────────

/// Gatekeeper Agent Task:
/// Listens for RawArticleAdded, buffers them, runs filter_batch concurrently, and emits FilteredEventAdded.
pub async fn start_gatekeeper(
    blackboard: Arc<Blackboard>,
    mut rx: mpsc::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
    config: Arc<Config>,
) {
    let mut buffer = Vec::new();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
    let mut spawn_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    loop {
        let sleep_fut = sleep(Duration::from_millis(500));
        tokio::pin!(sleep_fut);

        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(AgentMessage::RawArticleAdded(doc)) => {
                        buffer.push(doc);
                        if buffer.len() >= 10 {
                            let batch = std::mem::take(&mut buffer);
                            let blackboard_clone = blackboard.clone();
                            let client_clone = client.clone();
                            let pool_clone = pool.clone();
                            let config_clone = config.clone();
                            let sem_clone = semaphore.clone();
                            let handle = tokio::spawn(async move {
                                let _permit = sem_clone.acquire_owned().await.unwrap();
                                process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                            });
                            spawn_handles.push(handle);
                        }
                    }
                    Some(AgentMessage::AllScoutingDone) => {
                        if !buffer.is_empty() {
                            let batch = std::mem::take(&mut buffer);
                            let blackboard_clone = blackboard.clone();
                            let client_clone = client.clone();
                            let pool_clone = pool.clone();
                            let config_clone = config.clone();
                            let sem_clone = semaphore.clone();
                            let handle = tokio::spawn(async move {
                                let _permit = sem_clone.acquire_owned().await.unwrap();
                                process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                            });
                            spawn_handles.push(handle);
                        }
                        break;
                    }
                    None => {
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
                let handle = tokio::spawn(async move {
                    let _permit = sem_clone.acquire_owned().await.unwrap();
                    process_gatekeeper_batch(&blackboard_clone, &client_clone, &pool_clone, &config_clone, batch).await;
                });
                spawn_handles.push(handle);
            }
        }
    }
    // Await all in-flight batch tasks to ensure their active_count decrements are completed
    tracing::info!(in_flight = spawn_handles.len(), "Gatekeeper waiting for in-flight batch tasks to complete");
    for handle in spawn_handles {
        let _ = handle.await;
    }
    tracing::info!("Gatekeeper task finished, all batches completed");
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
            let _ = super::parliament::log_task_outcome(pool, "filter", true).await;
            tracing::info!("Gatekeeper filtered batch of {}: found {} events", batch_len, events.len());
            // Decrement work count for the raw documents resolved
            for doc in &batch {
                let _ = blackboard.progress_tx.send(AgentMessage::RawArticleAdded(doc.clone())).await;
                blackboard.decrement_work();
            }
            // Perform intra-batch event semantic de-duplication
            let events = deduplicate_events(config, events).await;
            // Increment and emit for the filtered events created
            blackboard.increment_work(events.len());
            for event in events {
                let _ = blackboard.dedup_tx.send(AgentMessage::FilteredEventAdded(event.clone())).await;
                let _ = blackboard.progress_tx.send(AgentMessage::FilteredEventAdded(event)).await;
            }
        }
        Err(e) => {
            let _ = super::parliament::log_task_outcome(pool, "filter", false).await;
            tracing::error!("Gatekeeper failed to filter batch of {}: {}", batch_len, e);
            for doc in &batch {
                let _ = blackboard.progress_tx.send(AgentMessage::RawArticleAdded(doc.clone())).await;
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
    mut rx: mpsc::Receiver<AgentMessage>,
    pool: sqlx::SqlitePool,
    qdrant: Option<qdrant_client::Qdrant>,
    config: Arc<Config>,
) {
    loop {
        match rx.recv().await {
            Some(AgentMessage::FilteredEventAdded(event)) => {
                let pool_clone = pool.clone();
                let qdrant_clone = qdrant.clone();
                let config_clone = config.clone();
                let blackboard_clone = blackboard.clone();

                tokio::spawn(async move {
                    let text = format!("{} {}", event.title, event.summary);
                    // Acquire LLM permit for embedding calculation
                    let embedding = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        crate::vectordb::get_embeddings(&config_clone, &[text]).await.pop().unwrap_or_else(|| crate::vectordb::pseudo_embedding(&event.title))
                    };

                    // Check concurrent deduplication
                    if let Some(target_id) = blackboard_clone.check_and_merge_duplicate(&event, &embedding).await {
                        tracing::info!(event_id = %event.id, target_id = %target_id, "Found concurrent duplicate event, merged URLs and discarding");
                        blackboard_clone.decrement_work();
                        return;
                    }

                    // Register this event as active in-flight processing
                    blackboard_clone.register_event(event.clone(), embedding).await;

                    let mut found_historical = false;

                    if let Some(ref qclient) = qdrant_clone {
                        let collection = &config_clone.qdrant_collection;
                        if let Some((hist_id, mut hist_urls, analysis)) = find_historical_duplicate(qclient, collection, &event, &config_clone).await {
                            tracing::info!(event_id = %event.id, hist_id = %hist_id, "Found historical duplicate event, merging and rolling forward");
                            
                            // Merge URLs from memory cache if any concurrent duplicates were merged
                            let merged_urls = blackboard_clone.get_merged_urls(&event.id).await.unwrap_or_else(|| event.source_urls.clone());
                            for url in &merged_urls {
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
                                    id: hist_id.clone(),
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
                                let _ = blackboard_clone.progress_tx.send(AgentMessage::ConsensusReached(updated_event)).await;
                                blackboard_clone.remove_event(&event.id).await;
                                blackboard_clone.decrement_work();
                                found_historical = true;
                            }
                        }
                    }

                    if !found_historical {
                        let _ = blackboard_clone.analyst_tx.send(AgentMessage::AnalysisReady(event)).await;
                    }
                });
            }
            None => break,
            _ => {}
        }
    }
    tracing::info!("De-duplicator task shut down");
}

/// Analyst Agent Coordinator Task:
/// Listens for AnalysisReady, dispatches specialized analysis concurrently.
pub async fn start_analyst_coordinator(
    blackboard: Arc<Blackboard>,
    mut rx: mpsc::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Some(AgentMessage::AnalysisReady(event)) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    tracing::info!(event_id = %event.id, category = %event.category, "Analyst starting analysis");
                    let analysis_result = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        analyst::analyze_single_event(&client_clone, &pool_clone, &event).await
                    };
                    match analysis_result {
                        Ok(analyzed) => {
                            let _ = blackboard_clone.peer_tx.send(AgentMessage::AnalysisCompleted(analyzed.clone())).await;
                            let _ = blackboard_clone.progress_tx.send(AgentMessage::AnalysisCompleted(analyzed)).await;
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
                            let _ = blackboard_clone.peer_tx.send(AgentMessage::AnalysisCompleted(fallback.clone())).await;
                            let _ = blackboard_clone.progress_tx.send(AgentMessage::AnalysisCompleted(fallback)).await;
                        }
                    }
                });
            }
            None => break,
            _ => {}
        }
    }
    tracing::info!("Analyst coordinator task shut down");
}

pub async fn select_peer_reviewer(
    pool: &sqlx::SqlitePool,
    category: &str,
    event_id: &str,
) -> String {
    // 1. Fetch all active analyst role_ids from agent_playbook
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT role_id FROM agent_playbook WHERE role_id LIKE 'analyst_%'"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if roles.is_empty() {
        return "analyst_platform".to_string();
    }

    let target_role_id = format!("analyst_{}", category.to_lowercase());

    // 2. Identify custom roles vs core roles
    let core_roles = vec![
        "analyst_competition",
        "analyst_product",
        "analyst_platform",
        "analyst_regulation",
        "analyst_social",
    ];

    let custom_roles: Vec<&str> = roles
        .iter()
        .map(|s| s.as_str())
        .filter(|r| !core_roles.contains(r) && *r != target_role_id)
        .collect();

    // 3. Selection logic:
    // We want to preserve standard peer reviews for core categories to maintain baseline stability,
    // but introduce a chance (e.g. 40%) to route them to a custom analyst if one exists.
    // This allows custom analysts to cross-review and inject specialized knowledge.
    let use_custom = if !custom_roles.is_empty() {
        // Use a deterministic seed from event_id so that retries/reruns are stable
        let mut sum = 0u32;
        for byte in event_id.bytes() {
            sum = sum.wrapping_add(byte as u32);
        }
        (sum % 10) < 4 // 40% chance
    } else {
        false
    };

    if use_custom && !custom_roles.is_empty() {
        let mut sum = 0u32;
        for byte in event_id.bytes() {
            sum = sum.wrapping_add(byte as u32);
        }
        let index = (sum as usize) % custom_roles.len();
        return custom_roles[index].to_string();
    }

    // Default static mapping for core categories
    let default_peer = match category {
        "Competition" => "analyst_product",
        "Product" => "analyst_competition",
        "Platform" => "analyst_social",
        "Regulation" => "analyst_competition",
        "Social" => "analyst_platform",
        _ => {
            // For custom categories, let them be reviewed by a core analyst or another custom analyst
            // We can pick a core analyst based on category name hashing
            let mut sum = 0u32;
            for byte in category.bytes() {
                sum = sum.wrapping_add(byte as u32);
            }
            let core_idx = (sum as usize) % core_roles.len();
            core_roles[core_idx]
        }
    };

    // Ensure the default peer actually exists in the roles
    if roles.contains(&default_peer.to_string()) {
        default_peer.to_string()
    } else {
        // Fallback to any core role that exists, or the first role in the list
        core_roles.iter()
            .find(|r| roles.contains(&r.to_string()))
            .map(|r| r.to_string())
            .unwrap_or_else(|| roles[0].clone())
    }
}

/// Peer Analyst Reviewer Task:
/// Listens for AnalysisCompleted, runs cross-domain review, and emits PeerReviewCompleted.
pub async fn start_peer_reviewer(
    blackboard: Arc<Blackboard>,
    mut rx: mpsc::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Some(AgentMessage::AnalysisCompleted(event)) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    // Map primary category to related peer analyst role dynamically
                    let peer_role_id = select_peer_reviewer(&pool_clone, &event.category, &event.id).await;

                    tracing::info!(event_id = %event.id, peer = %peer_role_id, "Peer reviewer starting cross-domain review");
                    let review_comments = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        analyst::peer_review_event(&client_clone, &pool_clone, &event, &peer_role_id).await
                    };
                    let review_comments = match review_comments {
                        Ok(comments) => comments,
                        Err(e) => {
                            tracing::error!("Peer review failed for event {}: {}", event.id, e);
                            "No peer comments available due to review error.".to_string()
                        }
                    };

                    let _ = blackboard_clone.critic_tx.send(AgentMessage::PeerReviewCompleted {
                        event,
                        peer_reviewer: peer_role_id,
                        peer_comments: review_comments,
                    }).await;
                });
            }
            None => break,
            _ => {}
        }
    }
    tracing::info!("Peer reviewer task shut down");
}

/// Critic/Verifier Task:
/// Listens for PeerReviewCompleted and RefinementCompleted. Runs fact checks, adjust confidence, and decides approved/rejected.
pub async fn start_critic(
    blackboard: Arc<Blackboard>,
    mut rx: mpsc::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Some(AgentMessage::PeerReviewCompleted { event, peer_reviewer, peer_comments }) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                let peer_reviewer_clone = peer_reviewer.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Critic starting pass 1 fact check");
                    let pass_result = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        verifier::run_critic_pass(&client_clone, &pool_clone, &event, &doc_content, Some(&peer_comments)).await
                    };
                    match pass_result {
                        Ok((approved, critique_notes, conf_adj)) => {
                            let target_analyst = format!("analyst_{}", event.category.to_lowercase());
                            if approved {
                                tracing::info!(event_id = %event.id, "Critic approved analysis on pass 1");
                                let _ = super::parliament::log_task_outcome(&pool_clone, &target_analyst, true).await;
                                let _ = super::parliament::log_task_outcome(&pool_clone, &peer_reviewer_clone, true).await;
                                let _ = super::parliament::log_task_outcome(&pool_clone, "filter", true).await;

                                let mut final_event = event.clone();
                                final_event.confidence = conf_adj.clamp(1, 5);
                                if let Some(urls) = blackboard_clone.get_merged_urls(&event.id).await {
                                    final_event.source_urls = urls;
                                }
                                let _ = blackboard_clone.progress_tx.send(AgentMessage::ConsensusReached(final_event)).await;
                                blackboard_clone.remove_event(&event.id).await;
                                blackboard_clone.decrement_work();
                            } else {
                                tracing::info!(event_id = %event.id, critique = %critique_notes, "Critic rejected analysis on pass 1, triggering refinement");
                                let _ = super::parliament::log_task_outcome(&pool_clone, &target_analyst, false).await;
                                let _ = super::parliament::log_task_outcome(&pool_clone, &peer_reviewer_clone, false).await;

                                // Write feedback to the target domain analyst
                                log_feedback(&pool_clone, "critic", &target_analyst, Some(&event.id), &critique_notes).await;

                                // Write feedback to the peer reviewer to trigger their co-evolution
                                let peer_feedback = format!(
                                    "作为同行评审，在此事件的核查中被判定评审不合格。主分析特工做出了有事实偏差或逻辑矛盾的判断，但你作为同行评审未能指出这些偏差。监督官的核查不通过意见为：{}",
                                    critique_notes
                                );
                                log_feedback(&pool_clone, "critic", &peer_reviewer_clone, Some(&event.id), &peer_feedback).await;

                                // If the document content is empty or contains promotional ad spam, log feedback to filter
                                if critique_notes.contains("仅") || critique_notes.contains("缺少正文") || critique_notes.contains("广告") || critique_notes.contains("脑补") {
                                    let _ = super::parliament::log_task_outcome(&pool_clone, "filter", false).await;
                                    let filter_feedback = format!(
                                        "标题：{}。该事件被核查判定为信息极度匮乏或属于广告噪音，过滤特工不应放行。请收紧过滤标准。",
                                        event.title
                                    );
                                    log_feedback(&pool_clone, "critic", "filter", Some(&event.id), &filter_feedback).await;
                                } else {
                                    let _ = super::parliament::log_task_outcome(&pool_clone, "filter", true).await;
                                }

                                let _ = blackboard_clone.refiner_tx.send(AgentMessage::VerifierVerdict {
                                    event,
                                    approved: false,
                                    critique_notes,
                                    confidence_adjustment: conf_adj,
                                }).await;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Critic pass 1 failed for event {}: {}, bypassing", event.id, e);
                            let mut final_event = event.clone();
                            if let Some(urls) = blackboard_clone.get_merged_urls(&event.id).await {
                                  final_event.source_urls = urls;
                            }
                            let _ = blackboard_clone.progress_tx.send(AgentMessage::ConsensusReached(final_event)).await;
                            blackboard_clone.remove_event(&event.id).await;
                            blackboard_clone.decrement_work();
                        }
                    }
                });
            }
            Some(AgentMessage::RefinementCompleted(event)) => {
                // Critic Pass 2 (final approval/rejection)
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Critic starting pass 2 fact check");
                    let pass_result = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        verifier::run_critic_pass(&client_clone, &pool_clone, &event, &doc_content, None).await
                    };
                    match pass_result {
                        Ok((approved, critique_notes, conf_adj)) => {
                            let mut final_event = event.clone();
                            final_event.confidence = conf_adj.clamp(1, 5);
                            let target_analyst = format!("analyst_{}", event.category.to_lowercase());
                            if !approved {
                                tracing::warn!(event_id = %event.id, "Critic still rejected analysis on pass 2, proceeding with warnings");
                                final_event.analysis = format!("{}\n[核查警告] 再次核查仍未完全通过：{}", final_event.analysis, critique_notes);
                                
                                let _ = super::parliament::log_task_outcome(&pool_clone, &target_analyst, false).await;
                                let _ = super::parliament::log_task_outcome(&pool_clone, "refiner", false).await;

                                // Critic still rejected analyst's output on pass 2. Log feedback to all roles to improve coordination.
                                log_feedback(&pool_clone, "critic", &target_analyst, Some(&event.id), &format!("二轮核查失败意见: {}", critique_notes)).await;
                                log_feedback(&pool_clone, "refiner", "critic", Some(&event.id), "修正特工已针对首轮意见进行修改，但监督官在二轮给出了不同的核查要求或标准过于严苛。建议监督官在首轮给出完整且一致的意见。").await;
                                log_feedback(&pool_clone, "critic", "refiner", Some(&event.id), &format!("修正特工未能在二轮修改中完全解决首轮提出的核查意见。未通过原因为：{}", critique_notes)).await;
                            } else {
                                tracing::info!(event_id = %event.id, "Critic approved analysis on pass 2");
                                let _ = super::parliament::log_task_outcome(&pool_clone, &target_analyst, true).await;
                                let _ = super::parliament::log_task_outcome(&pool_clone, "refiner", true).await;
                            }
                            if let Some(urls) = blackboard_clone.get_merged_urls(&event.id).await {
                                final_event.source_urls = urls;
                            }
                            let _ = blackboard_clone.progress_tx.send(AgentMessage::ConsensusReached(final_event)).await;
                            blackboard_clone.remove_event(&event.id).await;
                            blackboard_clone.decrement_work();
                        }
                        Err(e) => {
                            tracing::error!("Critic pass 2 failed for event {}: {}, proceeding with original", event.id, e);
                            let mut final_event = event.clone();
                            if let Some(urls) = blackboard_clone.get_merged_urls(&event.id).await {
                                final_event.source_urls = urls;
                            }
                            let _ = blackboard_clone.progress_tx.send(AgentMessage::ConsensusReached(final_event)).await;
                            blackboard_clone.remove_event(&event.id).await;
                            blackboard_clone.decrement_work();
                        }
                    }
                });
            }
            None => break,
            _ => {}
        }
    }
    tracing::info!("Critic task shut down");
}

async fn fetch_raw_content(pool: &sqlx::SqlitePool, event: &AnalyzedEvent) -> String {
    let source_url = event.source_urls.first().cloned().unwrap_or_default();
    let raw_content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM raw_articles WHERE source_url = ? OR resolved_url = ?"
    )
    .bind(&source_url)
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
    mut rx: mpsc::Receiver<AgentMessage>,
    client: DoubaoClient,
    pool: sqlx::SqlitePool,
) {
    loop {
        match rx.recv().await {
            Some(AgentMessage::VerifierVerdict { event, approved: false, critique_notes, confidence_adjustment: _ }) => {
                let client_clone = client.clone();
                let pool_clone = pool.clone();
                let blackboard_clone = blackboard.clone();
                tokio::spawn(async move {
                    let doc_content = fetch_raw_content(&pool_clone, &event).await;
                    tracing::info!(event_id = %event.id, "Refiner starting refinement pass");
                    let refined_result = {
                        let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                        verifier::run_refinement_pass(&client_clone, &pool_clone, &event, &critique_notes, &doc_content).await
                    };
                    match refined_result {
                        Ok(refined_event) => {
                            tracing::info!(event_id = %event.id, "Refinement complete. Triggering real-time evolution.");
                            
                            // Real-time Evolution: Run prompt evolution agent on feedback log immediately
                            let evolution_result = {
                                let _permit = blackboard_clone.llm_semaphore.acquire().await.unwrap();
                                evolution::evolve_from_feedback_log(&pool_clone, &client_clone).await
                            };
                            match evolution_result {
                                Ok(updates) => {
                                    for update in updates {
                                        tracing::info!(role = %update.target_role_id, "Real-time playbook evolution succeeded!");
                                        let _ = blackboard_clone.progress_tx.send(AgentMessage::PlaybookUpdated {
                                            role_id: update.target_role_id,
                                            new_guidelines: update.new_guidelines,
                                        }).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Real-time playbook evolution run failed: {}", e);
                                }
                            }

                            let _ = blackboard_clone.critic_tx.send(AgentMessage::RefinementCompleted(refined_event)).await;
                        }
                        Err(e) => {
                            let _ = super::parliament::log_task_outcome(&pool_clone, "refiner", false).await;
                            tracing::error!("Refinement failed for event {}: {}, returning original", event.id, e);
                            let mut fallback = event.clone();
                            fallback.analysis = format!("{}\n[核查失败] 修正特工异常：{}", fallback.analysis, e);
                            let _ = blackboard_clone.critic_tx.send(AgentMessage::RefinementCompleted(fallback)).await;
                        }
                    }
                });
            }
            None => break,
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
