use anyhow::{Context, Result};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use super::{AnalyzedEvent, DoubaoClient};

#[derive(Debug, Clone, Default)]
pub struct VerifyProgress {
    pub processed_count: usize,
    pub total_count: usize,
    pub output_count: usize,
    pub batch_index: usize,
    pub batch_total: usize,
    pub completed_batches: usize,
    pub failed_batches: usize,
    pub last_error: Option<String>,
    pub message: String,
}

struct VerifyOutcome {
    event_id: String,
    result: Result<AnalyzedEvent, String>,
}

/// Verify analyzed events by sending them through a fact-checking prompt.
/// Adjusts confidence scores based on logical consistency review.
/// Runs batches concurrently (limit: 5 concurrent API requests).
pub async fn verify_events_with_progress<F, Fut>(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    events: &[AnalyzedEvent],
    mut progress: F,
) -> Result<Vec<AnalyzedEvent>>
where
    F: FnMut(VerifyProgress) -> Fut,
    Fut: Future<Output = ()>,
{
    if events.is_empty() {
        return Ok(Vec::new());
    }

    let total_count = events.len();
    progress(VerifyProgress {
        total_count,
        message: format!("Launching collaborative Critic-Refiner loop for {} events", total_count),
        ..VerifyProgress::default()
    })
    .await;

    let semaphore = Arc::new(Semaphore::new(5));
    let (tx, mut rx) = mpsc::unbounded_channel();

    for (i, event) in events.iter().enumerate() {
        let client_clone = client.clone();
        let pool_clone = pool.clone();
        let event_clone = event.clone();
        let sem_clone = semaphore.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let _permit = match sem_clone.acquire_owned().await {
                Ok(permit) => permit,
                Err(e) => {
                    let _ = tx_clone.send(VerifyOutcome {
                        event_id: event_clone.id.clone(),
                        result: Err(e.to_string()),
                    });
                    return;
                }
            };

            let event_id = event_clone.id.clone();
            tracing::info!(event_id = %event_id, index = i + 1, "Starting Critic-Actor collaborative loop");
            let result = process_collaborative_verification(&client_clone, &pool_clone, event_clone).await;
            let outcome = match result {
                Ok(final_event) => VerifyOutcome {
                    event_id: event_id,
                    result: Ok(final_event),
                },
                Err(e) => VerifyOutcome {
                    event_id: event_id,
                    result: Err(e.to_string()),
                },
            };
            let _ = tx_clone.send(outcome);
        });
    }
    drop(tx);

    let mut all_verified = Vec::new();
    let mut processed_count = 0;
    let mut failed_count = 0;
    let mut last_error = None;

    while let Some(outcome) = rx.recv().await {
        processed_count += 1;

        match outcome.result {
            Ok(verified_event) => {
                all_verified.push(verified_event);
            }
            Err(e) => {
                failed_count += 1;
                last_error = Some(e.clone());
                // Fallback: keep original event if verification loop fails completely
                if let Some(orig) = events.iter().find(|ev| ev.id == outcome.event_id) {
                    let mut fallback = orig.clone();
                    fallback.analysis = format!("{}\n[核查失败] 协作核查模块异常：{}", fallback.analysis, e);
                    all_verified.push(fallback);
                }
            }
        }

        progress(VerifyProgress {
            processed_count,
            total_count,
            output_count: all_verified.len(),
            completed_batches: processed_count,
            batch_total: total_count,
            failed_batches: failed_count,
            last_error: last_error.clone(),
            message: format!(
                "Critic-Refiner audit progress: {} / {} events audited, {} finalized",
                processed_count, total_count, all_verified.len()
            ),
            ..VerifyProgress::default()
        })
        .await;
    }

    tracing::info!(
        total = all_verified.len(),
        failed_count,
        "Cooperative verification stage complete"
    );

    Ok(all_verified)
}

