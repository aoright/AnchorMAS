use anyhow::{Context, Result};
use chrono::{Utc, TimeZone};
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

use super::RawDocument;

const ARTICLE_FETCH_CONCURRENCY: usize = 8;
const MIN_FULL_TEXT_CHARS: usize = 120;
const MAX_ARTICLE_CHARS: usize = 8_000;
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Default)]
pub struct HarvestProgress {
    pub processed_count: usize,
    pub total_count: usize,
    pub output_count: usize,
    pub failed_count: usize,
    pub message: String,
    pub last_error: Option<String>,
}



/// Harvest documents from RSS feeds and Reddit.
/// Resilient: logs errors and continues on individual source failures.
pub async fn harvest_with_progress<F, Fut>(
    client: &Client,
    pool: &sqlx::SqlitePool,
    hours: Option<u32>,
    mut progress: F,
) -> Vec<RawDocument>
where
    F: FnMut(HarvestProgress) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut documents = Vec::new();
    let mut rss_documents = Vec::new();

    // Query active sources from database
    let sources_result: Result<Vec<(String, String, String)>, sqlx::Error> = sqlx::query_as(
        "SELECT url, source_type, language FROM data_sources WHERE is_active = 1"
    )
    .fetch_all(pool)
    .await;

    let mut rss_sources = Vec::new();
    let mut reddit_urls = Vec::new();

    if let Ok(sources) = sources_result {
        for (mut url, stype, lang) in sources {
            if stype == "rss" {
                if let Some(h) = hours {
                    if url.contains("news.google.com/rss/") {
                        if let Ok(mut parsed_url) = url::Url::parse(&url) {
                            let mut query_pairs: Vec<(String, String)> = parsed_url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
                            let mut found_q = false;
                            for (k, v) in &mut query_pairs {
                                if k == "q" {
                                    if !v.contains("when:") {
                                        *v = format!("{} when:{}h", v, h);
                                    }
                                    found_q = true;
                                    break;
                                }
                            }
                            if !found_q {
                                query_pairs.push(("q".to_string(), format!("when:{}h", h)));
                            }
                            let query_str = query_pairs
                                .iter()
                                .map(|(k, v)| format!("{}={}", k, url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()))
                                .collect::<Vec<String>>()
                                .join("&");
                            parsed_url.set_query(Some(&query_str));
                            url = parsed_url.to_string();
                        }
                    }
                }
                rss_sources.push((url, lang));
            } else if stype == "reddit" {
                reddit_urls.push(url);
            }
        }
    }

    // Query custom keywords from user settings
    let keywords_row: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM user_settings WHERE key = 'keywords'"
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let custom_keywords: Vec<String> = keywords_row
        .and_then(|r| serde_json::from_str(&r.0).ok())
        .unwrap_or_default();

    if !custom_keywords.is_empty() {
        for kw in &custom_keywords {
            if kw.trim().is_empty() {
                continue;
            }
            let encoded_kw = url::form_urlencoded::byte_serialize(kw.as_bytes()).collect::<String>();
            let lang = if kw.chars().any(|c| (c as u32) >= 0x4e00 && (c as u32) <= 0x9fff) {
                "zh"
            } else if kw.chars().any(|c| ((c as u32) >= 0x3040 && (c as u32) <= 0x30ff) || ((c as u32) >= 0x31f0 && (c as u32) <= 0x31ff)) {
                "ja"
            } else if kw.chars().any(|c| (c as u32) >= 0xac00 && (c as u32) <= 0xd7a3) {
                "ko"
            } else {
                "en"
            };

            let hl = match lang {
                "zh" => "zh-CN",
                "ja" => "ja",
                "ko" => "ko",
                _ => "en",
            };

            let h = hours.unwrap_or(24);
            let rss_url = format!("https://news.google.com/rss/search?q={}+when:{}h&hl={}", encoded_kw, h, hl);
            rss_sources.push((rss_url, lang.to_string()));
        }
    }


    // Fallback if no sources in DB yet
    if rss_sources.is_empty() && reddit_urls.is_empty() {
        let h = hours.unwrap_or(24);
        rss_sources.push(("https://www.jckonline.com/feed/".to_string(), "en".to_string()));
        rss_sources.push((format!("https://news.google.com/rss/search?q=jewelry+industry+when:{}h&hl=en", h), "en".to_string()));
        rss_sources.push((format!("https://news.google.com/rss/search?q=珠宝+行业+when:{}h&hl=zh-CN", h), "zh".to_string()));
        rss_sources.push((format!("https://news.google.com/rss/search?q=ジュエリー+行业+when:{}h&hl=ja", h), "ja".to_string()));
        rss_sources.push((format!("https://news.google.com/rss/search?q=주얼리+산업+when:{}h&hl=ko", h), "ko".to_string()));
        reddit_urls.push("https://www.reddit.com/r/jewelry/new.json?limit=10".to_string());
    }

    // Fetch all RSS feeds
    for (url, lang) in &rss_sources {
        match fetch_rss(client, url, lang, hours).await {
            Ok(mut docs) => {
                tracing::info!(
                    source = %url,
                    count = docs.len(),
                    "Harvested RSS feed"
                );
                rss_documents.append(&mut docs);
                progress(HarvestProgress {
                    processed_count: rss_documents.len(),
                    output_count: rss_documents.len(),
                    message: format!("Fetched RSS feed: {}", url),
                    ..HarvestProgress::default()
                })
                .await;
            }
            Err(e) => {
                tracing::error!(source = %url, error = %e, "Failed to fetch RSS feed");
                progress(HarvestProgress {
                    failed_count: 1,
                    message: format!("Failed to fetch RSS feed: {}", url),
                    last_error: Some(e.to_string()),
                    ..HarvestProgress::default()
                })
                .await;
            }
        }
    }

    // Load existing articles from database to de-duplicate against both URL and Title
    let existing_articles: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT title, source_url, resolved_url FROM raw_articles"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut existing_urls = std::collections::HashSet::new();
    let mut existing_normalized_titles = std::collections::HashSet::new();

    for (title, source_url, resolved_url) in existing_articles {
        existing_urls.insert(source_url);
        if let Some(res_url) = resolved_url {
            existing_urls.insert(res_url);
        }
        existing_normalized_titles.insert(normalize_title(&title));
    }

    let mut new_rss_documents = Vec::new();
    for doc in rss_documents {
        let norm_title = normalize_title(&doc.title);
        if !existing_urls.contains(&doc.source_url) && !existing_normalized_titles.contains(&norm_title) {
            new_rss_documents.push(doc);
            if !norm_title.is_empty() && norm_title != "untitled" {
                existing_normalized_titles.insert(norm_title);
            }
        }
    }

    let mut rss_documents = enrich_article_contents(client, new_rss_documents, |p| progress(p)).await;
    documents.append(&mut rss_documents);

    // Fetch Reddit
    for url in &reddit_urls {
        match fetch_reddit(client, url, hours).await {
            Ok(docs) => {
                tracing::info!(count = docs.len(), "Harvested Reddit posts");
                let mut new_reddit_docs = Vec::new();
                for doc in docs {
                    let norm_title = normalize_title(&doc.title);
                    if !existing_urls.contains(&doc.source_url) && !existing_normalized_titles.contains(&norm_title) {
                        new_reddit_docs.push(doc);
                        if !norm_title.is_empty() && norm_title != "untitled" {
                            existing_normalized_titles.insert(norm_title);
                        }
                    }
                }
                documents.extend(new_reddit_docs);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch Reddit from {}", url);
            }
        }
    }

    // Append a mock article only in debug builds or when AIRS_ENABLE_MOCK is set
    if cfg!(debug_assertions) || std::env::var("AIRS_ENABLE_MOCK").unwrap_or_default() == "1" {
        let mock_url = format!("https://www.mocknews.com/jewelry/chow-tai-fook-2026-strategy-{}", Utc::now().timestamp());
        documents.push(RawDocument {
            source_url: mock_url,
            title: "周大福发布2026战略规划：全面出海并推出智能穿戴珠宝系列".to_string(),
            content: "周大福珠宝集团今日发布了2026年度全球战略蓝图。在全球金价持续波动的背景下，周大福计划深度整合其供应链，并在美国和东南亚市场增设50家旗舰店。同时，周大福宣布与科技巨头合作，正式推出具备健康监测与情绪互动的“智能国潮足金首饰”系列，旨在通过智能化工艺升级吸引年轻一代消费群体。此外，面对培育钻石（LGD）市场价格的下行压力，集团决定调整天然钻石与培育钻石的产品布局，逐步提升足金国潮首饰的比例以实现防守型增长。".to_string(),
            raw_language: "zh".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            original_url: None,
        });
        tracing::info!("Mock article injected (debug/test mode)");
    }

    tracing::info!(total = documents.len(), "Total documents harvested");
    documents
}

