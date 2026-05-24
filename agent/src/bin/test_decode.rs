use std::time::Duration;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

async fn decode_google_news_url(client: &reqwest::Client, source_url: &str) -> Option<String> {
    let parsed_url = reqwest::Url::parse(source_url).ok()?;
    let art_id = parsed_url.path_segments()?.last()?;
    if art_id.is_empty() {
        println!("art_id is empty");
        return None;
    }

    let articles_url = format!("https://news.google.com/articles/{}", art_id);
    println!("Fetching Articles URL: {}", articles_url);
    let response = client
        .get(&articles_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await;

    let resp = match response {
        Ok(r) => r,
        Err(e) => {
            println!("Failed to send GET request: {}", e);
            return None;
        }
    };

    println!("GET status: {}", resp.status());
    let html = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            println!("Failed to read HTML body: {}", e);
            return None;
        }
    };

    let sg_start_token = "data-n-a-sg=\"";
    let sg_idx = match html.find(sg_start_token) {
        Some(idx) => idx,
        None => {
            println!("Failed to find signature start token");
            // Print a snippet of HTML to see what Google returned
            println!("HTML preview: {}", &html[..html.len().min(1000)]);
            return None;
        }
    };
    let sg_sub = &html[sg_idx + sg_start_token.len()..];
    let sg_end_idx = sg_sub.find('"')?;
    let signature = &sg_sub[..sg_end_idx];

    let ts_start_token = "data-n-a-ts=\"";
    let ts_idx = html.find(ts_start_token)?;
    let ts_sub = &html[ts_idx + ts_start_token.len()..];
    let ts_end_idx = ts_sub.find('"')?;
    let timestamp_str = &ts_sub[..ts_end_idx];
    let timestamp: i64 = timestamp_str.parse().ok()?;

    println!("Signature: {}", signature);
    println!("Timestamp: {}", timestamp);

    let inner_payload = format!(
        "[\"garturlreq\",[[\"X\",\"X\",[\"X\",\"X\"],null,null,1,1,\"US:en\",null,1,null,null,null,null,null,0,1],\"X\",\"X\",1,[1,1,1],1,1,null,0,0,null,0],\"{}\",{},\"{}\"]",
        art_id, timestamp, signature
    );
    let outer_payload_str = serde_json::to_string(&serde_json::json!([
        [
            "Fbv4je",
            inner_payload
        ]
    ])).ok()?;

    let resp = client
        .post("https://news.google.com/_/DotsSplashUi/data/batchexecute")
        .header("User-Agent", USER_AGENT)
        .form(&[("f.req", &outer_payload_str)])
        .send()
        .await
        .ok()?;

    println!("POST status: {}", resp.status());
    if !resp.status().is_success() {
        return None;
    }

    let resp_text = resp.text().await.ok()?;
    let parts: Vec<&str> = resp_text.split("\n\n").collect();
    if parts.len() < 2 {
        println!("Response split parts < 2");
        return None;
    }

    let json_data: serde_json::Value = serde_json::from_str(parts[1]).ok()?;
    let inner_str = json_data.get(0)?.get(2)?.as_str()?;
    let inner_json: serde_json::Value = serde_json::from_str(inner_str).ok()?;
    let decoded_url = inner_json.get(1)?.as_str()?.to_string();

    Some(decoded_url)
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    let url = "https://news.google.com/rss/articles/CBMie0FVX3lxTFBCZjBpSktsMmZrQzFZb0RkWm8xV2dZM1J5Z1kycG5pZlp6SHpSQWpvZG0tSFJYS1gzTmx6TDBHTTNSbDBYLWFsU1VpSFNWTk02WnZsNE12eXpTMktDMVVxNF9EazQwQXlVdnBhZWZPVkotTjZacHFuYXQ4RQ?oc=5";
    println!("Decoding in Rust...");
    if let Some(decoded) = decode_google_news_url(&client, url).await {
        println!("Decoded URL: {}", decoded);
    } else {
        println!("Failed to decode URL");
    }
}