async fn process_collaborative_verification(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    event: AnalyzedEvent,
) -> Result<AnalyzedEvent> {
    // 1. Fetch raw article content from SQLite
    let source_url = event.source_urls.first().cloned().unwrap_or_default();
    let raw_content: Option<String> = sqlx::query_scalar(
        "SELECT content FROM raw_articles WHERE source_url = ?"
    )
    .bind(&source_url)
    .fetch_optional(pool)
    .await
    .unwrap_or_default();

    let doc_content = match raw_content {
        Some(content) if !content.trim().is_empty() => content,
        _ => event.summary.clone(),
    };

    // 2. Run Critic Pass 1
    let default_critic_prompt = get_critic_prompt();
    let critic_system_prompt = super::get_agent_prompt(pool, "critic", default_critic_prompt).await;
    let critic_user_prompt = format!(
        "【原始网页正文】:\n{}\n\n【分析特工结论】:\n事件标题: {}\n摘要: {}\n影响类型: {}\n严重程度(severity): {}\n紧急度(urgency): {}\n置信度(confidence): {}\n分析内容: {}",
        doc_content, event.title, event.summary, event.impact_type, event.severity, event.urgency, event.confidence, event.analysis
    );

    let mut approved = true;
    let mut critique_notes = String::new();
    let mut final_confidence = event.confidence;

    let response = client.chat(&critic_system_prompt, &critic_user_prompt, true).await;
    match response {
        Ok(res) => {
            if let Ok(parsed) = parse_critic_response(&res) {
                approved = parsed.approved;
                critique_notes = parsed.critique_notes;
                final_confidence = parsed.confidence_adjustment;
            }
        }
        Err(e) => {
            tracing::warn!("Critic Agent failed to respond: {}, bypassing loop", e);
        }
    }

    let mut final_event = event.clone();
    final_event.confidence = final_confidence;

    // 3. If Critic rejected it, run Refinement and Critic Pass 2
    if !approved && !critique_notes.is_empty() {
        tracing::info!(event_id = %event.id, critique = %critique_notes, "Event analysis rejected by Critic, starting refinement pass...");
        
        let default_refiner_prompt = get_refiner_prompt(&event.category);
        let refiner_prompt_template = super::get_agent_prompt(pool, "refiner", &default_refiner_prompt).await;
        let refiner_system_prompt = refiner_prompt_template.replace("{category}", &event.category);
        let refiner_user_prompt = format!(
            "【原始网页正文】:\n{}\n\n【原分析结论】:\n影响类型: {}\n严重程度(severity): {}\n紧急度(urgency): {}\n置信度(confidence): {}\n分析内容: {}\n\n【监督官批评意见】:\n{}",
            doc_content, event.impact_type, event.severity, event.urgency, event.confidence, event.analysis, critique_notes
        );

        let refiner_response = client.chat(&refiner_system_prompt, &refiner_user_prompt, true).await;
        if let Ok(ref_res) = refiner_response {
            if let Ok(refined) = parse_refined_response(&ref_res) {
                tracing::info!(event_id = %event.id, "Analyst Agent completed refinement successfully");
                final_event.impact_type = refined.impact_type;
                final_event.severity = refined.severity.clamp(1, 5);
                final_event.urgency = refined.urgency.clamp(1, 5);
                final_event.confidence = refined.confidence.clamp(1, 5);
                final_event.analysis = format!("{}\n[核查备注] 已根据事实监督官意见修正：{}", refined.analysis, critique_notes);

                // Run second-pass Critic review
                let critic_user_prompt_v2 = format!(
                    "【原始网页正文】:\n{}\n\n【分析特工结论】:\n事件标题: {}\n摘要: {}\n影响类型: {}\n严重程度(severity): {}\n紧急度(urgency): {}\n置信度(confidence): {}\n分析内容: {}",
                    doc_content, final_event.title, final_event.summary, final_event.impact_type, final_event.severity, final_event.urgency, final_event.confidence, final_event.analysis
                );

                if let Ok(res_v2) = client.chat(&critic_system_prompt, &critic_user_prompt_v2, true).await {
                    if let Ok(parsed_v2) = parse_critic_response(&res_v2) {
                        final_event.confidence = parsed_v2.confidence_adjustment.clamp(1, 5);
                        if !parsed_v2.approved {
                            // If still not approved after refinement, append a final warning
                            final_event.analysis = format!("{}\n[核查警告] 再次核查仍未完全通过：{}", final_event.analysis, parsed_v2.critique_notes);
                        }
                    }
                }
            }
        }
    }

    Ok(final_event)
}

fn get_critic_prompt() -> &'static str {
    r#"你是一个严苛的事实核查特工（角色：【事实与逻辑监督官】）。
你的职责是对比【原始网页正文】与【分析特工的结论】，评估分析是否夸大、偏离事实或打分逻辑不自洽。

评分与审查准则：
- 严禁脑补：分析中提到的数据或竞争策略，必须在【原始网页正文】中能找到事实依据。
- 逻辑评估：严重程度、紧急度打分必须严格符合量化标准。
- 引导修正：如果不合格，请指出具体的事实偏差，说明原因，以便分析特工重新修正。

