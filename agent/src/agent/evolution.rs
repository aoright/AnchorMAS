use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{DoubaoClient, get_agent_prompt};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvolutionUpdate {
    pub target_role_id: String,
    pub reasoning: String,
    pub new_guidelines: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvolutionResponse {
    pub updates: Vec<EvolutionUpdate>,
}

pub async fn evolve_agents(pool: &SqlitePool, client: &DoubaoClient) -> Result<String> {
    // 1. Fetch recent events with verifier critique notes
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        r#"SELECT category, title, summary, analysis 
           FROM events 
           WHERE analysis LIKE '%[核查备注]%'
           ORDER BY created_at DESC 
           LIMIT 15"#
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        tracing::info!("No events with critique feedback found. Bypassing evolution cycle.");
        return Ok("没有找到任何需要进化的冲突解决案例。".to_string());
    }

    tracing::info!("Found {} events with verifier critiques. Triggering Evolution Agent...", rows.len());

    // Compile the cases for the prompt
    let mut cases = String::new();
    for (i, (category, title, summary, analysis)) in rows.iter().enumerate() {
        cases.push_str(&format!(
            "--- 案例 #{} ---\n分类: {}\n标题: {}\n摘要: {}\n最终分析内容 (含核查备注):\n{}\n\n",
            i + 1, category, title, summary, analysis
        ));
    }

    // 2. Fetch the evolution agent system prompt
    let default_system_prompt = r#"你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。
你的任务是审查事实核查中的“冲突解决日志”（即分析特工结论被事实监督官驳回、并重新修正的事件案例），找出分析特工的共性事实性偏差或逻辑漏洞。
根据这些漏洞，你需要提炼出更具体的“业务过滤与评分守则”（guidelines）或“负面案例提示”，以便注入分析特工或事实监督官的运行指南中。

你的任务：
1. 分析冲突原因，指出分析特工之前夸大或算错分的地方，或者监督官检查不严密的地方。
2. 总结出 1-2 条具体的业务过滤或量化修正守则（例如："对于周大福的非核心零售点变动，严禁打分超过3"，"培育钻石价格下跌不能直接列为 Opportunity"）。
3. 决定这套新规则最适合应用在哪个 Agent 的角色（必须是 analyst_competition|analyst_product|analyst_platform|analyst_regulation|analyst_social|critic 之一）。

请以 JSON 格式输出你的进化建议：
{
  "updates": [
    {
      "target_role_id": "被优化的 Agent 角色ID，如 analyst_competition",
      "reasoning": "为什么需要增加这一条，发现的系统性共性问题是什么",
      "new_guidelines": "新增的业务守则，将被追加到该 Agent 的 guidelines 中（文字应直接简练，50字以内）"
    }
  ]
}"#;

    let system_prompt = get_agent_prompt(pool, "evolution", default_system_prompt).await;

    let user_prompt = format!(
        "以下是近期收集到的冲突解决日志案例：\n\n{}\n请帮我分析并生成相应的进化调整建议。",
        cases
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let raw_json = extract_json_object(&response);
    let parsed: EvolutionResponse = if let Ok(res) = serde_json::from_str::<EvolutionResponse>(&raw_json) {
        res
    } else if let Ok(single_update) = serde_json::from_str::<EvolutionUpdate>(&raw_json) {
        EvolutionResponse {
            updates: vec![single_update],
        }
    } else {
        tracing::error!("Failed to parse Evolution Agent response. Raw response: {}", response);
        return Err(anyhow::anyhow!("进化特工返回数据格式解析失败"));
    };

    let mut result_summary = String::new();
    for update in parsed.updates {
        // Fetch current playbook entry
        let current: Option<(String, i64)> = sqlx::query_as(
            "SELECT guidelines, version FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&update.target_role_id)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();

        if let Some((old_guidelines, version)) = current {
            let old_guidelines_log = old_guidelines.clone();
            let new_guidelines = if old_guidelines.trim().is_empty() {
                update.new_guidelines.clone()
            } else if old_guidelines.contains(&update.new_guidelines) {
                // Avoid duplicating the rule if already present
                old_guidelines
            } else {
                format!("{}\n- {}", old_guidelines, update.new_guidelines)
            };

            // Update guidelines and version
            let new_version = version + 1;
            sqlx::query(
                r#"UPDATE agent_playbook 
                   SET guidelines = ?, version = ?, updated_at = datetime('now')
                   WHERE role_id = ?"#
            )
            .bind(&new_guidelines)
            .bind(new_version)
            .bind(&update.target_role_id)
            .execute(pool)
            .await?;

            // Save to evolution log
            let log_id = Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO agent_evolution_log (id, role_id, old_guidelines, new_guidelines, reasoning)
                   VALUES (?, ?, ?, ?, ?)"#
            )
            .bind(&log_id)
            .bind(&update.target_role_id)
            .bind(&old_guidelines_log)
            .bind(&update.new_guidelines)
            .bind(&update.reasoning)
            .execute(pool)
            .await?;

            let target_name = get_role_name(&update.target_role_id);
            let msg = format!(
                "成功优化 Agent【{}】配置至 v{}！\n优化原因：{}\n新增守则：{}\n\n",
                target_name, new_version, update.reasoning, update.new_guidelines
            );
            result_summary.push_str(&msg);
            tracing::info!(role = %update.target_role_id, version = new_version, "Agent evolved successfully");
        }
    }

    if result_summary.is_empty() {
        Ok("进化特工评估完成：当前系统配置已符合预期，未产生任何突变。".to_string())
    } else {
        Ok(result_summary)
    }
}

fn get_role_name(role_id: &str) -> &'static str {
    match role_id {
        "filter" => "信息过滤特工 (Gatekeeper)",
        "analyst_competition" => "竞争动态分析特工",
        "analyst_product" => "产品趋势分析特工",
        "analyst_platform" => "渠道政策分析特工",
        "analyst_regulation" => "行业合规分析特工",
        "analyst_social" => "社会舆情分析特工",
        "critic" => "事实与逻辑监督官",
        "refiner" => "分析结论修正特工",
        "synthesizer" => "首席战略顾问 (Chief Strategist)",
        _ => "未知特工",
    }
}

