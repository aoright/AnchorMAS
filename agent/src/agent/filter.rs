use anyhow::{Context, Result};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::{DoubaoClient, FilteredEvent, RawDocument};

#[derive(Debug, Clone, Default)]
pub struct FilterProgress {
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

/// Filter and classify raw documents using Doubao LLM.
/// Batches documents (up to 10 at a time) for efficient API usage.
/// Runs batches concurrently (limit: 5 concurrent API requests).
pub async fn filter_events_with_progress<F, Fut>(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    raw_docs: &[RawDocument],
    mut progress: F,
) -> Result<Vec<FilteredEvent>>
where
    F: FnMut(FilterProgress) -> Fut,
    Fut: Future<Output = ()>,
{
    if raw_docs.is_empty() {
        return Ok(Vec::new());
    }

    let batch_total = raw_docs.chunks(10).count();
    progress(FilterProgress {
        total_count: raw_docs.len(),
        batch_total,
        message: format!("Filtering {} articles in {} batches", raw_docs.len(), batch_total),
        ..FilterProgress::default()
    })
    .await;

    let semaphore = Arc::new(Semaphore::new(5));
    let (tx, mut rx) = mpsc::unbounded_channel();

    for (i, batch) in raw_docs.chunks(10).enumerate() {
        let client_clone = client.clone();
        let pool_clone = pool.clone();
        let batch_vec = batch.to_vec();
        let sem_clone = semaphore.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let _permit = match sem_clone
                .acquire_owned()
                .await
                .context("Failed to acquire semaphore permit")
            {
                Ok(permit) => permit,
                Err(e) => {
                    let _ = tx_clone.send(FilterBatchOutcome {
                        batch_index: i,
                        input_count: batch_vec.len(),
                        result: Err(e.to_string()),
                    });
                    return;
                }
            };
            tracing::info!("Starting filter batch #{}", i + 1);
            let res = filter_batch(&client_clone, &pool_clone, &batch_vec).await;
            let outcome = match res {
                Ok(events) => {
                    tracing::info!(
                        "Filter batch #{} succeeded, found {} events",
                        i + 1,
                        events.len()
                    );
                    FilterBatchOutcome {
                        batch_index: i,
                        input_count: batch_vec.len(),
                        result: Ok(events),
                    }
                }
                Err(e) => {
                    tracing::error!("Filter batch #{} failed: {}", i + 1, e);
                    FilterBatchOutcome {
                        batch_index: i,
                        input_count: batch_vec.len(),
                        result: Err(e.to_string()),
                    }
                }
            };
            let _ = tx_clone.send(outcome);
        });
    }
    drop(tx);

    let mut all_events = Vec::new();
    let mut completed_batches = 0;
    let mut failed_batches = 0;
    let mut processed_count = 0;
    let mut last_error = None;

    while let Some(outcome) = rx.recv().await {
        completed_batches += 1;
        processed_count += outcome.input_count;

        match outcome.result {
            Ok(mut events) => {
                all_events.append(&mut events);
            }
            Err(e) => {
                failed_batches += 1;
                last_error = Some(e);
            }
        }

        let output_count = all_events.len();
        progress(FilterProgress {
            processed_count,
            total_count: raw_docs.len(),
            output_count,
            batch_index: outcome.batch_index + 1,
            batch_total,
            completed_batches,
            failed_batches,
            last_error: last_error.clone(),
            message: format!(
                "Filter batch {}/{} complete: {} / {} articles processed, {} events generated",
                completed_batches,
                batch_total,
                processed_count,
                raw_docs.len(),
                output_count
            ),
        })
        .await;
    }

    tracing::info!(total = all_events.len(), "Total events after filtering");
    Ok(all_events)
}

struct FilterBatchOutcome {
    batch_index: usize,
    input_count: usize,
    result: std::result::Result<Vec<FilteredEvent>, String>,
}

pub async fn filter_batch(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    batch: &[RawDocument],
) -> Result<Vec<FilteredEvent>> {
    let default_system_prompt = r#"你是珠宝行业情报分类与多语言专家。你的任务是对新闻文章进行分类和筛选，并统一输出语言。

对于每篇文章，请：
1. 判断是否与珠宝行业相关（过滤掉纯广告、无关内容）
2. 进行领域分类：
   - 经典类别：Competition（竞争动态）, Product（产品趋势）, Social（社会舆情）, Platform（平台渠道）, Regulation（法规政策）
   - 专业精细细分领域：若该事件属于经典五类之外的其他高度专精细分领域（如：宏观经济 MacroEconomy, 绿色可持续 ESG, 奢侈品并购 MergerAcquisition, 独立设计与IP联名 DesignIP, 拍卖会与收藏 Auction, 工艺技术与数字化 TechCraft 等），可以输出自定义的驼峰命名英文类别名（仅限单个单词或以拼音/英文组成的驼峰命名，限制在20字符以内，禁止使用中文类别名）。
3. 判定关联市场：China, Japan, Korea, SoutheastAsia, UnitedStates, 或 Global
4. 提取核心摘要。如果原始文章是外语，必须在 JSON 返回中把 "title" 和 "summary" 翻译为中文输出，确保整份简报语言的一致性。

请以JSON数组格式返回结果，每个元素包含：
{
  "title": "中文事件标题（外语请翻译为中文）",
  "summary": "50字以内中文摘要（外语请翻译为中文）",
  "category": "Competition|Product|Social|Platform|Regulation|或者自定义驼峰英文类别",
  "market": "China|Japan|Korea|SoutheastAsia|UnitedStates|Global",
  "source_url": "来源URL"
}

仅返回有价值的事件，过滤掉噪音。如果所有文章都是噪音，返回空数组 []。
只返回JSON数组，不要包含其他文字。"#;

    let system_prompt = super::get_agent_prompt(pool, "filter", default_system_prompt).await;

    let articles_json: Vec<serde_json::Value> = batch
        .iter()
        .map(|doc| {
            serde_json::json!({
                "title": doc.title,
                "content": truncate_content(&doc.content, 500),
                "source_url": doc.source_url,
                "language": doc.raw_language,
            })
        })
        .collect();

    let user_prompt = format!(
        "请分析以下{}篇文章：\n\n{}",
        batch.len(),
        serde_json::to_string_pretty(&articles_json)?
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let events = parse_filtered_events(&response)?;

    Ok(events)
}

fn parse_filtered_events(response: &str) -> Result<Vec<FilteredEvent>> {
    // Try to extract JSON from the response (model might wrap it in markdown)
    let json_str = extract_json_array(response);

    let items: Vec<serde_json::Value> = serde_json::from_str(&json_str)
        .context("Failed to parse filter response as JSON array")?;

    let events = items
        .into_iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?.to_string();
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let category = item.get("category")?.as_str()?.to_string();
            let market = item
                .get("market")
                .and_then(|v| v.as_str())
                .unwrap_or("Global")
                .to_string();
            let source_url = item
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            Some(FilteredEvent {
                id: Uuid::new_v4().to_string(),
                market,
                category,
                title,
                summary,
                source_urls: vec![source_url],
            })
        })
        .collect();

    Ok(events)
}

/// Extract a JSON array from a response that might contain markdown code fences.
fn extract_json_array(text: &str) -> String {
    // Try to find JSON array between code fences
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return text[start..=end].to_string();
        }
    }
    // Fallback: return as-is
    text.to_string()
}

/// Truncate content to a maximum number of characters.
fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        content.to_string()
    } else {
        let truncated: String = content.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}
