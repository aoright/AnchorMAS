use anyhow::{Context, Result};
use qdrant_client::Qdrant;
use sqlx::SqlitePool;
use std::collections::HashSet;
use uuid::Uuid;

use crate::agent::{AnalyzedEvent, DoubaoClient, RawDocument};
use crate::config::Config;
use crate::vectordb;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct EvidenceEvaluatorResponse {
    pub is_linked: bool,
    pub match_score: f64,
    pub relation_type: String,
    pub relation_description: String,
}

/// Compute cosine similarity between two vector embeddings.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a > 0.0 && norm_b > 0.0 {
        dot_product / (norm_a * norm_b)
    } else {
        0.0
    }
}

/// Extract core keywords/entities for the evidence profile of an event using LLM.
pub async fn extract_evidence_profile(
    doubao: &DoubaoClient,
    title: &str,
    summary: &str,
    analysis: &str,
) -> Result<Vec<String>> {
    let system_prompt = "你是一个珠宝行业情报与大数据分析专家。你的任务是分析给定的新闻事件，提取最核心的3-5个特征关键词。这些词应包括：具体的企业/品牌名称（如‘周大福’、‘潘多拉’）、特定的材质（如‘培育钻石’、‘足金’）、以及核心商业动作或事件词（如‘降价’、‘毛利率下滑’、‘关店潮’、‘收购’）。\n\n请直接以 JSON 字符串数组格式返回特征词，禁止包含任何外层包装或 Markdown 标记。示例：[\"周大福\", \"培育钻石\", \"价格战\"]";
    let user_prompt = format!(
        "标题: {}\n摘要: {}\n深度分析: {}",
        title, summary, analysis
    );

    let res = doubao.chat(system_prompt, &user_prompt, true).await?;
    let cleaned_res = res.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let keywords: Vec<String> = serde_json::from_str(cleaned_res)
        .context(format!("Failed to parse keywords JSON: {}", res))?;

    Ok(keywords)
}

/// Crawl Google News RSS feed for given keywords as fallback when local DB is dry.
pub async fn crawl_web_for_keywords(
    client: &reqwest::Client,
    keywords: &[String],
    lang: &str,
) -> Result<Vec<RawDocument>> {
    let rss_url = "https://news.google.com/rss/search";
    
    // We try multiple queries in order of specificity
    let mut queries = Vec::new();
    if keywords.len() > 2 {
        queries.push(keywords[..2].join(" "));
    }
    if !keywords.is_empty() {
        queries.push(keywords[0].clone());
    }
    if keywords.len() > 1 {
        queries.push(keywords[1].clone());
    }

    for query in queries {
        tracing::info!(query = %query, "Attempting web crawl for query");
        let res_attempt = client
            .get(rss_url)
            .query(&[("q", &query), ("hl", &lang.to_string())])
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await;

        if let Ok(res) = res_attempt {
            if let Ok(bytes) = res.bytes().await {
                if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
                    let mut docs = Vec::new();
                    for entry in feed.entries.into_iter().take(8) {
                        let title = entry.title.map(|t| t.content).unwrap_or_else(|| "Untitled".to_string());
                        let link = entry.links.first().map(|l| l.href.clone()).unwrap_or_else(|| "".to_string());
                        let summary_text = entry.summary.map(|s| s.content).unwrap_or_default();
                        let content_text = entry.content.and_then(|c| c.body).unwrap_or_default();
                        let raw_desc = if content_text.len() > summary_text.len() { content_text } else { summary_text };
                        
                        let fragment = scraper::Html::parse_fragment(&raw_desc);
                        let content = fragment
                            .root_element()
                            .text()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_string();

                        if !link.is_empty() {
                            let ts = entry.published
                                .map(|dt| dt.format("%Y-%m-%d").to_string())
                                .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
                            docs.push(RawDocument {
                                source_url: link,
                                title,
                                content,
                                raw_language: lang.to_string(),
                                timestamp: ts,
                                original_url: None,
                            });
                        }
                    }
                    if !docs.is_empty() {
                        tracing::info!(query = %query, count = docs.len(), "Web crawl succeeded");
                        return Ok(docs);
                    }
                }
            }
        }
    }

    Ok(Vec::new())
}

