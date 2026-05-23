use anyhow::{Context, Result};

use super::{AnalyzedEvent, DoubaoClient, FilteredEvent};

/// Analyze filtered events with specialized analyst prompts per category.
/// Runs analysis for each category using domain-specific expertise.
pub async fn analyze_events(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    events: &[FilteredEvent],
) -> Result<Vec<AnalyzedEvent>> {
    let mut analyzed = Vec::new();

    // Group events by category
    let categories = ["Competition", "Product", "Social", "Platform", "Regulation"];

    for category in &categories {
        let category_events: Vec<&FilteredEvent> = events
            .iter()
            .filter(|e| e.category.eq_ignore_ascii_case(category))
            .collect();

        if category_events.is_empty() {
            continue;
        }

        let role_id = match *category {
            "Competition" => "analyst_competition",
            "Product" => "analyst_product",
            "Platform" => "analyst_platform",
            "Regulation" => "analyst_regulation",
            _ => "analyst_social",
        };
        let default_prompt = get_analyst_prompt(category);
        let system_prompt = super::get_agent_prompt(pool, role_id, &default_prompt).await;

        // Process in batches of 20 to avoid token limits
        for batch in category_events.chunks(20) {
            match analyze_category(client, &system_prompt, batch).await {
                Ok(mut results) => {
                    tracing::info!(
                        category = category,
                        count = results.len(),
                        "Analyzed category batch"
                    );
                    analyzed.append(&mut results);
                }
                Err(e) => {
                    tracing::error!(
                        category = category,
                        error = %e,
                        "Failed to analyze category batch, using defaults"
                    );
                    // Fall back: convert FilteredEvents to AnalyzedEvents with default scores
                    for event in batch {
                        analyzed.push(AnalyzedEvent {
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
                        });
                    }
                }
            }
            // Small delay between batches to avoid rate limiting
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Ok(analyzed)
}

fn get_analyst_prompt(category: &str) -> String {
    let scoring_rubric = r#"
【打分与分类量化标准】
1. impact_type（影响类型）：
   - Opportunity: 事件带来明显的业务增长空间、新渠道红利或降低成本的机会。
   - Risk: 事件可能导致客户流失、成本上升、销售额受损或面临罚款等潜在威胁。
   - Attention: 事件对行业有影响但对企业自身暂无直接机会/风险，需保持观察。

2. severity（严重度量化，1-5分）：
   - 1：仅波及小范围零售点或小众设计师款式，对企业全局财务、商誉或业务大盘无实质影响。
   - 3：直接波及单一国家或地区的主流渠道/主打单品，需要地区业务线进行战略防御或调整。
   - 5：波及全球供应链、面临跨国巨额合规指控与制裁，或者竞争对手形成颠覆性技术/替代品。

3. urgency（紧急度量化，1-5分）：
   - 1：事件对应的趋势或规则处于公开提案/酝酿期，预计1年以上才有实质落地。
   - 3：新规划、竞争策略或变动已对外公布，预计本季度内将对市场产生显著冲击。
   - 5：危机正在发生，或者规则要求即刻/72小时内做出合规调整，不应对将面临巨额损失。

4. confidence（置信度量化，1-5分）：
   - 1：仅凭单一自媒体爆料、社交网络传言或猜测，缺乏交叉佐证。
   - 3：有主流财经、科技或行业垂直媒体的交叉专题报道，但无当事方公告。
   - 5：政府官方通告、跨国组织声明、上市公司年报/财报/官方通告、法庭裁决等无可争议的硬性事实。
"#;

    match category {
        "Competition" => format!(
            r#"你是一位珠宝行业竞争情报分析师。分析以下竞争动态事件，评估其对品牌型珠宝企业的机会与风险。

重点关注：
- 头部品牌（周大福、周生生、老凤祥、Pandora、Tiffany等）的战略动作
- 新入局者和跨界竞争者
- 价格战和促销策略变化
- 市场份额变动信号
{}
对每个事件，请返回JSON数组，每个元素包含：
{{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}}

只返回JSON数组。"#,
            scoring_rubric
        ),

        "Product" => format!(
            r#"你是珠宝产品趋势分析师。关注培育钻石(LGD)、足金国潮、K金、彩宝、珍珠等材质趋势，分析以下产品相关事件。

重点关注：
- 培育钻石vs天然钻石市场演变
- 金价波动对黄金首饰消费的影响
- 新材质、新工艺的市场接受度
- 消费者偏好转变信号
{}
对每个事件，请返回JSON数组，每个元素包含：
{{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}}

只返回JSON数组。"#,
            scoring_rubric
        ),

        "Platform" => format!(
            r#"你是跨境电商渠道分析师。分析电商平台政策变动对珠宝卖家的影响。

重点关注：
- 天猫/京东/拼多多/抖音电商的珠宝品类政策
- 跨境平台（Amazon、Shopee、Lazada）的合规要求
- 直播电商趋势和平台算法变化
- 平台佣金和流量政策调整
{}
对每个事件，请返回JSON数组，每个元素包含：
{{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}}

只返回JSON数组。"#,
            scoring_rubric
        ),

        "Regulation" => format!(
            r#"你是国际珠宝合规法务分析师。关注FTC培育钻标签规则、金伯利进程、各国贵金属成色标记(Hallmark)法规，分析以下法规政策事件。

重点关注：
- 各国珠宝进出口关税变化
- 产品标签和认证要求更新
- 消费者保护法规
- 行业自律标准变动
{}
对每个事件，请返回JSON数组，每个元素包含：
{{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}}

只返回JSON数组。"#,
            scoring_rubric
        ),

        // "Social" and any other category
        _ => format!(
            r#"你是珠宝行业社会舆情分析师。分析以下社会舆情事件对珠宝行业的潜在影响。

重点关注：
- 消费观念变化（悦己消费、理性消费）
- 社交媒体话题和KOL影响力
- 婚庆市场变化
- 可持续发展和ESG相关舆论
{}
对每个事件，请返回JSON数组，每个元素包含：
{{
  "id": "事件ID",
  "impact_type": "Opportunity|Risk|Attention",
  "severity": 1-5,
  "urgency": 1-5,
  "confidence": 1-5,
  "analysis": "详细分析（100字以内）"
}}

只返回JSON数组。"#,
            scoring_rubric
        ),
    }
}


async fn analyze_category(
    client: &DoubaoClient,
    system_prompt: &str,
    events: &[&FilteredEvent],
) -> Result<Vec<AnalyzedEvent>> {
    let events_json: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "title": e.title,
                "summary": e.summary,
                "market": e.market,
                "category": e.category,
            })
        })
        .collect();

    let user_prompt = format!(
        "请分析以下{}个事件：\n\n{}",
        events.len(),
        serde_json::to_string_pretty(&events_json)?
    );

    let response = client.chat(system_prompt, &user_prompt, true).await?;
    let analysis_results = parse_analysis_results(&response, events)?;

    Ok(analysis_results)
}

