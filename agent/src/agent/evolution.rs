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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddedRule {
    pub target_role_id: String,
    pub content: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeprecatedRule {
    pub rule_id: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvolveCrudResponse {
    pub added_rules: Vec<AddedRule>,
    pub deprecated_rule_ids: Vec<DeprecatedRule>,
}

pub async fn auto_update_regression_suite(pool: &SqlitePool) -> Result<()> {
    tracing::info!("Updating regression test suite with top verified historical events...");

    let top_events = sqlx::query_as::<_, (String, String, String, String, String)>(
        r#"SELECT id, category, title, summary, analysis 
           FROM events
           WHERE analysis NOT LIKE '%[核查警告]%'
             AND confidence >= 4
             AND analysis <> ''
             AND summary <> ''
           ORDER BY created_at DESC
           LIMIT 15"#
    )
    .fetch_all(pool)
    .await?;

    for (event_id, category, title, summary, analysis) in top_events {
        sqlx::query(
            r#"INSERT OR REPLACE INTO regression_test_suite (event_id, category, title, summary, analysis, created_at)
               VALUES (?, ?, ?, ?, ?, datetime('now'))"#
        )
        .bind(&event_id)
        .bind(&category)
        .bind(&title)
        .bind(&summary)
        .bind(&analysis)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn compile_guidelines(pool: &SqlitePool, role_id: &str) -> Result<String> {
    let rules: Vec<(String,)> = sqlx::query_as(
        "SELECT content FROM agent_playbook_rules WHERE role_id = ? AND status = 'active' ORDER BY created_at ASC"
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;

    if rules.is_empty() {
        return Ok(String::new());
    }

    let compiled = rules
        .into_iter()
        .map(|(content,)| {
            let trimmed = content.trim();
            if trimmed.starts_with("- ") {
                trimmed.to_string()
            } else if trimmed.starts_with('-') {
                format!("- {}", trimmed[1..].trim())
            } else {
                format!("- {}", trimmed)
            }
        })
        .collect::<Vec<String>>()
        .join("\n");

    Ok(compiled)
}

pub async fn verify_regression_sandbox(
    pool: &SqlitePool,
    client: &DoubaoClient,
    role_id: &str,
    proposed_guidelines: &str,
) -> Result<bool> {
    tracing::info!(role = %role_id, "Running regression test suite sandbox...");

    // Determine category for database filtering
    let category = match role_id {
        "analyst_competition" => "Competition",
        "analyst_product" => "Product",
        "analyst_platform" => "Platform",
        "analyst_regulation" => "Regulation",
        "analyst_social" => "Social",
        other => {
            if other.starts_with("analyst_") {
                &other[8..]
            } else {
                ""
            }
        }
    };

    // Fetch test cases from regression_test_suite
    let test_cases = if category.is_empty() {
        sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT event_id, title, summary, analysis FROM regression_test_suite ORDER BY created_at DESC LIMIT 5"
        )
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT event_id, title, summary, analysis FROM regression_test_suite WHERE category LIKE ? ORDER BY created_at DESC LIMIT 5"
        )
        .bind(format!("%{}%", category))
        .fetch_all(pool)
        .await?
    };

    if test_cases.is_empty() {
        tracing::info!("No matching historical regression test cases found. Sandbox checks bypassed (Pass).");
        return Ok(true);
    }

    tracing::info!("Running {} regression test cases in sandbox...", test_cases.len());

    // Fetch base system prompt for the role
    let system_prompt: String = sqlx::query_scalar(
        "SELECT system_prompt FROM agent_playbook WHERE role_id = ?"
    )
    .bind(role_id)
    .fetch_one(pool)
    .await?;

    let proposed_system_prompt = if proposed_guidelines.is_empty() {
        system_prompt.clone()
    } else {
        format!("{}\n\n【动态追加的业务进化守则】：\n{}", system_prompt, proposed_guidelines)
    };

    let mut passed_count = 0;
    let total_count = test_cases.len();

    for (event_id, title, summary, old_analysis) in &test_cases {
        // Run analysis in sandbox
        let user_prompt = format!(
            "请分析以下1个事件：\n\n{}",
            serde_json::to_string_pretty(&vec![serde_json::json!({
                "id": event_id,
                "title": title,
                "summary": summary,
                "market": "Global",
                "category": category,
            })])?
        );

        let new_analysis_response = match client.chat(&proposed_system_prompt, &user_prompt, true).await {
            Ok(res) => res,
            Err(e) => {
                tracing::warn!("Sandbox analysis failed: {}. Counting as regression failure.", e);
                continue;
            }
        };

        // Extract the actual analysis field from JSON response
        let mut new_analysis_text = String::new();
        let cleaned_json = super::extract_json_array_or_object(&new_analysis_response);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned_json) {
            if let Some(arr) = val.as_array() {
                if let Some(first) = arr.first() {
                    new_analysis_text = first.get("analysis").and_then(|a| a.as_str()).unwrap_or("").to_string();
                }
            } else {
                new_analysis_text = val.get("analysis").and_then(|a| a.as_str()).unwrap_or("").to_string();
            }
        }
        if new_analysis_text.is_empty() {
            new_analysis_text = new_analysis_response.clone();
        }

        // Fast path: if identical, score is automatically 5/5
        let score = if new_analysis_text.trim() == old_analysis.trim() {
            tracing::info!(event_id = %event_id, "New analysis is identical to old analysis. Sandbox check PASSED (Fast Path).");
            5
        } else {
            // Call Critic to evaluate
            let critic_eval_prompt = format!(
                "你是一个高级多智能体质量评估官（角色：【进化沙箱回测专家】）。\n\
                 我们需要评估某个智能体在注入新业务守则（Guidelines）后，其分析质量是否发生了退化（Regression）。\n\n\
                 【原始事件信息】：\n\
                 标题: {}\n\
                 摘要: {}\n\n\
                 【原分析结论（已知高质量）】：\n\
                 {}\n\n\
                 【新守则下的新分析结论】：\n\
                 {}\n\n\
                 请对比新旧分析结论：\n\
                 1. 评估新结论是否符合事实，是否发生了关键事实性遗漏或过度脑补。\n\
                 2. 评估新结论是否偏离了原有的深度，或者打分逻辑是否发生自相矛盾。\n\
                 3. 给出自洽性打分：1-5分（5分为极其优秀，新分析完全继承或超越了旧分析；3分为基本可接受，无退化；2分及以下表示发生了严重退化、事实错误或核心信息丢失）。\n\n\
                 请直接以 JSON 格式输出评估结果，例如：\n\
                 {{ \n\
                   \"score\": 3, \n\
                   \"reason\": \"解释你的评估理由\" \n\
                 }}",
                title, summary, old_analysis, new_analysis_text
            );

            let critic_prompt = "你是一个客观理性的沙箱回测审查官。";
            if let Ok(res) = client.chat(critic_prompt, &critic_eval_prompt, true).await {
                let json_str = super::extract_json_object(&res);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    val.get("score").and_then(|s| s.as_i64()).unwrap_or(2) as i32
                } else {
                    2
                }
            } else {
                2
            }
        };

        if score >= 3 {
            passed_count += 1;
            tracing::info!(event_id = %event_id, score = %score, "Sandbox test case PASSED");
        } else {
            tracing::warn!(event_id = %event_id, score = %score, "Sandbox test case FAILED");
        }
    }

    let pass_rate = passed_count as f64 / total_count as f64;
    let passed = pass_rate >= 0.95;

    tracing::info!(
        passed = %passed,
        pass_rate = %format!("{:.1}%", pass_rate * 100.0),
        "Regression sandbox results: {}/{} passed",
        passed_count,
        total_count
    );

    Ok(passed)
}

pub async fn evolve_agents(pool: &SqlitePool, client: &DoubaoClient) -> Result<String> {
    // Update regression suite first
    let _ = auto_update_regression_suite(pool).await;

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

    // Map categories to role_ids to fetch active rules
    let mut involved_roles = std::collections::HashSet::new();
    for (category, _, _, _) in &rows {
        let role_id = match category.as_str() {
            "Competition" => "analyst_competition".to_string(),
            "Product" => "analyst_product".to_string(),
            "Platform" => "analyst_platform".to_string(),
            "Regulation" => "analyst_regulation".to_string(),
            "Social" => "analyst_social".to_string(),
            other => format!("analyst_{}", other.to_lowercase()),
        };
        involved_roles.insert(role_id);
    }

    let mut active_rules_context = String::new();
    for role_id in &involved_roles {
        let rules: Vec<(String, String)> = sqlx::query_as(
            "SELECT rule_id, content FROM agent_playbook_rules WHERE role_id = ? AND status = 'active'"
        )
        .bind(role_id)
        .fetch_all(pool)
        .await?;

        if !rules.is_empty() {
            active_rules_context.push_str(&format!("【角色 ID: {} 的当前活跃守则】:\n", role_id));
            for (rule_id, content) in rules {
                active_rules_context.push_str(&format!("- ID: {} | 内容: {}\n", rule_id, content));
            }
            active_rules_context.push_str("\n");
        }
    }

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
你的任务是审查事实核查中的“冲突解决日志”，找出分析特工的共性偏差或逻辑漏洞。
根据这些漏洞，你需要对特工的业务守则进行增删改查（CRUD）维护：
1. 【新增规则】：如果发现现有规则有漏洞，或者需要新增针对特定场景的量化守则（字数限制在50字以内）。
2. 【废弃规则】：如果发现新规则与某条现有的活跃规则（包含在输入列表中，有唯一的 UUID）存在冲突、重复，或者现有规则已被证实不合理，请废弃它。

请以 JSON 格式输出你的决策，禁止包含任何外层包装或 markdown 标记：
{
  "added_rules": [
    {
      "target_role_id": "被优化的 Agent 角色ID，如 analyst_competition",
      "content": "新增的业务守则内容（文字应直接简练，50字以内）",
      "reasoning": "为什么需要增加这一条"
    }
  ],
  "deprecated_rule_ids": [
    {
      "rule_id": "要废弃的现有规则的 UUID",
      "reasoning": "废弃该规则的原因"
    }
  ]
}"#;

    let system_prompt = get_agent_prompt(pool, "evolution", default_system_prompt).await;

    let user_prompt = format!(
        "以下是当前活跃的特工规则：\n\n{}\n\n以下是近期收集到的冲突解决日志案例：\n\n{}\n请帮我分析并生成相应的进化调整建议。",
        active_rules_context, cases
    );

    let response = client.chat(&system_prompt, &user_prompt, true).await?;
    let raw_json = extract_json_object(&response);
    let parsed: EvolveCrudResponse = match serde_json::from_str(&raw_json) {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Failed to parse EvolveCrudResponse: {}. Raw: {}", e, response);
            return Err(anyhow::anyhow!("进化特工返回数据格式解析失败"));
        }
    };

    // Group changes by role_id to validate them in sandbox
    let mut role_changes: std::collections::HashMap<String, (Vec<AddedRule>, Vec<DeprecatedRule>)> = std::collections::HashMap::new();
    for added in parsed.added_rules {
        role_changes.entry(added.target_role_id.clone()).or_default().0.push(added);
    }
    for dep in parsed.deprecated_rule_ids {
        let role_opt: Option<(String,)> = sqlx::query_as(
            "SELECT role_id FROM agent_playbook_rules WHERE rule_id = ?"
        )
        .bind(&dep.rule_id)
        .fetch_optional(pool)
        .await?;

        if let Some((role_id,)) = role_opt {
            role_changes.entry(role_id).or_default().1.push(dep);
        }
    }

    let mut result_summary = String::new();

    // Validate and apply for each role
    for (role_id, (added_rules, deprecated_rules)) in role_changes {
        let current_rules: Vec<String> = sqlx::query_as(
            "SELECT content FROM agent_playbook_rules WHERE role_id = ? AND status = 'active'"
        )
        .bind(&role_id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(c,)| c)
        .collect();

        // Simulate deprecations and additions in memory
        let mut simulated_rules = current_rules;
        for dep in &deprecated_rules {
            let content_opt: Option<(String,)> = sqlx::query_as(
                "SELECT content FROM agent_playbook_rules WHERE rule_id = ?"
            )
            .bind(&dep.rule_id)
            .fetch_optional(pool)
            .await?;
            if let Some((content,)) = content_opt {
                simulated_rules.retain(|r| r != &content);
            }
        }
        for add in &added_rules {
            if !simulated_rules.contains(&add.content) {
                simulated_rules.push(add.content.clone());
            }
        }

        let proposed_guidelines = simulated_rules
            .iter()
            .map(|r| format!("- {}", r.trim()))
            .collect::<Vec<String>>()
            .join("\n");

        // Run regression sandbox
        let sandbox_passed = verify_regression_sandbox(pool, client, &role_id, &proposed_guidelines).await?;

        if !sandbox_passed {
            tracing::warn!(role = %role_id, "Evolution sandbox failed! Triggering Rollback (changes discarded).");
            result_summary.push_str(&format!(
                "特工【{}】沙箱验证未通过（检测到分析退化），已触发 Rollback 撤销本次优化。\n\n",
                get_role_name(&role_id)
            ));
            continue;
        }

        // Apply changes to database
        let current_version: i64 = sqlx::query_scalar(
            "SELECT version FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&role_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or(1);
        let new_version = current_version + 1;

        // Perform deprecations
        for dep in &deprecated_rules {
            sqlx::query(
                "UPDATE agent_playbook_rules SET status = 'deprecated', reasoning = ? WHERE rule_id = ?"
            )
            .bind(&dep.reasoning)
            .bind(&dep.rule_id)
            .execute(pool)
            .await?;
        }

        // Perform additions
        for add in &added_rules {
            let rule_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO agent_playbook_rules (rule_id, role_id, content, reasoning, version, status) VALUES (?, ?, ?, ?, ?, 'active')"
            )
            .bind(&rule_id)
            .bind(&role_id)
            .bind(&add.content)
            .bind(&add.reasoning)
            .bind(new_version)
            .execute(pool)
            .await?;

            // Save to evolution log for tracking
            let log_id = Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO agent_evolution_log (id, role_id, old_guidelines, new_guidelines, reasoning)
                   VALUES (?, ?, '', ?, ?)"#
            )
            .bind(&log_id)
            .bind(&role_id)
            .bind(&add.content)
            .bind(&add.reasoning)
            .execute(pool)
            .await?;
        }

        // Recompile guidelines and write to agent_playbook
        let compiled = compile_guidelines(pool, &role_id).await?;
        sqlx::query(
            "UPDATE agent_playbook SET guidelines = ?, version = ?, updated_at = datetime('now') WHERE role_id = ?"
        )
        .bind(&compiled)
        .bind(new_version)
        .bind(&role_id)
        .execute(pool)
        .await?;

        let target_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&role_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_else(|| get_role_name(&role_id));

        let msg = format!(
            "成功优化 Agent【{}】配置至 v{}！\n新活跃守则：\n{}\n\n",
            target_name, new_version, compiled
        );
        result_summary.push_str(&msg);
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
    // Run auto update regression suite
    let _ = auto_update_regression_suite(pool).await;

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
4. 去除重复的或相互矛盾的规则，确保整体守则条理清晰，总篇幅控制在 500 字以内。

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
        let update: EvolutionUpdate = match serde_json::from_str(&raw_json) {
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

        // Sandbox check before applying feedback guidelines!
        let sandbox_passed = verify_regression_sandbox(pool, client, &receiver, &new_guidelines).await?;
        if !sandbox_passed {
            tracing::warn!(role = %receiver, "Evolution sandbox check failed for feedback update! Triggering Rollback.");
            // We resolve the feedback entries anyway to avoid loop lock, but do not update rules
            for (id, _, _, _) in &feedback_list {
                let _ = sqlx::query("UPDATE agent_feedback_log SET is_resolved = 1 WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await;
            }
            continue;
        }

        let new_version = version + 1;

        // Save updated guidelines to rules DB and compile back
        // 1. Deprecate all old active rules for receiver
        let _ = sqlx::query("UPDATE agent_playbook_rules SET status = 'deprecated' WHERE role_id = ? AND status = 'active'")
            .bind(&receiver)
            .execute(pool)
            .await;

        // 2. Insert new lines into rules DB
        for line in new_guidelines.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let content = if trimmed.starts_with("- ") {
                &trimmed[2..]
            } else if trimmed.starts_with('-') {
                &trimmed[1..]
            } else {
                trimmed
            };
            if content.is_empty() {
                continue;
            }

            let rule_id = Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO agent_playbook_rules (rule_id, role_id, content, reasoning, version, status) VALUES (?, ?, ?, ?, ?, 'active')"
            )
            .bind(&rule_id)
            .bind(&receiver)
            .bind(content)
            .bind("Evolved from feedback logs")
            .bind(new_version)
            .execute(pool)
            .await;
        }

        // 3. Compile rules back to agent_playbook.guidelines
        let compiled = compile_guidelines(pool, &receiver).await.unwrap_or_else(|_| new_guidelines.clone());
        sqlx::query(
            "UPDATE agent_playbook SET guidelines = ?, version = ?, updated_at = datetime('now') WHERE role_id = ?"
        )
        .bind(&compiled)
        .bind(new_version)
        .bind(&receiver)
        .execute(pool)
        .await?;

        // Log evolution step
        let log_id = Uuid::new_v4().to_string();
        let _ = sqlx::query(
            r#"INSERT INTO agent_evolution_log (id, role_id, old_guidelines, new_guidelines, reasoning)
               VALUES (?, ?, ?, ?, ?)"#
        )
        .bind(&log_id)
        .bind(&receiver)
        .bind(&old_guidelines)
        .bind(&compiled)
        .bind(&update.reasoning)
        .execute(pool)
        .await;

        // Mark this group's feedback entries as resolved
        for (id, _, _, _) in &feedback_list {
            let _ = sqlx::query("UPDATE agent_feedback_log SET is_resolved = 1 WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await;
        }

        applied_updates.push(update);
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