/// Convert a crawled raw document to a structured AnalyzedEvent using LLM.
pub async fn convert_raw_doc_to_event(
    doubao: &DoubaoClient,
    doc: &RawDocument,
) -> Result<AnalyzedEvent> {
    let system_prompt = r#"你是一个珠宝行业的高级情报整理专家。你的任务是分析一篇原始新闻，将其整理结构化为一个高级分析事件。
请将输入新闻提取并翻译（若为外语，务必翻译成中文）为以下 JSON 格式（禁止包含任何外层包装或 markdown 标记）：
{
  "market": "China|Japan|Korea|SoutheastAsia|UnitedStates|Global",
  "category": "Competition|Product|Social|Platform|Regulation",
  "title": "中文事件标题",
  "summary": "50字以内中文概要",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 3,
  "urgency": 3,
  "confidence": 4,
  "analysis": "100字以内详细分析结论"
}"#;

    let user_prompt = format!("标题: {}\n正文: {}", doc.title, doc.content);
    let res = doubao.chat(system_prompt, &user_prompt, true).await?;
    let cleaned_res = res.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(serde::Deserialize)]
    struct TempEvent {
        market: String,
        category: String,
        title: String,
        summary: String,
        impact_type: String,
        severity: i32,
        urgency: i32,
        confidence: i32,
        analysis: String,
    }

    let temp: TempEvent = serde_json::from_str(cleaned_res)
        .context(format!("Failed to parse temp event JSON: {}", res))?;

    let event_id = uuid::Uuid::new_v4().to_string();
    Ok(AnalyzedEvent {
        id: event_id,
        market: temp.market,
        category: temp.category,
        title: temp.title,
        summary: temp.summary,
        source_urls: vec![doc.source_url.clone()],
        impact_type: temp.impact_type,
        severity: temp.severity,
        urgency: temp.urgency,
        confidence: temp.confidence,
        analysis: temp.analysis,
    })
}

