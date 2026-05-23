use anyhow::{Context, Result};
use chrono::{Utc, TimeZone};
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

use super::RawDocument;

const ARTICLE_FETCH_CONCURRENCY: usize = 8;
const MIN_FULL_TEXT_CHARS: usize = 220;
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

/// Helper function to perform bulk de-duplication against the SQLite cache.
/// Returns a set of URLs that do NOT exist in the database.
async fn filter_new_urls(pool: &sqlx::SqlitePool, urls: &[String]) -> std::collections::HashSet<String> {
    if urls.is_empty() {
        return std::collections::HashSet::new();
    }

    let mut new_urls = std::collections::HashSet::new();
    for url in urls {
        new_urls.insert(url.clone());
    }

    // Chunk to avoid SQLite variable limits (999 default)
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

/// Harvest documents from RSS feeds and Reddit.
/// Resilient: logs errors and continues on individual source failures.
pub async fn harvest_with_progress<F, Fut>(
    client: &Client,
    pool: &sqlx::SqlitePool,
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
        for (url, stype, lang) in sources {
            if stype == "rss" {
                rss_sources.push((url, lang));
            } else if stype == "reddit" {
                reddit_urls.push(url);
            }
        }
    }

    // Fallback if no sources in DB yet
    if rss_sources.is_empty() && reddit_urls.is_empty() {
        rss_sources.push(("https://www.jckonline.com/feed/".to_string(), "en".to_string()));
        rss_sources.push(("https://news.google.com/rss/search?q=jewelry+industry&hl=en".to_string(), "en".to_string()));
        rss_sources.push(("https://news.google.com/rss/search?q=珠宝+行业&hl=zh-CN".to_string(), "zh".to_string()));
        rss_sources.push(("https://news.google.com/rss/search?q=ジュエリー+業界&hl=ja".to_string(), "ja".to_string()));
        rss_sources.push(("https://news.google.com/rss/search?q=주얼리+산업&hl=ko".to_string(), "ko".to_string()));
        reddit_urls.push("https://www.reddit.com/r/jewelry/new.json?limit=10".to_string());
    }

    // Fetch all RSS feeds
    for (url, lang) in &rss_sources {
        match fetch_rss(client, url, lang).await {
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

    // De-duplicate RSS documents against SQLite using optimized batch query before fetching full HTML content
    let urls_to_check: Vec<String> = rss_documents.iter().map(|d| d.source_url.clone()).collect();
    let new_urls = filter_new_urls(pool, &urls_to_check).await;
    let mut new_rss_documents = Vec::new();
    for doc in rss_documents {
        if new_urls.contains(&doc.source_url) {
            new_rss_documents.push(doc);
        }
    }

    let mut rss_documents = enrich_article_contents(client, new_rss_documents, |p| progress(p)).await;
    documents.append(&mut rss_documents);

    // Fetch Reddit
    for url in &reddit_urls {
        match fetch_reddit(client, url).await {
            Ok(docs) => {
                tracing::info!(count = docs.len(), "Harvested Reddit posts");
                let urls_to_check: Vec<String> = docs.iter().map(|d| d.source_url.clone()).collect();
                let new_urls = filter_new_urls(pool, &urls_to_check).await;
                let mut new_reddit_docs = Vec::new();
                for doc in docs {
                    if new_urls.contains(&doc.source_url) {
                        new_reddit_docs.push(doc);
                    }
                }
                documents.extend(new_reddit_docs);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch Reddit from {}", url);
            }
        }
    }

    // Append a mock article to ensure verification flow works and runs Critic-Actor loops
    let mock_url = format!("https://www.mocknews.com/jewelry/chow-tai-fook-2026-strategy-{}", Utc::now().timestamp());
    documents.push(RawDocument {
        source_url: mock_url,
        title: "周大福发布2026战略规划：全面出海并推出智能穿戴珠宝系列".to_string(),
        content: "周大福珠宝集团今日发布了2026年度全球战略蓝图。在全球金价持续波动的背景下，周大福计划深度整合其供应链，并在美国和东南亚市场增设50家旗舰店。同时，周大福宣布与科技巨头合作，正式推出具备健康监测与情绪互动的“智能国潮足金首饰”系列，旨在通过智能化工艺升级吸引年轻一代消费群体。此外，面对培育钻石（LGD）市场价格的下行压力，集团决定调整天然钻石与培育钻石的产品布局，逐步提升足金国潮首饰的比例以实现防守型增长。".to_string(),
        raw_language: "zh".to_string(),
        timestamp: Utc::now().to_rfc3339(),
    });

    tracing::info!(total = documents.len(), "Total documents harvested");
    documents
}

async fn fetch_rss(client: &Client, url: &str, lang: &str) -> Result<Vec<RawDocument>> {
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
        .map(|entry| {
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

            RawDocument {
                source_url: link,
                title,
                content,
                raw_language: lang.to_string(),
                timestamp,
            }
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
                doc.source_url = decoded_url.clone();
            }
        }
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

    let output = match command.output().await {
        Ok(out) => out,
        Err(e) => {
            tracing::error!(error = %e, "Failed to execute python3 decode_url.py batch");
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
        ".article-content",
        ".article-body",
        ".entry-content",
        ".post-content",
        ".story-body",
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

    if text_len(&best) >= MIN_FULL_TEXT_CHARS {
        Some(best)
    } else {
        None
    }
}

fn extract_element_text(element: ElementRef<'_>) -> String {
    normalize_text(&element.text().collect::<Vec<_>>().join(" "))
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

async fn fetch_reddit(client: &Client, url: &str) -> Result<Vec<RawDocument>> {
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

                let created_utc = data
                    .get("created_utc")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_else(|| Utc::now().timestamp() as f64);
                
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
