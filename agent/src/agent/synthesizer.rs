use anyhow::{Context, Result};
use std::collections::HashMap;
use uuid::Uuid;

use super::{AnalyzedEvent, DoubaoClient, StrategicBriefing};

/// Synthesize all verified events into a strategic briefing.
pub async fn synthesize_briefing(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    events: &[AnalyzedEvent],
) -> Result<StrategicBriefing> {
    if events.is_empty() {
        return Ok(StrategicBriefing {
            id: Uuid::new_v4().to_string(),
            date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            overview: "今日无重大珠宝行业事件。".to_string(),
            heatmap: default_heatmap(),
            events: Vec::new(),
            recommendations: vec!["持续监控各市场动态。".to_string()],
        });
    }

    let default_system_prompt = r#"你是珠宝行业首席战略顾问。请将以下经过验证的市场事件，整合为一份面向管理层的每日战略简报。

请输出以下JSON格式：
{
  "overview": "50字以内的核心综述",
  "heatmap": {
    "China": "稳定|关注|警告|紧急",
    "Japan": "稳定|关注|警告|紧急",
    "Korea": "稳定|关注|警告|紧急",
    "SoutheastAsia": "稳定|关注|警告|紧急",
    "UnitedStates": "稳定|关注|警告|紧急"
  },
  "recommendations": [
    "具体行动建议1",
    "具体行动建议2",
    "..."
  ]
}

评估标准：
- 稳定：无重大变化，维持现有策略
- 关注：出现值得关注的信号，需持续监控
- 警告：发现潜在风险或重大机会，需制定预案
- 紧急：需要立即采取行动的紧迫事件

行动建议要具体、可执行，指明负责部门和时间要求。

只返回JSON对象，不要包含其他文字。"#;

    let system_prompt = super::get_agent_prompt(pool, "synthesizer", default_system_prompt).await;

    let events_json: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "title": e.title,
                "summary": e.summary,
                "category": e.category,
                "market": e.market,
                "impact_type": e.impact_type,
                "severity": e.severity,
                "urgency": e.urgency,
                "confidence": e.confidence,
                "analysis": e.analysis,
            })
        })
        .collect();

    let user_prompt = format!(
        "今日共收集到{}个经过验证的市场事件：\n\n{}",
        events.len(),
        serde_json::to_string_pretty(&events_json)?
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let briefing = parse_briefing_response(&response, events)?;

    tracing::info!(id = %briefing.id, "Strategic briefing synthesized");
    Ok(briefing)
}

fn parse_briefing_response(
    response: &str,
    events: &[AnalyzedEvent],
) -> Result<StrategicBriefing> {
    let json_str = extract_json_object(response);
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .context("Failed to parse synthesis response as JSON")?;

    let overview = parsed
        .get("overview")
        .and_then(|v| v.as_str())
        .unwrap_or("今日珠宝市场简报已生成。")
        .to_string();

    let mut heatmap = default_heatmap();
    if let Some(hm) = parsed.get("heatmap").and_then(|v| v.as_object()) {
        for (key, value) in hm {
            if let Some(status) = value.as_str() {
                heatmap.insert(key.clone(), status.to_string());
            }
        }
    }

    let recommendations: Vec<String> = parsed
        .get("recommendations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| vec!["请持续关注市场动态。".to_string()]);

    Ok(StrategicBriefing {
        id: Uuid::new_v4().to_string(),
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        overview,
        heatmap,
        events: events.to_vec(),
        recommendations,
    })
}

fn default_heatmap() -> HashMap<String, String> {
    let mut heatmap = HashMap::new();
    heatmap.insert("China".to_string(), "稳定".to_string());
    heatmap.insert("Japan".to_string(), "稳定".to_string());
    heatmap.insert("Korea".to_string(), "稳定".to_string());
    heatmap.insert("SoutheastAsia".to_string(), "稳定".to_string());
    heatmap.insert("UnitedStates".to_string(), "稳定".to_string());
    heatmap
}

fn extract_json_object(text: &str) -> String {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}