fn parse_analysis_results(
    response: &str,
    original_events: &[&FilteredEvent],
) -> Result<Vec<AnalyzedEvent>> {
    let json_str = extract_json_array(response);
    let items: Vec<serde_json::Value> = serde_json::from_str(&json_str)
        .context("Failed to parse analysis response as JSON array")?;

    // Build a lookup map from original events
    let event_map: std::collections::HashMap<&str, &FilteredEvent> = original_events
        .iter()
        .map(|e| (e.id.as_str(), *e))
        .collect();

    let mut analyzed = Vec::new();

    for item in &items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Find the original event, or use the first one as fallback
        let original = event_map
            .get(id)
            .copied()
            .or_else(|| original_events.first().copied());

        if let Some(orig) = original {
            let impact_type = item
                .get("impact_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Attention")
                .to_string();
            let severity = item
                .get("severity")
                .and_then(|v| v.as_i64())
                .unwrap_or(2) as i32;
            let urgency = item
                .get("urgency")
                .and_then(|v| v.as_i64())
                .unwrap_or(2) as i32;
            let confidence = item
                .get("confidence")
                .and_then(|v| v.as_i64())
                .unwrap_or(2) as i32;
            let analysis = item
                .get("analysis")
                .and_then(|v| v.as_str())
                .unwrap_or("No analysis provided.")
                .to_string();

            analyzed.push(AnalyzedEvent {
                id: orig.id.clone(),
                market: orig.market.clone(),
                category: orig.category.clone(),
                title: orig.title.clone(),
                summary: orig.summary.clone(),
                source_urls: orig.source_urls.clone(),
                impact_type,
                severity: severity.clamp(1, 5),
                urgency: urgency.clamp(1, 5),
                confidence: confidence.clamp(1, 5),
                analysis,
            });
        }
    }

    // For any events not covered by the model response, add defaults
    for event in original_events {
        if !analyzed.iter().any(|a| a.id == event.id) {
            analyzed.push(AnalyzedEvent {
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
                analysis: "Model did not provide analysis for this event.".to_string(),
            });
        }
    }

    Ok(analyzed)
}

