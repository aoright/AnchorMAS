use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{DoubaoClient, get_agent_prompt};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvolutionUpdate {
    pub target_role_id: String,
    pub reasoning: String,
    #[serde(deserialize_with = "deserialize_guidelines")]
    pub new_guidelines: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvolutionResponse {
    pub updates: Vec<EvolutionUpdate>,
}

fn deserialize_guidelines<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct GuidelinesVisitor;

    impl<'de> Visitor<'de> for GuidelinesVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or an array of strings")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut list = Vec::new();
            while let Some(elem) = seq.next_element::<String>()? {
                list.push(elem);
            }
            Ok(list.join("\n- "))
        }
    }

    deserializer.deserialize_any(GuidelinesVisitor)
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
        let role_id = match category.as_str() {
            "Competition" => "analyst_competition".to_string(),
            "Product" => "analyst_product".to_string(),
            "Platform" => "analyst_platform".to_string(),
            "Regulation" => "analyst_regulation".to_string(),
            "Social" => "analyst_social".to_string(),
            other => format!("analyst_{}", other.to_lowercase()),
        };
        cases.push_str(&format!(
            "--- 案例 #{} ---\n分类: {} (特工角色ID: {})\n标题: {}\n摘要: {}\n最终分析内容 (含核查备注):\n{}\n\n",
            i + 1, category, role_id, title, summary, analysis
        ));
    }

    // 2. Fetch the evolution agent system prompt
    let default_system_prompt = r#"你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。
你的任务是审查事实核查中的“冲突解决日志”（即分析特工结论被事实监督官驳回、并重新修正的事件案例），找出分析特工的共性事实性偏差或逻辑漏洞。
根据这些漏洞，你需要提炼出更具体的“业务过滤与评分守则”（guidelines）或“负面案例提示”，以便注入分析特工或事实监督官的运行指南中。

你的任务：
1. 分析冲突原因，指出分析特工之前夸大或算错分的地方，或者监督官检查不严密的地方。
2. 总结出 1-2 条具体的业务过滤或量化修正守则（例如："对于周大福的非核心零售点变动，严禁打分超过3"，"培育钻石价格下跌不能直接列为 Opportunity"）。
3. 决定这套新规则最适合应用在哪个 Agent 的角色。这可以是核心特工（analyst_competition|analyst_product|analyst_platform|analyst_regulation|analyst_social|critic），或者是任何在冲突案例中出现的自定义分析特工角色（格式如 analyst_xxxx，请从案例数据提供的“特工角色ID”字段中直接获取，不要输出不存在的ID）。

请以 JSON 格式输出你的进化建议：
{
  "updates": [
    {
      "target_role_id": "被优化的 Agent 角色ID，如 analyst_competition 或 analyst_policyincentive",
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

            let target_name: String = sqlx::query_scalar(
                "SELECT name FROM agent_playbook WHERE role_id = ?"
            )
            .bind(&update.target_role_id)
            .fetch_optional(pool)
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| get_role_name(&update.target_role_id));
            let msg = format!(
                "成功优化 Agent【{}】配置至 v{}！\n优化原因：{}\n新增守则：{}\n\n",
                target_name, new_version, update.reasoning, update.new_guidelines
            );
            result_summary.push_str(&msg);
        }
    }

    if result_summary.is_empty() {
        Ok("进化特工评估了冲突日志，但没有产生任何规则更新。".to_string())
    } else {
        Ok(result_summary)
    }
}

pub async fn evolve_from_feedback_log(
    pool: &SqlitePool,
    client: &DoubaoClient,
) -> Result<Vec<EvolutionUpdate>> {
    // 1. Fetch all distinct receiver roles that have unresolved feedback
    let receiver_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT receiver FROM agent_feedback_log WHERE is_resolved = 0"
    )
    .fetch_all(pool)
    .await?;

    if receiver_rows.is_empty() {
        return Ok(Vec::new());
    }

    tracing::info!("Found unresolved feedback for receiver roles: {:?}. Triggering Targeted Co-Evolution...", receiver_rows);

    let default_system_prompt = r#"你是一个高级方法论专家与智能进化特工（角色：【多特工协作进化导师】）。