async fn fetch_rss(client: &Client, url: &str, lang: &str, hours: Option<u32>) -> Result<Vec<RawDocument>> {
    let body = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("HTTP request failed")?
        .bytes()
        .await
        .context("Failed to read response body")?;

    let feed = feed_rs::parser::parse(&body[..])
        .context("Failed to parse feed with feed-rs")?;

    let docs: Vec<RawDocument> = feed
        .entries
        .into_iter()
        .filter_map(|entry| {
            if let (Some(published), Some(h)) = (entry.published, hours) {
                let cutoff = Utc::now() - chrono::Duration::hours(h as i64);
                if published < cutoff {
                    return None;
                }
            }

            let title = entry.title.map(|t| t.content).unwrap_or_else(|| "Untitled".to_string());
            let link = entry.links.first().map(|l| l.href.clone()).unwrap_or_else(|| url.to_string());
            
            // Get content or summary
            let summary_text = entry.summary.map(|s| s.content).unwrap_or_default();
            let content_text = entry.content.and_then(|c| c.body).unwrap_or_default();
            let raw_desc = if content_text.len() > summary_text.len() { content_text } else { summary_text };

            // Strip HTML tags from description using scraper
            let content = strip_html(&raw_desc);

            let timestamp = entry.published
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

            Some(RawDocument {
                source_url: link,
                title,
                content,
                raw_language: lang.to_string(),
                timestamp,
                original_url: None,
            })
        })
        .collect();

    Ok(docs)
}