/// Save crawled event to SQLite and Qdrant database.
pub async fn save_crawled_event(
    pool: &SqlitePool,
    qdrant: Option<&Qdrant>,
    event: &AnalyzedEvent,
    created_at: Option<String>,
    config: &Config,
) -> Result<()> {
    // 1. Save to SQLite raw_articles
    let article_id = Uuid::new_v4().to_string();
    let _ = sqlx::query!(
        r#"INSERT OR IGNORE INTO raw_articles (id, source_url, title, content, raw_language)
           VALUES (?, ?, ?, ?, ?)"#,
        article_id,
        event.source_urls[0],
        event.title,
        event.summary,
        "zh"
    )
    .execute(pool)
    .await;

    // 2. Save to SQLite events
    let mut created_at_val = created_at.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
    if created_at_val.len() > 10 {
        created_at_val.truncate(10);
    }
    let source_urls_json = serde_json::to_string(&event.source_urls)?;
    sqlx::query!(
        r#"INSERT INTO events
           (id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, briefing_id, analysis, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(id) DO UPDATE SET
             market = excluded.market,
             category = excluded.category,
             title = excluded.title,
             summary = excluded.summary,
             impact_type = excluded.impact_type,
             severity = excluded.severity,
             urgency = excluded.urgency,
             confidence = excluded.confidence,
             source_urls = excluded.source_urls,
             briefing_id = COALESCE(excluded.briefing_id, events.briefing_id),
             analysis = excluded.analysis,
             created_at = excluded.created_at"#,
        event.id,
        event.market,
        event.category,
        event.title,
        event.summary,
        event.impact_type,
        event.severity,
        event.urgency,
        event.confidence,
        source_urls_json,
        None::<String>,
        event.analysis,
        created_at_val
    )
    .execute(pool)
    .await?;

    // 3. Save to Qdrant
    if let Some(client) = qdrant {
        let collection = &config.qdrant_collection;
        let _ = vectordb::store_events(client, collection, &[event.clone()], "", config).await;
    }

    Ok(())
}

/// Retrieve all candidate events from SQLite and Qdrant that could be related to the keywords.
pub async fn search_candidate_events(
    pool: &SqlitePool,
    qdrant: Option<&Qdrant>,
    keywords: &[String],
    original_event_id: &str,
    config: &Config,
) -> Result<Vec<AnalyzedEvent>> {
    let mut candidate_ids = HashSet::new();
    let mut candidates = Vec::new();

    // Expand keywords into shingles (2 and 3 chars for CJK, split by space for English)
    let mut search_terms = HashSet::new();
    for kw in keywords {
        search_terms.insert(kw.clone());
        let chars: Vec<char> = kw.chars().collect();
        if chars.iter().any(|c| !c.is_ascii()) {
            for i in 0..chars.len() {
                if i + 2 <= chars.len() {
                    let term: String = chars[i..i+2].iter().collect();
                    search_terms.insert(term);
                }
                if i + 3 <= chars.len() {
                    let term: String = chars[i..i+3].iter().collect();
                    search_terms.insert(term);
                }
            }
        } else {
            for part in kw.split_whitespace() {
                if part.len() >= 3 {
                    search_terms.insert(part.to_string());
                }
            }
        }
    }
    let search_terms_vec: Vec<String> = search_terms.into_iter().collect();

    // 1. Vector similarity search in Qdrant (Dense Search)
    if let Some(client) = qdrant {
        let collection = &config.qdrant_collection;
        let query_text = keywords.join(" ");
        if let Ok(results) = vectordb::search_similar(
            client,
            collection,
            &query_text,
            25,
            Some("analyzed_event".to_string()),
            None,
            None,
            config,
        )
        .await
        {
            for item in results {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    let clean_id = id.trim_matches('"').to_string();
                    if clean_id != original_event_id {
                        candidate_ids.insert(clean_id);
                    }
                }
            }
        }
    }

    // 2. Keyword matching search in SQLite (Sparse Search) using expanded shingles
    for term in &search_terms_vec {
        let pattern = format!("%{}%", term);
        let rows = sqlx::query!(
            r#"SELECT id FROM events 
               WHERE (title LIKE ? OR summary LIKE ? OR analysis LIKE ?) 
               AND id != ?
               LIMIT 20"#,
            pattern,
            pattern,
            pattern,
            original_event_id
        )
        .fetch_all(pool)
        .await?;

        for row in rows {
            if let Some(id) = row.id {
                candidate_ids.insert(id);
            }
        }
    }

    // 3. Load full events for all unique candidate IDs and strictly filter them
    for id in candidate_ids {
        let event_row = sqlx::query!(
            r#"SELECT id, market, category, title, summary, impact_type, severity, urgency, confidence, source_urls, analysis
               FROM events WHERE id = ?"#,
            id
        )
        .fetch_optional(pool)
        .await?;

        if let Some(row) = event_row {
            let source_urls: Vec<String> = serde_json::from_str(&row.source_urls).unwrap_or_default();
            
            // Check matching keywords in title, summary, or analysis
            let title_lower = row.title.to_lowercase();
            let summary_lower = row.summary.to_lowercase();
            let analysis_lower = row.analysis.to_lowercase();
            
            let mut matched_keywords = Vec::new();
            for kw in &search_terms_vec {
                let kw_lower = kw.to_lowercase();
                if title_lower.contains(&kw_lower)
                    || summary_lower.contains(&kw_lower)
                    || analysis_lower.contains(&kw_lower)
                {
                    matched_keywords.push(kw.clone());
                }
            }

            // Exclude loose brand or general category associations:
            // If the candidate matches only 1 keyword, and that keyword's length is <= 3, skip it.
            // (e.g., if it only matches "周大福" or "黄金", discard).
            // Also discard candidates that match 0 keywords.
            if matched_keywords.len() == 1 && matched_keywords[0].chars().count() <= 3 {
                tracing::info!(
                    title = %row.title,
                    matched_keyword = %matched_keywords[0],
                    "Discarding candidate due to matching only a single short keyword (length <= 3)"
                );
                continue;
            }
            if matched_keywords.is_empty() {
                tracing::info!(
                    title = %row.title,
                    "Discarding candidate due to matching zero keywords"
                );
                continue;
            }

            candidates.push(AnalyzedEvent {
                id: row.id.unwrap_or_default(),
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

    Ok(candidates)
}

/// Evaluates if a candidate event is logically linked to the bookmarked event.
pub async fn evaluate_event_link(
    doubao: &DoubaoClient,
    pool: &SqlitePool,
    bookmark_title: &str,
    bookmark_summary: &str,
    bookmark_analysis: &str,
    bookmark_keywords: &[String],
    bookmark_created_at: &str,
    candidate: &AnalyzedEvent,
    candidate_created_at: &str,
    direction: &str,
) -> Result<Option<EvidenceEvaluatorResponse>> {
    let system_prompt = crate::agent::get_agent_prompt(
        pool,
        "evidence_evaluator",
        "你是一个严谨的行业数据分析专家。判断两个新闻事件是否属于相同的发展脉络。"
    ).await;

    let dir_desc = if direction == "past" {
        "past (向过去溯源：评估候选新闻是否为已收藏新闻的直接因果起因/前置事件)"
    } else {
        "future (向未来追踪：评估候选新闻是否为已收藏新闻的直接因果后续进展/结果)"
    };

    let user_prompt = format!(
        "【追踪方向】: {}\n\n\
         【已收藏新闻】\n\
         - 标题: {}\n\
         - 摘要: {}\n\
         - 分析: {}\n\
         - 特征词: {:?}\n\
         - 发布时间: {}\n\n\
         【待评估候选新闻】\n\
         - 标题: {}\n\
         - 摘要: {}\n\
         - 分析: {}\n\
         - 发布时间: {}",
        dir_desc,
        bookmark_title,
        bookmark_summary,
        bookmark_analysis,
        bookmark_keywords,
        bookmark_created_at,
        candidate.title,
        candidate.summary,
        candidate.analysis,
        candidate_created_at
    );

    let res = doubao.chat(&system_prompt, &user_prompt, true).await?;
    let cleaned_res = res.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let evaluator_res: EvidenceEvaluatorResponse = serde_json::from_str(cleaned_res)
        .context(format!("Failed to parse evaluator response: {}", res))?;

    // --- Validation logic for evidence_evaluator ---
    let mut evaluator_complaints = Vec::new();
    if evaluator_res.is_linked {
        if evaluator_res.relation_description.chars().count() < 10 {
            evaluator_complaints.push("关联原因描述 (relation_description) 过于简短，无法具体说明逻辑传导机制。");
        }
        if evaluator_res.relation_description.contains("`") || evaluator_res.relation_description.contains("json") {
            evaluator_complaints.push("描述中含有 markdown 格式的转义符或 JSON 块残留，须输出纯文本。");
        }
        let rel_type = evaluator_res.relation_type.to_lowercase();
        if rel_type != "cause" && rel_type != "result" && rel_type != "comparison" {
            evaluator_complaints.push("关联类型 (relation_type) 必须在 cause、result 或 comparison 中选择，请勿自行编造新类型。");
        }
    }

    if !evaluator_complaints.is_empty() {
        let feedback_msg = format!(
            "在评估已收藏新闻与待匹配新闻【{}】的关联时，监督机制发现输出不规范：{} 请重新梳理逻辑关联机制，输出更精确的逻辑链评测结果。",
            candidate.title,
            evaluator_complaints.join(" ")
        );
        crate::agent::blackboard::log_feedback(
            pool,
            "validator",
            "evidence_evaluator",
            Some(&candidate.id),
            &feedback_msg
        ).await;
        tracing::warn!("Evidence evaluator validation failed. Logged feedback to evidence_evaluator.");
    }

    if evaluator_res.is_linked {
        Ok(Some(evaluator_res))
    } else {
        Ok(None)
    }
}

/// Run retrospective tracing on a newly bookmarked event (5 levels recursive search + web crawl fallback).
pub async fn run_retrospective_tracing(
    doubao: &DoubaoClient,
    pool: &SqlitePool,
    qdrant: Option<&Qdrant>,
    bookmark_id: &str,
    event_id: &str,
    config: &Config,
) -> Result<usize> {
    // 1. Fetch bookmark details
    let bookmark_event = sqlx::query!(
        r#"SELECT e.title, e.summary, e.analysis, e.created_at, b.keywords
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id
           WHERE b.id = ?"#,
        bookmark_id
    )
    .fetch_one(pool)
    .await?;

    let keywords: Vec<String> = serde_json::from_str(&bookmark_event.keywords).unwrap_or_default();
    let bookmark_created_at = &bookmark_event.created_at;

    // Clear all existing links for this bookmark to prevent dirty runs
    sqlx::query!(
        "DELETE FROM bookmark_evidence_chain WHERE bookmark_id = ?",
        bookmark_id
    )
    .execute(pool)
    .await?;

    // 2. Search local candidates
    let mut candidates = search_candidate_events(pool, qdrant, &keywords, event_id, config).await?;

    // Scan newer candidates that already exist in the DB for 'future' developments
    for c in &candidates {
        if let Ok(c_row) = sqlx::query!("SELECT created_at FROM events WHERE id = ?", c.id)
            .fetch_one(pool)
            .await
        {
            if c_row.created_at > bookmark_event.created_at {
                if let Ok(Some(eval_res)) = evaluate_event_link(
                    doubao,
                    pool,
                    &bookmark_event.title,
                    &bookmark_event.summary,
                    &bookmark_event.analysis,
                    &keywords,
                    &bookmark_event.created_at,
                    c,
                    &c_row.created_at,
                    "future",
                )
                .await
                {
                    // Calculate real similarity
                    let e0_text = format!("{} {}", bookmark_event.title, bookmark_event.summary);
                    let e0_emb = vectordb::get_embeddings(config, &[e0_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);
                    
                    let candidate_text = format!("{} {}", c.title, c.summary);
                    let candidate_emb = vectordb::get_embeddings(config, &[candidate_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);
                    let real_score = cosine_similarity(&e0_emb, &candidate_emb) as f64;

                    if eval_res.match_score >= 0.75 && real_score >= 0.55 {
                        let id = Uuid::new_v4().to_string();
                        let _ = sqlx::query!(
                            r#"INSERT OR REPLACE INTO bookmark_evidence_chain (id, bookmark_id, matched_event_id, direction, match_score, match_reason)
                               VALUES (?, ?, ?, 'future', ?, ?)"#,
                            id,
                            bookmark_id,
                            c.id,
                            real_score,
                            eval_res.relation_description
                        )
                        .execute(pool)
                        .await;
                    }
                }
            }
        }
    }

    // Filter local candidates that are strictly older than the bookmark to see if we need fallback crawl
    let mut older_local_candidates_count = 0;
    for c in &candidates {
        if let Ok(c_row) = sqlx::query!("SELECT created_at FROM events WHERE id = ?", c.id)
            .fetch_one(pool)
            .await
        {
            if c_row.created_at < bookmark_event.created_at {
                older_local_candidates_count += 1;
            }
        }
    }

    // 3. Fallback: If older local candidates are dry (< 3), trigger web crawl to fetch background facts!
    if older_local_candidates_count < 3 {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        
        if let Ok(crawled_docs) = crawl_web_for_keywords(&http_client, &keywords, "zh").await {
            tracing::info!(crawled = crawled_docs.len(), "Web crawl returned articles");
            for doc in crawled_docs {
                // Ensure crawled document timestamp is older than E_0 (the bookmark) before saving
                if doc.timestamp < bookmark_event.created_at {
                    if let Ok(crawled_event) = convert_raw_doc_to_event(doubao, &doc).await {
                        let _ = save_crawled_event(pool, qdrant, &crawled_event, Some(doc.timestamp.clone()), config).await;
                    }
                }
            }
            // Reload candidates list to include the crawled entries
            if let Ok(reloaded) = search_candidate_events(pool, qdrant, &keywords, event_id, config).await {
                candidates = reloaded;
            }
        }
    }

    // 4. Implement 5-Level Recursive Tracing (保留五层，因果向前回溯)
    // We start from E_0 (the bookmark event) and trace backwards step-by-step
    let mut visited = HashSet::new();
    visited.insert(event_id.to_string());

    let mut current_node = AnalyzedEvent {
        id: event_id.to_string(),
        market: "".to_string(),
        category: "".to_string(),
        title: bookmark_event.title.clone(),
        summary: bookmark_event.summary.clone(),
        source_urls: vec![],
        impact_type: "".to_string(),
        severity: 0,
        urgency: 0,
        confidence: 0,
        analysis: bookmark_event.analysis.clone(),
    };

    let mut matched_count = 0;
    
    // Cache the embedding of E_0
    let e0_text = format!("{} {}", bookmark_event.title, bookmark_event.summary);
    let e0_emb = vectordb::get_embeddings(config, &[e0_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);

    for level in 1..=5 {
        tracing::info!(level = level, current_node_id = %current_node.id, "Recursive retrospective tracing step");

        // Fetch candidate's created_at for node E_{k-1}
        let current_node_created_at = if level == 1 {
            bookmark_created_at.clone()
        } else {
            let row = sqlx::query!("SELECT created_at FROM events WHERE id = ?", current_node.id)
                .fetch_one(pool)
                .await?;
            row.created_at
        };

        // Filter candidates that are strictly older than E_{k-1} to avoid loops and preserve chronological direction
        let mut step_candidates = Vec::new();
        for c in &candidates {
            if visited.contains(&c.id) {
                continue;
            }
            
            // Get candidate created_at
            let c_row = sqlx::query!("SELECT created_at FROM events WHERE id = ?", c.id)
                .fetch_one(pool)
                .await?;
            
            if c_row.created_at < current_node_created_at {
                step_candidates.push((c.clone(), c_row.created_at));
            }
        }

        if step_candidates.is_empty() {
            tracing::info!(level = level, "No older candidates found, halting recursive tracking");
            break;
        }

        // Batch calculate embeddings for candidates to rank them by Coherence Score
        let candidate_texts: Vec<String> = step_candidates.iter()
            .map(|(c, _)| format!("{} {}", c.title, c.summary))
            .collect();
        let candidate_embs = vectordb::get_embeddings(config, &candidate_texts).await;

        // Current node E_{k-1} embedding
        let ek_minus_1_text = format!("{} {}", current_node.title, current_node.summary);
        let ek_minus_1_emb = vectordb::get_embeddings(config, &[ek_minus_1_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);

        // Rank candidates using Coherence Score: C_k = 0.6 * cos_sim(E_0, c) + 0.4 * cos_sim(E_{k-1}, c)
        let mut ranked = Vec::new();
        for (idx, (c, c_created_at)) in step_candidates.into_iter().enumerate() {
            if idx >= candidate_embs.len() {
                break;
            }
            let sim_e0 = cosine_similarity(&e0_emb, &candidate_embs[idx]) as f64;
            let sim_ek_minus_1 = cosine_similarity(&ek_minus_1_emb, &candidate_embs[idx]) as f64;
            
            // Coherence Score Formula to prevent semantic drift
            let coherence_score = 0.6 * sim_e0 + 0.4 * sim_ek_minus_1;
            ranked.push((c, coherence_score, c_created_at));
        }

        // Sort descending by coherence score
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Threshold validation: if highest similarity is below 0.55, semantic drift is too high (追溯失真), stop tracking.
        if ranked.is_empty() || ranked[0].1 < 0.55 {
            tracing::info!(
                level = level,
                max_score = ?ranked.first().map(|r| r.1),
                "Coherence score dropped below threshold (0.55), halting tracing to prevent semantic drift"
            );
            break;
        }

        // Take Top 3 candidate nodes to evaluate logically via LLM
        let mut found_next = false;
        let evaluation_pool = ranked.into_iter().take(3);

        for (candidate, coherence_score, c_created_at) in evaluation_pool {
            if let Ok(Some(eval_res)) = evaluate_event_link(
                doubao,
                pool,
                &bookmark_event.title,
                &bookmark_event.summary,
                &bookmark_event.analysis,
                &keywords,
                &bookmark_event.created_at,
                &candidate,
                &c_created_at,
                "past",
            )
            .await
            {
                // Strict validation: check LLM match_score!
                if eval_res.match_score < 0.75 {
                    tracing::info!(
                        candidate_title = %candidate.title,
                        match_score = %eval_res.match_score,
                        "Match score is too low, rejecting weak link"
                    );
                    continue;
                }
                // Save link to DB
                let id = Uuid::new_v4().to_string();
                sqlx::query!(
                    r#"INSERT OR REPLACE INTO bookmark_evidence_chain (id, bookmark_id, matched_event_id, direction, match_score, match_reason)
                       VALUES (?, ?, ?, ?, ?, ?)"#,
                    id,
                    bookmark_id,
                    candidate.id,
                    "past",
                    coherence_score, // Use the real, non-discrete cosine similarity score!
                    eval_res.relation_description
                )
                .execute(pool)
                .await?;

                visited.insert(candidate.id.clone());
                current_node = candidate;
                matched_count += 1;
                found_next = true;
                break; // Found the best verified step, proceed to level + 1
            }
        }

        if !found_next {
            tracing::info!(level = level, "None of the Top candidates were logically verified by LLM, halting");
            break;
        }
    }

    // Insert current row as a marker that this bookmark has been traced
    let marker_id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"INSERT OR REPLACE INTO bookmark_evidence_chain (id, bookmark_id, matched_event_id, direction, match_score, match_reason)
           VALUES (?, ?, ?, 'current', 1.0, '当前关注新闻事件')"#,
        marker_id,
        bookmark_id,
        event_id
    )
    .execute(pool)
    .await?;

    tracing::info!(
        bookmark_id,
        matched_count,
        "5-level retrospective recursive tracing completed"
    );
    Ok(matched_count)
}

/// Run prospective tracking on newly verified consensus events (Calculate real semantic similarity).
pub async fn run_prospective_tracking(
    doubao: &DoubaoClient,
    pool: &SqlitePool,
    _qdrant: Option<&Qdrant>,
    new_events: &[AnalyzedEvent],
    config: &Config,
) -> Result<usize> {
    if new_events.is_empty() {
        return Ok(0);
    }

    // Fetch all active bookmarks
    let active_bookmarks = sqlx::query!(
        r#"SELECT b.id as bookmark_id, b.keywords, e.id as event_id, e.title, e.summary, e.analysis, e.created_at as event_created_at
           FROM bookmarks b
           JOIN events e ON b.event_id = e.id"#
    )
    .fetch_all(pool)
    .await?;

    let mut matched_count = 0;

    for bookmark in active_bookmarks {
        let keywords: Vec<String> = serde_json::from_str(&bookmark.keywords).unwrap_or_default();
        
        let e0_text = format!("{} {}", bookmark.title, bookmark.summary);
        let e0_emb = vectordb::get_embeddings(config, &[e0_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);

        for event in new_events {
            // Skip matching the event with itself
            if bookmark.event_id.as_deref() == Some(&event.id) {
                continue;
            }

            // Fetch candidate created_at from SQLite
            let candidate_created_at = match sqlx::query!(
                "SELECT created_at FROM events WHERE id = ?",
                event.id
            )
            .fetch_optional(pool)
            .await
            {
                Ok(Some(row)) => row.created_at,
                _ => String::new(),
            };

            // Call evaluator to see if this new event fits the bookmark's story
            if let Ok(Some(eval_res)) = evaluate_event_link(
                doubao,
                pool,
                &bookmark.title,
                &bookmark.summary,
                &bookmark.analysis,
                &keywords,
                &bookmark.event_created_at,
                event,
                &candidate_created_at,
                "future",
            )
            .await
            {
                // Calculate real cosine similarity
                let candidate_text = format!("{} {}", event.title, event.summary);
                let candidate_emb = vectordb::get_embeddings(config, &[candidate_text]).await.pop().unwrap_or_else(|| vec![0.0; 1024]);
                let real_score = cosine_similarity(&e0_emb, &candidate_emb) as f64;

                // Strict validation: both LLM match score and vector score must be >= 0.75 / 0.55
                if eval_res.match_score < 0.75 || real_score < 0.55 {
                    tracing::info!(
                        event_title = %event.title,
                        match_score = %eval_res.match_score,
                        real_score = %real_score,
                        "Rejecting weak prospective link due to low scores"
                    );
                    continue;
                }

                let id = Uuid::new_v4().to_string();
                let direction = "future";
                sqlx::query!(
                    r#"INSERT OR REPLACE INTO bookmark_evidence_chain (id, bookmark_id, matched_event_id, direction, match_score, match_reason)
                       VALUES (?, ?, ?, ?, ?, ?)"#,
                    id,
                    bookmark.bookmark_id,
                    event.id,
                    direction,
                    real_score, // Real similarity score!
                    eval_res.relation_description
                )
                .execute(pool)
                .await?;
                matched_count += 1;

                tracing::info!(
                    bookmark_id = bookmark.bookmark_id,
                    matched_event_id = event.id,
                    "New future event linked to bookmark with real score"
                );
            }
        }
    }

    Ok(matched_count)
}