fn extract_json_object(text: &str) -> String {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

pub async fn evolve_single_event(
    pool: &SqlitePool,
    client: &DoubaoClient,
    category: &str,
    title: &str,
    summary: &str,
    analysis_with_critique: &str,
) -> Result<Option<EvolutionUpdate>> {
    let case = format!(
        "分类: {}\n标题: {}\n摘要: {}\n最终分析内容 (含核查备注):\n{}\n",
        category, title, summary, analysis_with_critique
    );

    let default_system_prompt = r#"你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。
你的任务是审查事实核查中的“冲突解决日志”（即分析特工结论被事实监督官驳回、并重新修正的事件案例），找出分析特工的共性事实性偏差或逻辑漏洞。
根据这些漏洞，你需要提炼出更具体的“业务过滤与评分守则”（guidelines）或“负面案例提示”，以便注入分析特工或事实监督官的运行指南中。

你的任务：
1. 分析冲突原因，指出分析特工之前夸大或算错分的地方，或者监督官检查不严密的地方。
2. 总结出 1-2 条具体的业务过滤或量化修正守则（例如："对于周大福的非核心零售点变动，严禁打分超过3"，"培育钻石价格下跌不能直接列为 Opportunity"）。
3. 决定这套新规则最适合应用在哪个 Agent 的角色（必须是 analyst_competition|analyst_product|analyst_platform|analyst_regulation|analyst_social|critic 之一）。

请以 JSON 格式输出你的进化建议：
{
  "updates": [
    {
      "target_role_id": "被优化的 Agent 角色ID，如 analyst_competition",
      "reasoning": "为什么需要增加这一条，发现的系统性共性问题是什么",
      "new_guidelines": "新增的业务守则，将被追加到该 Agent 的 guidelines 中（文字应直接简练，50字以内）"
    }
  ]
}"#;

    let system_prompt = get_agent_prompt(pool, "evolution", default_system_prompt).await;

    let user_prompt = format!(
        "以下是近期发生的冲突解决日志案例：\n\n{}\n请帮我分析并生成相应的进化调整建议。",
        case
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let raw_json = extract_json_object(&response);
    
    let parsed: EvolutionResponse = if let Ok(res) = serde_json::from_str::<EvolutionResponse>(&raw_json) {
        res
    } else if let Ok(single_update) = serde_json::from_str::<EvolutionUpdate>(&raw_json) {
        EvolutionResponse {
            updates: vec![single_update],
        }
    } else {
        tracing::error!("Failed to parse Evolution Agent response for single event. Raw response: {}", response);
        return Ok(None);
    };

    if let Some(update) = parsed.updates.first() {
        let current: Option<(String, i64)> = sqlx::query_as(
            "SELECT guidelines, version FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&update.target_role_id)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();

        if let Some((old_guidelines, version)) = current {
            let old_guidelines_log = old_guidelines.clone();
            let new_guidelines = if old_guidelines.trim().is_empty() {
                update.new_guidelines.clone()
            } else if old_guidelines.contains(&update.new_guidelines) {
                old_guidelines
            } else {
                format!("{}\n- {}", old_guidelines, update.new_guidelines)
            };

            let new_version = version + 1;
            sqlx::query(
                r#"UPDATE agent_playbook 
                   SET guidelines = ?, version = ?, updated_at = datetime('now')
                   WHERE role_id = ?"#
            )
            .bind(&new_guidelines)
            .bind(new_version)
            .bind(&update.target_role_id)
            .execute(pool)
            .await?;

            let log_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO agent_evolution_log (id, role_id, old_guidelines, new_guidelines, reasoning)
                   VALUES (?, ?, ?, ?, ?)"#
            )
            .bind(&log_id)
            .bind(&update.target_role_id)
            .bind(&old_guidelines_log)
            .bind(&update.new_guidelines)
            .bind(&update.reasoning)
            .execute(pool)
            .await?;

            tracing::info!(role = %update.target_role_id, version = new_version, "Agent evolved successfully from single event");
            return Ok(Some(update.clone()));
        }
    }

    Ok(None)
}

