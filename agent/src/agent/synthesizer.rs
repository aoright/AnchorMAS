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
            date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            overview: "今日无重大珠宝行业事件。".to_string(),
            heatmap: default_heatmap(),
            events: Vec::new(),
            recommendations: vec!["持续监控各市场动态。".to_string()],
        });
    }

    // Quality check input events and log feedback
    for event in events {
        if event.analysis.trim().is_empty() {
            let analyst_role = format!("analyst_{}", event.category.to_lowercase());
            let _ = crate::agent::blackboard::log_feedback(
                pool,
                "synthesizer",
                &analyst_role,
                Some(&event.id),
                "分析内容为空。请补充具体的商业机会与风险深度分析。",
            )
            .await;
        } else if event.severity == 1 && event.analysis.contains("重大") {
            let analyst_role = format!("analyst_{}", event.category.to_lowercase());
            let _ = crate::agent::blackboard::log_feedback(
                pool,
                "synthesizer",
                &analyst_role,
                Some(&event.id),
                "事件严重度评级为轻微(1分)，但在正文分析中使用了'重大'描述。评级与描述存在冲突。请修正评分标准。",
            )
            .await;
            let _ = crate::agent::blackboard::log_feedback(
                pool,
                "synthesizer",
                "critic",
                Some(&event.id),
                "事实核查官未能纠正分析中评分与描述的矛盾。请加强逻辑自洽性审核。",
            )
            .await;
        }
    }

    let default_system_prompt = r#"你是珠宝行业首席战略顾问。请将以下经过验证的市场事件，按不同地区市场，整合为一份面向管理层的每日战略简报。

请输出以下JSON格式：
{
  "overview": {
    "Global": {
      "summary": "全球珠宝行业宏观战略综述，包含今日核心趋势、价格波动及市场影响的深度分析，字数在150-250字之间。",
      "keywords": [
        {
          "word": "核心关键词或短句（如：金价历史新高）",
          "explanation": "对该关键词/句在此刻市场环境下的商业逻辑和深远战略解释，字数在50-100字之间。",
          "event_ids": ["与此关键词相关的具体新闻/事件ID (UUID)，必须从下方给出的事件列表中获取，可包含多个ID。若无直接对应事件则留空数组。"]
        }
      ]
    },
    "China": {
      "summary": "中国珠宝市场的详细战略总结，分析宏观面、国潮趋势、主要品牌动态等。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "Japan": {
      "summary": "日本珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "Korea": {
      "summary": "韩国珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "SoutheastAsia": {
      "summary": "东南亚珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    },
    "UnitedStates": {
      "summary": "美国珠宝市场的详细战略总结。注意：必须依据今日传入的该市场事件列表进行实质总结，字数在150-250字之间。只有当传入该市场的事件列表为空时，才能且必须写‘今日无重大事件’。",
      "keywords": [
        {
          "word": "关键词或短句",
          "explanation": "深度战略释义，字数在50-100字之间。",
          "event_ids": ["对应事件的ID (UUID)"]
        }
      ]
    }
  },
  "heatmap": {
    "China": {
      "status": "稳定|关注|警告|紧急",
      "notes": "状态维持或变化的核心原因，15字以内"
    },
    "Japan": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "Korea": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "SoutheastAsia": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    },
    "UnitedStates": {
      "status": "稳定|关注|警告|紧急",
      "notes": "核心原因，15字以内"
    }
  },
  "recommendations": [
    "针对性策略行动建议1（需具体到负责部门和执行截止时间，如：供应链部须在2日内完成...）",
    "针对性策略行动建议2",
    "..."
  ]
}

评估标准：
- 稳定：无重大变化，维持现有策略
- 关注：出现值得关注的信号，需持续监控
- 警告：发现潜在风险或重大机会，需制定预案
- 紧急：需要立即采取行动的紧迫事件

只返回JSON对象，不要包含其他任何解释性或markdown标记外层包装的文字。"#;

    // Query benchmark companies from user settings
    let companies_row: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM user_settings WHERE key = 'benchmark_companies'"
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let benchmark_companies: Vec<String> = companies_row
        .and_then(|r| serde_json::from_str(&r.0).ok())
        .unwrap_or_default();

    let mut system_prompt = super::get_agent_prompt(pool, "synthesizer", default_system_prompt).await;

    // 强力质检底线规则：强制要求对已有事件的市场进行具体详实的分析，禁止敷衍使用“今日无重大事件”
    system_prompt.push_str("\n\n【关键质检底线规则】：\n对于传入事件列表中已经包含具体事件的地区市场（例如 Japan、Korea、SoutheastAsia 等），严禁在 JSON 响应的 'overview' 概览里使用‘今日无重大事件’或空洞套话敷衍。你必须依据该市场下列出的所有事件，进行具有商业逻辑和启发性的实质性总结与分析。");

    if !benchmark_companies.is_empty() {
        system_prompt.push_str(&format!(
            "\n\n请在撰写每日简报时，特别关注并重点分析与以下对标公司（Benchmark Companies）相关的动态，并阐述对本企业的战略启示或潜在威胁：\n{}",
            benchmark_companies.iter().map(|c| format!("- {}", c)).collect::<Vec<_>>().join("\n")
        ));
    }

    // 按市场（market）分组事件，并去除冗余的庞大 analysis 文本
    let mut market_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for e in events {
        let ev_val = serde_json::json!({
            "id": e.id,
            "title": e.title,
            "summary": e.summary,
            "category": e.category,
            "impact_type": e.impact_type,
            "severity": e.severity,
            "urgency": e.urgency,
            "confidence": e.confidence,
        });
        market_groups.entry(e.market.clone()).or_default().push(ev_val);
    }

    let mut events_section = String::new();
    for (market, group) in &market_groups {
        events_section.push_str(&format!("### 【{} 市场事件（共 {} 个）】\n", market, group.len()));
        events_section.push_str(&serde_json::to_string_pretty(group)?);
        events_section.push_str("\n\n");
    }

    let user_prompt = format!(
        "今日共收集到{}个经过验证的市场事件，已按地区市场分组如下：\n\n{}",
        events.len(),
        events_section
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;

    // Charge credits:
    let tokens = (system_prompt.len() + user_prompt.len() + response.len()) / 3;
    let _ = super::parliament::charge_compute_credits(pool, "synthesizer", tokens as i64).await;

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
        .map(|v| {
            if v.is_object() {
                serde_json::to_string(v).unwrap_or_else(|_| "今日珠宝市场简报已生成。".to_string())
            } else {
                v.as_str().unwrap_or("今日珠宝市场简报已生成。").to_string()
            }
        })
        .unwrap_or_else(|| "今日珠宝市场简报已生成。".to_string());

    let mut heatmap = default_heatmap();
    if let Some(hm) = parsed.get("heatmap").and_then(|v| v.as_object()) {
        for (key, value) in hm {
            let (status, notes) = if value.is_object() {
                let status = value.get("status").and_then(|s| s.as_str()).unwrap_or("稳定").to_string();
                let notes = value.get("notes").and_then(|n| n.as_str()).unwrap_or("--").to_string();
                (status, notes)
            } else {
                let status = value.as_str().unwrap_or("稳定").to_string();
                (status, "--".to_string())
            };
            heatmap.insert(key.clone(), super::MarketStatus { status, notes });
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
        date: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        overview,
        heatmap,
        events: events.to_vec(),
        recommendations,
    })
}