fn extract_json_array(text: &str) -> String {
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

pub async fn analyze_single_event(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    event: &FilteredEvent,
) -> Result<AnalyzedEvent> {
    let role_id = match event.category.as_str() {
        "Competition" => "analyst_competition",
        "Product" => "analyst_product",
        "Platform" => "analyst_platform",
        "Regulation" => "analyst_regulation",
        _ => "analyst_social",
    };
    let default_prompt = get_analyst_prompt(&event.category);
    let system_prompt = super::get_agent_prompt(pool, role_id, &default_prompt).await;

    let events_json = vec![serde_json::json!({
        "id": event.id,
        "title": event.title,
        "summary": event.summary,
        "market": event.market,
        "category": event.category,
    })];

    let user_prompt = format!(
        "请分析以下1个事件：\n\n{}",
        serde_json::to_string_pretty(&events_json)?
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let mut analysis_results = parse_analysis_results(&response, &[event])?;
    if let Some(res) = analysis_results.pop() {
        Ok(res)
    } else {
        anyhow::bail!("No analysis results returned")
    }
}

pub async fn peer_review_event(
    client: &DoubaoClient,
    pool: &sqlx::SqlitePool,
    event: &AnalyzedEvent,
    peer_role_id: &str,
) -> Result<String> {
    let peer_role_name = match peer_role_id {
        "analyst_competition" => "竞争动态分析特工",
        "analyst_product" => "产品趋势分析特工",
        "analyst_platform" => "渠道政策分析特工",
        "analyst_regulation" => "行业合规分析特工",
        _ => "社会舆情分析特工",
    };
    
    let default_prompt = format!(
        "你是一个高级珠宝行业分析师（角色：【{}】）。\n\
         你需要对另一位分析师关于【{}】领域的报告进行同行评审（Peer Review）。\n\
         请根据原始事件摘要和主分析师的结论，提出你的跨领域见解、补充意见或事实逻辑质疑。\n\
         字数控制在100字以内，内容务必针对性强且客观。\n\
         不要返回任何 Markdown 格式，只返回纯文本点评。",
        peer_role_name, event.category
    );

    let system_prompt = super::get_agent_prompt(pool, peer_role_id, &default_prompt).await;

    let user_prompt = format!(
        "事件标题: {}\n摘要: {}\n主分析师结论 (影响类型: {}, 严重度: {}, 紧急度: {}, 置信度: {}):\n{}",
        event.title, event.summary, event.impact_type, event.severity, event.urgency, event.confidence, event.analysis
    );

    let response = client.chat(&system_prompt, &user_prompt, false).await?;
    Ok(response.trim().to_string())
}