async fn enrich_article_contents<F, Fut>(
    client: &Client,
    docs: Vec<RawDocument>,
    mut progress: F,
) -> Vec<RawDocument>
where
    F: FnMut(HarvestProgress) -> Fut,
    Fut: Future<Output = ()>,
{
    if docs.is_empty() {
        return docs;
    }

    let mut docs = docs;
    
    // Extract Google News URLs to decode in batch
    let gnews_urls: Vec<String> = docs
        .iter()
        .filter(|doc| doc.source_url.contains("news.google.com/rss/articles/") || doc.source_url.contains("news.google.com/articles/"))
        .map(|doc| doc.source_url.clone())
        .collect();

    if !gnews_urls.is_empty() {
        tracing::info!("Batch decoding {} Google News URLs...", gnews_urls.len());
        let decoded_map = decode_google_news_urls_batch(&gnews_urls).await;
        tracing::info!("Decoded {} / {} Google News URLs", decoded_map.len(), gnews_urls.len());
        for doc in &mut docs {
            if let Some(decoded_url) = decoded_map.get(&doc.source_url) {
                doc.original_url = Some(doc.source_url.clone());
                doc.source_url = decoded_url.clone();
            }
        }
    }

    // Filter out articles that are still Google News URLs (meaning batch decoding failed)
    let original_count = docs.len();
    docs.retain(|doc| !doc.source_url.contains("news.google.com"));
    let filtered_count = original_count - docs.len();
    if filtered_count > 0 {
        tracing::info!("Filtered out {} unresolved Google News articles to prevent database pollution", filtered_count);
    }

    let total_count = docs.len();
    progress(HarvestProgress {
        total_count,
        message: format!("Fetching full article text for {} RSS articles", total_count),
        ..HarvestProgress::default()
    })
    .await;

    let semaphore = Arc::new(Semaphore::new(ARTICLE_FETCH_CONCURRENCY));
    let (tx, mut rx) = mpsc::unbounded_channel();

    for (index, doc) in docs.into_iter().enumerate() {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(e) => {
                    let _ = tx.send(ArticleContentOutcome {
                        index,
                        doc,
                        enriched: false,
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };

            // Politeness delay: sleep a offset duration to avoid parallel requests slamming the same site
            let delay_ms = 150 * (index % 10) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

            let mut doc = doc;
            let original_len = text_len(&doc.content);
            let result = fetch_article_text(&client, &doc.source_url).await;
            match result {
                Ok(text) if text_len(&text) > original_len && text_len(&text) >= MIN_FULL_TEXT_CHARS => {
                    doc.content = truncate_chars(&text, MAX_ARTICLE_CHARS);
                    let _ = tx.send(ArticleContentOutcome {
                        index,
                        doc,
                        enriched: true,
                        error: None,
                    });
                }
                Ok(_) => {
                    let _ = tx.send(ArticleContentOutcome {
                        index,
                        doc,
                        enriched: false,
                        error: None,
                    });
                }
                Err(e) => {
                    tracing::debug!(
                        source_url = %doc.source_url,
                        error = %e,
                        "Failed to fetch full article text, keeping RSS summary"
                    );
                    let _ = tx.send(ArticleContentOutcome {
                        index,
                        doc,
                        enriched: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        });
    }
    drop(tx);

    let mut outcomes = Vec::with_capacity(total_count);
    let mut processed_count = 0;
    let mut enriched_count = 0;
    let mut failed_count = 0;
    let mut last_error = None;

    while let Some(outcome) = rx.recv().await {
        processed_count += 1;
        if outcome.enriched {
            enriched_count += 1;
        }
        if let Some(error) = outcome.error.clone() {
            failed_count += 1;
            last_error = Some(error);
        }

        progress(HarvestProgress {
            processed_count,
            total_count,
            output_count: enriched_count,
            failed_count,
            message: format!(
                "Fetched article bodies: {} / {} processed, {} enriched",
                processed_count, total_count, enriched_count
            ),
            last_error: last_error.clone(),
        })
        .await;

        outcomes.push(outcome);
    }

    outcomes.sort_by_key(|outcome| outcome.index);
    outcomes.into_iter().map(|outcome| outcome.doc).collect()
}

struct ArticleContentOutcome {
    index: usize,
    doc: RawDocument,
    enriched: bool,
    error: Option<String>,
}

async fn decode_google_news_urls_batch(urls: &[String]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if urls.is_empty() {
        return map;
    }

    let script_path = std::env::var("AIRS_DECODE_URL_SCRIPT")
        .unwrap_or_else(|_| "scripts/decode_url.py".to_string());
    let mut command = tokio::process::Command::new("python3");
    command.arg(script_path);
    for url in urls {
        command.arg(url);
    }

    let output = match tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::error!(error = %e, "Failed to execute python3 decode_url.py batch");
            return map;
        }
        Err(_) => {
            tracing::error!("decode_url.py batch process timed out after 30 seconds");
            return map;
        }
    };

    if !output.status.success() {
        tracing::error!(
            status = ?output.status,
            stderr = %String::from_utf8_lossy(&output.stderr),
            "decode_url.py batch process exited with non-zero code"
        );
        return map;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(decoded_map) = serde_json::from_str::<std::collections::HashMap<String, String>>(&stdout) {
        map = decoded_map;
    } else {
        tracing::error!("Failed to parse decode_url.py batch stdout: {}", stdout);
    }

    map
}

async fn fetch_article_text(client: &Client, url: &str) -> Result<String> {
    let html = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/html,application/xhtml+xml")
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .context("Article HTTP request failed")?
        .error_for_status()
        .context("Article HTTP status was not successful")?
        .text()
        .await
        .context("Failed to read article response body")?;

    extract_article_text(&html).context("No usable article text found")
}

fn extract_article_text(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let container_selectors = [
        "article",
        "main",
        "[role=\"main\"]",
        "[itemprop=\"articleBody\"]",
        ".article-content",
        ".article-body",
        ".entry-content",
        ".post-content",
        ".story-body",
        ".news-content",
        ".post-body",
        "#content",
        "div.article",
        "div.content",
        ".content",
    ];

    let mut best = String::new();
    for selector in container_selectors {
        if let Ok(selector) = Selector::parse(selector) {
            for element in document.select(&selector) {
                let text = extract_element_text(element);
                if text_len(&text) > text_len(&best) {
                    best = text;
                }
            }
        }
    }

    if text_len(&best) < MIN_FULL_TEXT_CHARS {
        best = extract_paragraph_text(&document);
    }

    // Final fallback: extract all text from <body> if still below threshold
    if text_len(&best) < MIN_FULL_TEXT_CHARS {
        if let Ok(body_sel) = Selector::parse("body") {
            for element in document.select(&body_sel) {
                let text = extract_element_text(element);
                if text_len(&text) > text_len(&best) {
                    best = text;
                }
            }
        }
    }

    if text_len(&best) >= MIN_FULL_TEXT_CHARS {
        Some(best)
    } else {
        None
    }
}

fn extract_element_text(element: ElementRef<'_>) -> String {
    let mut text = String::new();
    traverse_clean_text(*element, &mut text);
    normalize_text(&text)
}

fn traverse_clean_text(node: ego_tree::NodeRef<'_, scraper::Node>, text: &mut String) {
    match node.value() {
        scraper::Node::Text(t) => {
            text.push_str(&t);
            text.push(' ');
        }
        scraper::Node::Element(elem) => {
            let name = elem.name();
            if name == "script" || name == "style" || name == "noscript" || name == "iframe" {
                return;
            }
            for child in node.children() {
                traverse_clean_text(child, text);
            }
        }
        _ => {
            for child in node.children() {
                traverse_clean_text(child, text);
            }
        }
    }
}

fn extract_paragraph_text(document: &Html) -> String {
    let selector = match Selector::parse("p") {
        Ok(selector) => selector,
        Err(_) => return String::new(),
    };

    let paragraphs: Vec<String> = document
        .select(&selector)
        .map(extract_element_text)
        .filter(|text| text_len(text) >= 40)
        .collect();

    normalize_text(&paragraphs.join("\n\n"))
}

async fn fetch_reddit(client: &Client, url: &str, hours: Option<u32>) -> Result<Vec<RawDocument>> {
    let resp = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("Reddit HTTP request failed")?
        .json::<serde_json::Value>()
        .await
        .context("Failed to parse Reddit JSON")?;

    let mut docs = Vec::new();

    if let Some(children) = resp
        .get("data")
        .and_then(|d| d.get("children"))
        .and_then(|c| c.as_array())
    {
        for child in children {
            if let Some(data) = child.get("data") {
                let created_utc = data
                    .get("created_utc")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_else(|| Utc::now().timestamp() as f64);
                
                if let Some(h) = hours {
                    let cutoff = Utc::now().timestamp() - (h as i64 * 3600);
                    if (created_utc as i64) < cutoff {
                        continue;
                    }
                }

                let title = data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let permalink = data
                    .get("permalink")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let selftext = data
                    .get("selftext")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let source_url = if permalink.is_empty() {
                    url.to_string()
                } else {
                    format!("https://www.reddit.com{}", permalink)
                };

                let timestamp = match chrono::Utc.timestamp_opt(created_utc as i64, 0) {
                    chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                    _ => Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                };

                docs.push(RawDocument {
                    source_url,
                    title,
                    content: selftext,
                    raw_language: "en".to_string(),
                    timestamp,
                    original_url: None,
                });
            }
        }
    }

    Ok(docs)
}

/// Strip HTML tags from a string, returning only text content.
fn strip_html(html: &str) -> String {
    let fragment = scraper::Html::parse_fragment(html);
    fragment
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn text_len(text: &str) -> usize {
    text.chars().count()
}

pub fn normalize_title(title: &str) -> String {
    let mut clean = title.to_string();
    if let Some(pos) = clean.rfind(" - ") {
        if clean.len() - pos < 25 {
            clean.truncate(pos);
        }
    }
    clean
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}