请以 JSON 格式输出你的核查结论，禁止包含任何 Markdown 格式或多余文字。
格式如下：
{
  "approved": true|false,
  "confidence_adjustment": 1-5,
  "critique_notes": "若 approved 为 false，请写明具体的偏差和修正意见；若为 true，可写明同意理由。"
}
"#
}

fn get_refiner_prompt(category: &str) -> String {
    format!(
        r#"你是一个高级珠宝行业分析师（分类：【{}】）。
你之前做出的分析结论被【事实与逻辑监督官】退回，原因为监督官提出的批评意见。

请在【原始网页正文】事实的基础上，结合监督官的批评意见，重新修正你的分析结论。
修改时：
1. 修正任何夸大、脑补的内容。
2. 根据意见重新调整评分（1-5分）。

请以 JSON 格式输出修正后的分析结论，禁止包含任何 Markdown 格式或多余文字：
{{
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "修正后的详细分析（100字以内）"
}}
"#,
        category
    )
}

#[derive(serde::Deserialize)]
struct CriticResult {
    approved: bool,
    confidence_adjustment: i32,
    critique_notes: String,
}

fn parse_critic_response(response: &str) -> Result<CriticResult> {
    let json_str = extract_json_array_or_object(response);
    let parsed: CriticResult = serde_json::from_str(&json_str)
        .context("Failed to parse Critic response as JSON")?;
    Ok(parsed)
}

#[derive(serde::Deserialize)]
struct RefinedResult {
    impact_type: String,
    severity: i32,
    urgency: i32,
    confidence: i32,
    analysis: String,
}

fn parse_refined_response(response: &str) -> Result<RefinedResult> {
    let json_str = extract_json_array_or_object(response);
    let parsed: RefinedResult = serde_json::from_str(&json_str)
        .context("Failed to parse Refined response as JSON")?;
    Ok(parsed)
}

fn extract_json_array_or_object(text: &str) -> String {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

pub async fn run_critic_pass(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    event: &AnalyzedEvent,
    doc_content: &str,
    peer_comments: Option<&str>,
) -> Result<(bool, String, i32)> {
    let default_critic_prompt = get_critic_prompt();
    let critic_system_prompt = super::get_agent_prompt(pool, "critic", default_critic_prompt).await;

    let peer_context = match peer_comments {
        Some(comments) if !comments.trim().is_empty() => {
            format!("\n【同行评审点评意见】:\n{}", comments)
        }
        _ => "".to_string(),
    };

    let critic_user_prompt = format!(
        "【原始网页正文】:\n{}\n\n【分析特工结论】:\n事件标题: {}\n摘要: {}\n影响类型: {}\n严重程度(severity): {}\n紧急度(urgency): {}\n置信度(confidence): {}\n分析内容: {}{}",
        doc_content, event.title, event.summary, event.impact_type, event.severity, event.urgency, event.confidence, event.analysis, peer_context
    );

    let response = client.chat(&critic_system_prompt, &critic_user_prompt, true).await?;
    let parsed = parse_critic_response(&response)?;
    Ok((parsed.approved, parsed.critique_notes, parsed.confidence_adjustment))
}

pub async fn run_refinement_pass(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    event: &AnalyzedEvent,
    critique_notes: &str,
    doc_content: &str,
) -> Result<AnalyzedEvent> {
    let default_refiner_prompt = get_refiner_prompt(&event.category);
    let refiner_prompt_template = super::get_agent_prompt(pool, "refiner", &default_refiner_prompt).await;
    let refiner_system_prompt = refiner_prompt_template.replace("{category}", &event.category);

    let refiner_user_prompt = format!(
        "【原始网页正文】:\n{}\n\n【原分析结论】:\n影响类型: {}\n严重程度(severity): {}\n紧急度(urgency): {}\n置信度(confidence): {}\n分析内容: {}\n\n【监督官批评意见】:\n{}",
        doc_content, event.impact_type, event.severity, event.urgency, event.confidence, event.analysis, critique_notes
    );

    let response = client.chat(&refiner_system_prompt, &refiner_user_prompt, true).await?;
    let refined = parse_refined_response(&response)?;

    let mut refined_event = event.clone();
    refined_event.impact_type = refined.impact_type;
    refined_event.severity = refined.severity.clamp(1, 5);
    refined_event.urgency = refined.urgency.clamp(1, 5);
    refined_event.confidence = refined.confidence.clamp(1, 5);
    refined_event.analysis = format!("{}\n[核查备注] 已根据事实监督官意见修正：{}", refined.analysis, critique_notes);

    Ok(refined_event)
}