fn default_heatmap() -> HashMap<String, super::MarketStatus> {
    let mut heatmap = HashMap::new();
    heatmap.insert("China".to_string(), super::MarketStatus { status: "稳定".to_string(), notes: "--".to_string() });
    heatmap.insert("Japan".to_string(), super::MarketStatus { status: "稳定".to_string(), notes: "--".to_string() });
    heatmap.insert("Korea".to_string(), super::MarketStatus { status: "稳定".to_string(), notes: "--".to_string() });
    heatmap.insert("SoutheastAsia".to_string(), super::MarketStatus { status: "稳定".to_string(), notes: "--".to_string() });
    heatmap.insert("UnitedStates".to_string(), super::MarketStatus { status: "稳定".to_string(), notes: "--".to_string() });
    heatmap
}

fn extract_json_object(text: &str) -> String {
    super::extract_json_object(text)
}

/// Audit the quality of the synthesized briefing. If any quality issues are detected,
/// it logs feedback targeting 'synthesizer' to agent_feedback_log.
pub async fn audit_briefing(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    briefing: &StrategicBriefing,
) -> Result<()> {
    let default_system_prompt = r#"你是一个资深的报告审计官（角色：【简报质量审计专家】）。
你的任务是审查每日战略简报的质量，看它是否符合高管决策的要求。

主要审计项：
1. 综述（overview）是否过于笼统或存在拼写/语言不统一的问题（必须全中文，且具体到各市场核心动态）。
   注意：如果今日传入该市场的实际事件数量为 0，则该市场【必须】写为“今日无重大事件”。只有当实际事件数大于 0 时，才绝对禁止写“今日无重大事件”或空动套话，而必须具体分析。
2. 热力图评级是否真实反映了事件的紧急度与严重度，是否与综述内容一致。
3. 行动建议（recommendations）是否具体、可执行，是否指明了对应的业务部门和时间限制（例如，不能只写“密切关注”，而应该写“营运部本周内调整定价”）。

如果发现问题，请写明具体的问题和优化建议，这些内容将被写入反馈日志，指导 Synthesizer 特工优化其 Prompt。
请以 JSON 格式输出审计结果：
{
  "approved": true|false,
  "critique_notes": "指出具体的问题（不超过100字），如果 approved 为 true 则写'合格'"
}"#;

    let system_prompt = super::get_agent_prompt(pool, "auditor", default_system_prompt).await;

    let briefing_json = serde_json::json!({
        "overview": briefing.overview,
        "heatmap": briefing.heatmap,
        "recommendations": briefing.recommendations,
    });

    let mut market_counts = std::collections::HashMap::new();
    for event in &briefing.events {
        *market_counts.entry(event.market.clone()).or_insert(0) += 1;
    }
    let counts_str = market_counts
        .iter()
        .map(|(m, c)| format!("- {}: {} 个事件", m, c))
        .collect::<Vec<_>>()
        .join("\n");

    let user_prompt = format!(
        "今日各市场的实际事件数量如下：\n{}\n\n以下是新生成的每日战略简报，请对其质量进行审计：\n\n{}",
        counts_str,
        serde_json::to_string_pretty(&briefing_json)?
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;

    // Charge credits:
    let tokens = (system_prompt.len() + user_prompt.len() + response.len()) / 3;
    let _ = super::parliament::charge_compute_credits(pool, "auditor", tokens as i64).await;

    let raw_json = extract_json_object(&response);
    
    #[derive(serde::Deserialize)]
    struct AuditResult {
        approved: bool,
        critique_notes: String,
    }

    if let Ok(res) = serde_json::from_str::<AuditResult>(&raw_json) {
        if !res.approved {
            tracing::warn!("Briefing failed quality audit: {}", res.critique_notes);
            crate::agent::blackboard::log_feedback(
                pool,
                "auditor",
                "synthesizer",
                Some(&briefing.id),
                &res.critique_notes,
            )
            .await;
        }
    }

    Ok(())
}