你的任务是根据系统内的协作反馈日志，针对指定的 Agent 角色（role_id）优化其“业务进化守则”（guidelines）。
你将获得该 Agent 当前已有的进化守则，以及其他特工在合作中对其提出的反馈意见与批评。

你的任务：
1. 分析反馈意见，指出该 Agent 的不足与此次优化的必要性。
2. 结合该 Agent 当前已有的进化守则，合并、重构或新增守则，输出一份合并更新后的、结构化的【完整进化守则】。
3. 新进化守则必须是 Markdown 列表格式，每条规则应简短直接且具备可操作性（不超过50字）。
4. 去除重复的或相互矛盾 of 规则，确保整体守则条理清晰，总篇幅控制在 500 字以内。

请以 JSON 格式输出你的进化建议：
{
  "target_role_id": "被优化的 Agent 角色ID，如 filter | critic | analyst_competition | refiner | synthesizer 等",
  "reasoning": "分析冲突原因，指出该 Agent 的不足与此次优化的必要性",
  "new_guidelines": "合并更新后的完整进化守则（必须为 Markdown 列表格式，包含所有仍有效的旧规则与新增规则）"
}"#;

    let system_prompt = get_agent_prompt(pool, "evolution", default_system_prompt).await;
    let mut applied_updates = Vec::new();

    // 2. Process each receiver's feedback individually
    for (receiver,) in receiver_rows {
        // Fetch the oldest 10 unresolved feedback entries targeting this receiver
        let feedback_list = sqlx::query_as::<_, (String, String, Option<String>, String)>(
            r#"SELECT id, sender, event_id, feedback 
               FROM agent_feedback_log 
               WHERE receiver = ? AND is_resolved = 0
               ORDER BY created_at ASC
               LIMIT 10"#
        )
        .bind(&receiver)
        .fetch_all(pool)
        .await?;

        if feedback_list.is_empty() {
            continue;
        }

        // Fetch current playbook entry for this receiver
        let current_playbook: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT name, guidelines, version FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&receiver)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();

        let (name, old_guidelines, version) = match current_playbook {
            Some(data) => data,
            None => {
                tracing::warn!(role = %receiver, "Feedback received for unknown agent role. Skipping evolution.");
                // Mark feedback entries as resolved to prevent deadlock
                for (id, _, _, _) in &feedback_list {
                    let _ = sqlx::query("UPDATE agent_feedback_log SET is_resolved = 1 WHERE id = ?")
                        .bind(id)
                        .execute(pool)
                        .await;
                }
                continue;
            }
        };

        // Format complaints for the target receiver
        let mut logs_str = String::new();
        for (i, (_, sender, event_id, feedback)) in feedback_list.iter().enumerate() {
            let event_context = event_id.as_deref().unwrap_or("N/A");
            logs_str.push_str(&format!(
                "--- 反馈记录 #{} ---\n发送方: {}\n关联事件ID: {}\n反馈意见:\n{}\n\n",
                i + 1, sender, event_context, feedback
            ));
        }

        let user_prompt = format!(
            "目标 Agent 角色ID: {}\n该 Agent 角色名称: {}\n\n【该 Agent 当前的进化守则】：\n{}\n\n【针对该 Agent 的协作反馈意见列表】：\n{}\n\n请分析上述反馈，结合现有的进化守则，为该 Agent 优化并输出一份最新合并的、结构化的【完整进化守则】。",
            receiver,
            name,
            if old_guidelines.trim().is_empty() { "暂无" } else { &old_guidelines },
            logs_str
        );

        tracing::info!(role = %receiver, "Invoking LLM to evolve playbook guidelines...");
        let response = match client.chat(&system_prompt, &user_prompt, true).await {
            Ok(res) => res,
            Err(e) => {
                tracing::error!(role = %receiver, "LLM evolution request failed: {}. Skipping this role.", e);
                continue;
            }
        };

        let raw_json = extract_json_object(&response);
        let update: EvolutionUpdate = match serde_json::from_str::<EvolutionUpdate>(&raw_json) {
            Ok(u) => u,
            Err(_) => {
                if let Ok(res) = serde_json::from_str::<EvolutionResponse>(&raw_json) {
                    if let Some(u) = res.updates.into_iter().next() {
                        u
                    } else {
                        tracing::error!(role = %receiver, "Empty updates list from Evolution Agent. Raw: {}", response);
                        continue;
                    }
                } else {
                    tracing::error!(role = %receiver, "Failed to parse Evolution Agent response. Raw: {}", response);
                    continue;
                }
            }
        };

        let new_guidelines = update.new_guidelines.clone();
        let new_version = version + 1;

        // Save updated guidelines to playbook
        if let Err(e) = sqlx::query(
            r#"UPDATE agent_playbook 
               SET guidelines = ?, version = ?, updated_at = datetime('now')
               WHERE role_id = ?"#
        )
        .bind(&new_guidelines)
        .bind(new_version)
        .bind(&receiver)
        .execute(pool)
        .await {
            tracing::error!(role = %receiver, "Failed to update agent playbook: {}", e);
            continue;
        }

        // Log evolution step
        let log_id = Uuid::new_v4().to_string();
        let _ = sqlx::query(
            r#"INSERT INTO agent_evolution_log (id, role_id, old_guidelines, new_guidelines, reasoning)
               VALUES (?, ?, ?, ?, ?)"#
        )
        .bind(&log_id)
        .bind(&receiver)
        .bind(&old_guidelines)
        .bind(&new_guidelines)
        .bind(&update.reasoning)
        .execute(pool)
        .await;

        // Meta-validation on evolution engine's own output quality
        let mut evolution_feedback = Vec::new();
        if !new_guidelines.trim().starts_with('-') && !new_guidelines.trim().is_empty() {
            evolution_feedback.push("生成的新进化守则不是以 '-' 开头的 Markdown 列表格式，请修改为标准列表。");
        }
        if new_guidelines.chars().count() > 500 {
            evolution_feedback.push("进化守则文字超过500字限制，过于冗长，请压缩。");
        }
        if new_guidelines.contains("- -") {
            evolution_feedback.push("进化守则中存在不正确的嵌套或重复的减号列表符。");
        }
        if !evolution_feedback.is_empty() {
            let feedback_msg = evolution_feedback.join(" ");
            let fb_id = Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO agent_feedback_log (id, sender, receiver, feedback) VALUES (?, 'validator', 'evolution', ?)"
            )
            .bind(&fb_id)
            .bind(&feedback_msg)
            .execute(pool)
            .await;
            tracing::info!(role = %receiver, "Evolution output format validation failed, logged feedback to evolution role");
        }

        tracing::info!(role = %receiver, version = new_version, "Agent evolved successfully from targeted feedback logs");
        applied_updates.push(update);

        // Mark this group's feedback entries as resolved
        for (id, _, _, _) in &feedback_list {
            let _ = sqlx::query("UPDATE agent_feedback_log SET is_resolved = 1 WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await;
        }
    }

    Ok(applied_updates)
}

fn extract_json_object(text: &str) -> String {
    super::extract_json_object(text)
}

fn get_role_name(role_id: &str) -> String {
    match role_id {
        "filter" => "信息过滤特工 (Gatekeeper)".to_string(),
        "analyst_competition" => "竞争动态分析特工".to_string(),
        "analyst_product" => "产品趋势分析特工".to_string(),
        "analyst_platform" => "渠道政策分析特工".to_string(),
        "analyst_regulation" => "行业合规分析特工".to_string(),
        "analyst_social" => "社会舆情分析特工".to_string(),
        "critic" => "事实与逻辑监督官".to_string(),
        "refiner" => "分析结论修正特工".to_string(),
        "synthesizer" => "首席战略顾问 (Chief Strategist)".to_string(),
        "evidence_evaluator" => "证据链评估特工".to_string(),
        "designer" => "智能体设计专家 (Meta-Agent Designer)".to_string(),
        "evolution" => "进化指导特工 (Methodology Director)".to_string(),
        other => {
            if other.starts_with("analyst_") {
                format!("{} 细分分析特工", &other[8..].to_uppercase())
            } else {
                other.to_string()
            }
        }
    }
}
