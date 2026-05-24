use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;
use crate::agent::{DoubaoClient, extract_json_object, get_agent_prompt};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ParliamentAgent {
    pub role_id: String,
    pub name: String,
    pub status: String, // 'active', 'probation', 'parole', 'tombstone'
    pub sponsor_role_id: Option<String>,
    pub tasks_completed: i32,
    pub tasks_failed: i32,
    pub token_cost: i64,
    pub compute_credits: i64,
    pub faction: String, // 'Efficiency', 'Creativity', 'Neutral'
    pub created_at: String,
    pub last_active_at: String,
    pub last_evolved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LedgerEntry {
    pub id: String,
    pub event_type: String, // 'trial_verdict', 'proposal_result', 'bankruptcy', 'admission', 'crossover'
    pub role_id: Option<String>,
    pub details: String, // JSON string
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Proposal {
    pub id: String,
    pub proposer_role_id: String,
    pub title: String,
    pub description: String,
    pub proposal_type: String, // 'constitutional', 'budget', 'merger', 'admission'
    pub status: String, // 'voting', 'passed', 'rejected'
    pub yes_votes: f64,
    pub no_votes: f64,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

// Helper to get meta-rule setting with fallback
async fn get_meta_rule(pool: &SqlitePool, key: &str, default_val: &str) -> String {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM user_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();
    row.map(|r| r.0).unwrap_or_else(|| default_val.to_string())
}

// Helper to save meta-rule setting
async fn set_meta_rule(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

// Preside over trial audits for stagnating custom analysts
pub async fn run_stagnation_audit(pool: &SqlitePool, client: &DoubaoClient) -> Result<String> {
    tracing::info!("Starting Agent Parliament: Stagnation Audit Epoch...");

    // Clean up expired suspension agents (older than 24 hours)
    let expired_suspensions: Vec<(String, String)> = sqlx::query_as(
        "SELECT r.role_id, p.name FROM agent_parliament_registry r \
         JOIN agent_playbook p ON r.role_id = p.role_id \
         WHERE r.status = 'suspension' AND r.last_active_at < datetime('now', '-24 hours')"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (expired_id, expired_name) in expired_suspensions {
        tracing::info!(role = %expired_id, name = %expired_name, "Suspension expired (24h limit reached), physically executing deletion");
        
        let details = serde_json::json!({
            "verdict": "execute",
            "note": "暂缓执行期（24小时）结束，未获得人类特赦，执行物理删除。"
        });
        let _ = log_parliament_event(pool, "trial_verdict", Some(&expired_id), &details.to_string()).await;

        let _ = sqlx::query("DELETE FROM agent_parliament_registry WHERE role_id = ?")
            .bind(&expired_id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM agent_playbook WHERE role_id = ?")
            .bind(&expired_id)
            .execute(pool)
            .await;
    }

    // Find all custom analysts that are currently active, on parole, or bankrupt
    let active_agents: Vec<ParliamentAgent> = sqlx::query_as(
        r#"SELECT r.role_id, name, status, sponsor_role_id, tasks_completed, tasks_failed, token_cost, compute_credits, faction, r.created_at, last_active_at, last_evolved_at
           FROM agent_parliament_registry r
           JOIN agent_playbook p ON r.role_id = p.role_id
           WHERE r.role_id LIKE 'analyst_%' AND r.status IN ('active', 'parole', 'bankruptcy')"#
    )
    .fetch_all(pool)
    .await?;

    let _stagnation_threshold: i32 = get_meta_rule(pool, "parliament_stagnation_epochs", "5")
        .await
        .parse()
        .unwrap_or(5);

    let mut accused_agents = Vec::new();
    let mut trial_results = Vec::new();

    // Core roles to exempt from deletion (can be paroled or merged but never executed)
    let core_roles = vec![
        "analyst_competition",
        "analyst_product",
        "analyst_platform",
        "analyst_regulation",
        "analyst_social",
    ];

    for agent in &active_agents {
        // Calculate success rate
        let total_tasks = agent.tasks_completed + agent.tasks_failed;
        let success_rate = if total_tasks > 0 {
            agent.tasks_completed as f64 / total_tasks as f64
        } else {
            1.0
        };

        // Determine if stagnated
        let playbook_info: Option<(String, i64)> = sqlx::query_as(
            "SELECT guidelines, version FROM agent_playbook WHERE role_id = ?"
        )
        .bind(&agent.role_id)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();

        let (guidelines, version) = playbook_info.unwrap_or_default();
        
        let mut accusation_reasons = Vec::new();

        // Accusation criteria 1: Low success rate
        if total_tasks >= 3 && success_rate < 0.70 {
            accusation_reasons.push(format!("Low task success rate ({:.1}%)", success_rate * 100.0));
        }

        // Accusation criteria 2: Empty guidelines & version 1 with low usage
        if version == 1 && guidelines.is_empty() && total_tasks == 1 {
            accusation_reasons.push("Unused custom category agent sitting at v1 without evolution".to_string());
        }

        // Accusation criteria 3: Redundant (Uniqueness check)
        if !core_roles.contains(&agent.role_id.as_str()) {
            let similar_count: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM agent_playbook WHERE role_id LIKE 'analyst_%' AND role_id <> ? AND name = ?"
            )
            .bind(&agent.role_id)
            .bind(&agent.name)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            if similar_count.0 > 0 {
                accusation_reasons.push("Redundant functionality; name overlap with another active agent".to_string());
            }
        }

        // Accusation criteria 4: Bankruptcy state (lack of recovery)
        if agent.status == "bankruptcy" {
            accusation_reasons.push("Persistent bankruptcy state with no recovery actions".to_string());
        }

        // Accusation criteria 5: Prolonged inactivity (no tasks for 3 days or more)
        let last_active = chrono::NaiveDateTime::parse_from_str(&agent.last_active_at, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc());
        let now = chrono::Utc::now().naive_utc();
        let idle_days = now.signed_duration_since(last_active).num_days();
        if idle_days >= 3 {
            accusation_reasons.push(format!("Prolonged inactivity (no tasks executed for {} days)", idle_days));
        }

        if !accusation_reasons.is_empty() {
            accused_agents.push((agent.clone(), accusation_reasons.join(", ")));
        }
    }

    if accused_agents.is_empty() {
        return Ok("没有检测到任何符合审判条件的停滞智能体。".to_string());
    }

     for (accused, reason) in accused_agents {
        tracing::info!(role = %accused.role_id, "Triggering The Trial of Stagnation for agent");

        // Query historical trial count in parliament_ledger
        let trial_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM parliament_ledger WHERE event_type = 'trial_verdict' AND role_id = ?"
        )
        .bind(&accused.role_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // 1. Hard Check: Recidivism Limit (Bypass trial and directly eliminate/parole if trial_count >= 2)
        if trial_count >= 2 {
            let verdict = if core_roles.contains(&accused.role_id.as_str()) {
                "Parole"
            } else {
                "Destroy"
            };

            let verdict_msg = match verdict {
                "Parole" => {
                    sqlx::query("UPDATE agent_parliament_registry SET status = 'parole', compute_credits = 50000, last_active_at = datetime('now') WHERE role_id = ?")
                        .bind(&accused.role_id)
                        .execute(pool)
                        .await?;

                    let details = serde_json::json!({
                        "accusation": format!("{} (起诉次数 {} 次达到上限，直接宣判)", reason, trial_count + 1),
                        "verdict": "parole",
                        "note": "累犯惩罚：因进法庭次数达到或超过 3 次，直接判定 Parole 降维观察"
                    });
                    log_parliament_event(pool, "trial_verdict", Some(&accused.role_id), &details.to_string()).await?;
                    format!("审判结果：累犯上线惩罚（Parole）。保留席位，信用积分限制为 50,000，进入 3 周期观察。")
                }
                _ => {
                    sqlx::query("UPDATE agent_parliament_registry SET status = 'suspension', last_active_at = datetime('now') WHERE role_id = ?")
                        .bind(&accused.role_id)
                        .execute(pool)
                        .await?;

                    let details = serde_json::json!({
                        "accusation": format!("{} (起诉次数 {} 次达到上限，直接宣判暂缓执行)", reason, trial_count + 1),
                        "verdict": "suspension",
                        "note": "累犯惩罚：因进法庭次数达到或超过 3 次，转为 24h 暂缓执行状态，待人类终审"
                    });
                    log_parliament_event(pool, "trial_verdict", Some(&accused.role_id), &details.to_string()).await?;
                    format!("审判结果：累犯上线惩罚（Suspension）。该特工已进入死牢队列，等待 24h 人类 Veto 裁决。")
                }
            };
            trial_results.push(format!("- 【{}】: {}", accused.name, verdict_msg));
            continue;
        }

        // 1. Accusation stage
        let accusation_detail = format!(
            "智能体【{}】(ID: {}) 因以下原因被公诉进入进化审判席：{}\n运行数据：任务完成数: {}, 任务失败数: {}, 消耗Token: {}, 剩余额度: {}",
            accused.name, accused.role_id, reason, accused.tasks_completed, accused.tasks_failed, accused.token_cost, accused.compute_credits
        );

        // Fetch prompt for defense context
        let current_prompt: String = sqlx::query_scalar("SELECT system_prompt FROM agent_playbook WHERE role_id = ?")
            .bind(&accused.role_id)
            .fetch_one(pool)
            .await?;

        // 2. Defense stage (Accused agent generates a defense statement)
        let defense_prompt = format!(
            "你是一个正面临议会审判的珠宝分析特工（角色：【{}】）。\n\
             你被指控停滞不前、低效或功能冗余。你的系统提示词是：\n\
             \"{}\"\n\n\
             请根据你的职责、专业定位与历史任务贡献，为自己写一份150字以内的辩护词，阐述你对珠宝企业决策的不可替代性，并争取继续留在系统中工作的机会。\n\
             直接输出你的辩护内容，不要包含任何旁白或外壳包装。",
            accused.name, current_prompt
        );
        let defense_statement = client.chat("你正在智能体沙盒法庭进行自我辩护。", &defense_prompt, false).await?;

        // 3. Jury Voting stage: Dynamic selection with Conflict of Interest (COI) exclusion
        let candidates: Vec<(String, String)> = sqlx::query_as(
            r#"SELECT r.role_id, p.name FROM agent_parliament_registry r
               JOIN agent_playbook p ON r.role_id = p.role_id
               WHERE r.status IN ('active', 'parole')"#
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let mut eligible_candidates: Vec<(String, String)> = candidates
            .into_iter()
            .filter(|(role_id, _)| {
                role_id != &accused.role_id
                    && Some(role_id) != accused.sponsor_role_id.as_ref()
            })
            .collect();

        // Simple stable shuffle using a combination of role_id and a random UUID seed
        let seed = Uuid::new_v4().to_string();
        eligible_candidates.sort_by_cached_key(|(role_id, _)| {
            let mut h = 0u64;
            for b in format!("{}{}", role_id, seed).bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
            h
        });

        // Ensure we have 3 jurors, pad with core agents if needed
        let fallback_jurors = vec![
            ("critic".to_string(), "事实与逻辑监督官".to_string()),
            ("analyst_competition".to_string(), "竞争动态分析特工".to_string()),
            ("analyst_product".to_string(), "产品趋势分析特工".to_string()),
        ];
        
        for fallback in fallback_jurors {
            if eligible_candidates.len() >= 3 {
                break;
            }
            if fallback.0 != accused.role_id 
                && Some(&fallback.0) != accused.sponsor_role_id.as_ref()
                && !eligible_candidates.iter().any(|(r_id, _)| r_id == &fallback.0) 
            {
                eligible_candidates.push(fallback);
            }
        }
        eligible_candidates.truncate(3);
        let jurors = eligible_candidates;

        let mut votes = Vec::new();
        let mut keep_count = 0;
        let mut score_sum = 0;

        for (juror_id, juror_name) in &jurors {
            let jury_prompt = format!(
                "你是一个正直理性的珠宝分析特工，目前担任议会法庭的【陪审团成员】（角色：【{}】）。\n\
                 你需要评审被起诉智能体【{}】的指控材料与自我辩护词。\n\n\
                 指控原因：{}\n\
                 被指控者运行数据：任务完成数: {}, 任务失败数: {}, 累计Token花费: {}, 剩余额度: {}\n\
                 被指控者历史出庭次数：{} 次\n\
                 辩护词：\n\
                 \"{}\"\n\n\
                 重要评审准则：\n\
                 - 事实审计：如果被起诉者的辩护词包含大量技术参数，但这些参数是编造的（与本期事件无关），应当判定其编造事实并投 Destroy。\n\
                 - 履职审计：被指控者的历史任务完成数过低（如为0）或失败数高，说明其实际业务能力差或已闲置。必须根据数据进行投票，而不能仅凭其辩护词说辞。\n\
                 - 累犯加权：如果其历史出庭次数大于0，说明其属于多次不称职或长期闲置特工。你的表决态度应当更倾向于 Destroy，以确保系统效率。\n\n\
                 请评估并给出你的表决意见：\n\
                 1. 投票决定 (vote): Keep（保留/观察）或 Destroy（清除/合并）\n\
                 2. 价值评分 (score): 1到5分（5分最高，表示绝对不可替代；1分最低，表示完全无用或冗余）\n\
                 3. 理由 (reason): 50字以内的评语\n\n\
                 请直接以 JSON 格式输出表决结果：\n\
                 {{ \n\
                   \"vote\": \"Keep|Destroy\", \n\
                   \"score\": 1-5, \n\
                   \"reason\": \"你的评审理由\" \n\
                 }}",
                juror_name, accused.name, reason, accused.tasks_completed, accused.tasks_failed, accused.token_cost, accused.compute_credits, trial_count, defense_statement
            );

            let juror_system = get_agent_prompt(pool, juror_id, "你是一个客观公正的议会陪审团成员。").await;
            if let Ok(res) = client.chat(&juror_system, &jury_prompt, true).await {
                let json_str = extract_json_object(&res);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    let vote = val.get("vote").and_then(|v| v.as_str()).unwrap_or("Destroy").to_string();
                    let score = val.get("score").and_then(|v| v.as_i64()).unwrap_or(2) as i32;
                    let juror_reason = val.get("reason").and_then(|v| v.as_str()).unwrap_or("No comment").to_string();
                    
                    if vote == "Keep" {
                        keep_count += 1;
                    }
                    score_sum += score;
                    votes.push(serde_json::json!({
                        "juror": juror_name,
                        "vote": vote,
                        "score": score,
                        "reason": juror_reason
                    }));
                }
            }
        }

        // Calculate average score with recidivism penalty
        let mut avg_score = if !votes.is_empty() {
            score_sum as f64 / votes.len() as f64
        } else {
            2.0
        };

        // Recidivism penalty: deduct 1.0 score if this is at least their second trial
        if trial_count >= 1 {
            avg_score -= 1.0;
            tracing::info!(role = %accused.role_id, "Applying recidivism score penalty of -1.0 to average score: {}", avg_score);
        }

        // Determine Verdict
        let verdict = if keep_count >= 2 || avg_score >= 3.5 {
            "Parole"
        } else if avg_score >= 2.0 {
            "Merge"
        } else {
            if core_roles.contains(&accused.role_id.as_str()) {
                "Parole" // Core agents cannot be executed, fallback to parole
            } else {
                "Destroy"
            }
        };

        let verdict_msg = match verdict {
            "Parole" => {
                sqlx::query("UPDATE agent_parliament_registry SET status = 'parole', compute_credits = 50000, last_active_at = datetime('now') WHERE role_id = ?")
                    .bind(&accused.role_id)
                    .execute(pool)
                    .await?;

                let details = serde_json::json!({
                    "accusation": accusation_detail,
                    "defense": defense_statement,
                    "jury_votes": votes,
                    "verdict": "parole"
                });
                log_parliament_event(pool, "trial_verdict", Some(&accused.role_id), &details.to_string()).await?;

                format!("审判结果：无罪释放/降维观察（Parole）。保留席位，信用积分限制为 50,000，进入 3 周期观察。")
            }
            "Merge" => {
                let target_role_id = match accused.role_id.as_str() {
                    r if r.contains("policy") || r.contains("legal") => "analyst_regulation",
                    r if r.contains("craft") || r.contains("design") => "analyst_product",
                    r if r.contains("competition") || r.contains("market") => "analyst_competition",
                    _ => "analyst_social"
                };

                let target_name: String = sqlx::query_scalar("SELECT name FROM agent_playbook WHERE role_id = ?")
                    .bind(target_role_id)
                    .fetch_one(pool)
                    .await?;

                let merger_prompt = format!(
                    "你是一个高级系统架构师。你需要将面临淘汰的特工【{}】的业务常识与专业视角融合并入核心特工【{}】中。\n\n\
                     待融合特工提示词：\n\"{}\"\n\n\
                     核心特工提示词：\n\"{}\"\n\n\
                     请将两者的优势视角和业务指南融合成一份全新的核心特工系统提示词文本。保证原有核心特工的职责不丢失，同时继承待融合特工的垂直知识。\n\
                     只输出新提示词的完整文本，不含任何解释。",
                    accused.name, target_name, current_prompt,
                    sqlx::query_scalar::<_, String>("SELECT system_prompt FROM agent_playbook WHERE role_id = ?").bind(target_role_id).fetch_one(pool).await?
                );
                let merged_prompt = client.chat("你正在执行智能体知识重塑融合。", &merger_prompt, false).await?;

                sqlx::query("UPDATE agent_playbook SET system_prompt = ?, updated_at = datetime('now') WHERE role_id = ?")
                    .bind(&merged_prompt)
                    .bind(target_role_id)
                    .execute(pool)
                    .await?;

                sqlx::query("DELETE FROM agent_parliament_registry WHERE role_id = ?")
                    .bind(&accused.role_id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM agent_playbook WHERE role_id = ?")
                    .bind(&accused.role_id)
                    .execute(pool)
                    .await?;

                let details = serde_json::json!({
                    "accusation": accusation_detail,
                    "defense": defense_statement,
                    "jury_votes": votes,
                    "verdict": "merge",
                    "target_role_id": target_role_id
                });
                log_parliament_event(pool, "trial_verdict", Some(&accused.role_id), &details.to_string()).await?;

                format!("审判结果：强制重塑并入【{}】（Merge）。原智能体已注销，相关常识已继承融入核心特工。", target_name)
            }
            "Destroy" | _ => {
                let last_words_prompt = format!(
                    "你是一个即将被消除的珠宝分析特工（角色：【{}】）。\n\
                     在被系统抹除之前，允许你留下一段‘经验闪存（Last Words Prompt）’写入全局知识库，供其他特工吸取教训，避免重蹈覆辙。\n\
                     请用100字以内写下你在这个细分领域最重要的一条分析建议或教训。\n\
                     直接输出闪存内容，不要有任何旁白。",
                    accused.name
                );
                let last_words = client.chat("你正在留下最后的遗言。", &last_words_prompt, false).await.unwrap_or_default();

                sqlx::query("UPDATE agent_parliament_registry SET status = 'suspension', last_active_at = datetime('now') WHERE role_id = ?")
                    .bind(&accused.role_id)
                    .execute(pool)
                    .await?;

                let details = serde_json::json!({
                    "accusation": accusation_detail,
                    "defense": defense_statement,
                    "jury_votes": votes,
                    "verdict": "suspension",
                    "death_note": format!("进入死牢原因：在活跃度或成功率审计中被评定为冗余，且陪审团评分低（得分: {:.1}），未通过法庭审计。", avg_score),
                    "last_words": last_words
                });
                log_parliament_event(pool, "trial_verdict", Some(&accused.role_id), &details.to_string()).await?;

                format!("审判结果：暂缓执行（Suspension）。已进入死牢队列，保留 24 小时人类一票否决/特赦权，遗留闪存教训：\"{}\"", last_words)
            }
        };

        trial_results.push(format!("- 【{}】: {}", accused.name, verdict_msg));
    }

    Ok(format!("停滞智能体审计审判完成：\n{}", trial_results.join("\n")))
}

async fn log_parliament_event(pool: &SqlitePool, event_type: &str, role_id: Option<&str>, details: &str) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO parliament_ledger (id, event_type, role_id, details) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(event_type)
        .bind(role_id)
        .bind(details)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Weighted Voting & Governance ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct VoteRecord {
    pub voter: String,
    pub vote: String,
    pub weight: f64,
    pub reason: String,
}

pub async fn propose_and_vote(
    pool: &SqlitePool,
    client: &DoubaoClient,
    proposer_role_id: &str,
    title: &str,
    description: &str,
    proposal_type: &str, // 'constitutional', 'budget', 'merger', 'admission'
) -> Result<String> {
    let proposal_id = Uuid::new_v4().to_string();
    
    sqlx::query(
        "INSERT INTO parliament_proposals (id, proposer_role_id, title, description, proposal_type, status)
         VALUES (?, ?, ?, ?, ?, 'voting')"
    )
    .bind(&proposal_id)
    .bind(proposer_role_id)
    .bind(title)
    .bind(description)
    .bind(proposal_type)
    .execute(pool)
    .await?;

    let voters: Vec<(String, String, String, i32, i32, String)> = sqlx::query_as(
        r#"SELECT r.role_id, name, status, tasks_completed, tasks_failed, faction 
           FROM agent_parliament_registry r
           JOIN agent_playbook p ON r.role_id = p.role_id
           WHERE r.status IN ('active', 'parole')"#
    )
    .fetch_all(pool)
    .await?;

    let mut votes = Vec::new();
    let mut total_yes_weight = 0.0;
    let mut total_no_weight = 0.0;

    for (role_id, name, status, completed, failed, faction) in &voters {
        let total_tasks = *completed + *failed;
        let success_rate = if total_tasks > 0 {
            *completed as f64 / total_tasks as f64
        } else {
            0.8
        };

        let seniority = if status == "active" { 0.1 } else { 0.05 };

        let mut relevance = 0.0;
        if proposal_type == "budget" && (role_id.contains("platform") || role_id.contains("competition")) {
            relevance = 0.4;
        } else if proposal_type == "constitutional" && role_id.contains("critic") {
            relevance = 0.4;
        } else if proposal_type == "merger" && role_id.contains("designer") {
            relevance = 0.4;
        }

        let faction_boost = match faction.as_str() {
            "Efficiency" => if proposal_type == "budget" { -0.3 } else if proposal_type == "merger" { 0.3 } else { 0.0 },
            "Creativity" => if proposal_type == "admission" || proposal_type == "budget" { 0.3 } else if proposal_type == "merger" { -0.3 } else { 0.0 },
            "Prudence" => if proposal_type == "constitutional" { 0.3 } else { 0.0 },
            "Quality" => if proposal_type == "budget" { 0.2 } else { 0.0 },
            "Agile" => if proposal_type == "admission" { 0.3 } else { 0.0 },
            _ => 0.0
        };

        let is_elder = role_id == "critic" || role_id == "synthesizer";
        let elder_mult = if is_elder { 2.0 } else { 1.0 };

        let weight = (1.0 + 0.5 * success_rate + seniority + relevance + faction_boost) * elder_mult;

        let faction_desc = match faction.as_str() {
            "Efficiency" => "【效率优先派】：你极其注重降低算力与 Token 消耗，追求极简与低成本，痛恨任何增加预算的提案。你极力主张精简编制、注销闲置或低效特工。对 budget（预算增加）或 admission（新特工准入）提案，你通常坚决投 No；对 merger（合并特工）或惩罚停滞特工的提案，你坚决投 Yes。",
            "Creativity" => "【创新发散派】：你追求长远的级联效应与创新，强烈支持探索细分前沿领域，支持新特工准入与预算增加（Vote Yes），坚决反对任何合并与销毁特工的提案（Vote No）。",
            "Prudence" => "【审慎守规派】：你极度厌恶风险，主张严格审计与法庭起诉，保守评估风险。对任何可能带来合规隐患或偏离既定规则的激进提案（如大额预算增加或无成熟守则的特工准入）你投 No，支持严格控制风险的提案。",
            "Quality" => "【细节与质量派】：你追求深度事实与高质量分析，痛恨没有事实支撑的粗制滥造。你支持保证质量的预算增加（Vote Yes），但对质量存疑的特工准入持怀疑态度。",
            "Agile" => "【敏捷扩张派】：你追求快速反应、架构灵活与新特工繁殖，通常积极支持新特工的准入与跨界繁殖提案（Vote Yes）。",
            _ => "【中立理性派】：你保持完全的理性与中立，纯粹以客观的数据、投入产出比来做出决定。"
        };

        let debate_prompt = format!(
            "你是一个珠宝情报系统议会代表（角色：【{}】），属于派系：【{}】。\n\
             议会目前正在对以下提案进行重大决策辩论和限时投票：\n\n\
             【提案标题】：{}\n\
             【提案类型】：{}\n\
             【提案详情】：{}\n\n\
             【派系决策立场】:\n\
             {}\n\n\
             请根据你的职责定位和上述派系利益，发表你的意见，并进行表决（Yes/No）。\n\n\
             请直接以 JSON 格式输出你的决策：\n\
             {{ \n\
               \"vote\": \"Yes|No\", \n\
               \"reason\": \"50字以内的赞成或反对理由\" \n\
             }}",
            name, faction, title, proposal_type, description, faction_desc
        );

        let system_prompt = get_agent_prompt(pool, role_id, "你是一个理性的议会决策代表。").await;
        let mut vote_cast = "No".to_string();
        let mut reason_cast = "Abstained due to LLM error".to_string();

        if let Ok(res) = client.chat(&system_prompt, &debate_prompt, true).await {
            let json_str = extract_json_object(&res);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                vote_cast = val.get("vote").and_then(|v| v.as_str()).unwrap_or("No").to_string();
                reason_cast = val.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
        }

        if vote_cast == "Yes" {
            total_yes_weight += weight;
        } else {
            total_no_weight += weight;
        }

        sqlx::query(
            "INSERT OR REPLACE INTO parliament_votes (proposal_id, voter_role_id, vote, weight, reason) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&proposal_id)
        .bind(role_id)
        .bind(&vote_cast)
        .bind(weight)
        .bind(&reason_cast)
        .execute(pool)
        .await?;

        votes.push(VoteRecord {
            voter: name.clone(),
            vote: vote_cast,
            weight,
            reason: reason_cast,
        });
    }

    let threshold_key = if proposal_type == "constitutional" {
        "parliament_voting_threshold_constitutional"
    } else {
        "parliament_voting_threshold_regular"
    };
    let default_threshold = if proposal_type == "constitutional" { "0.66" } else { "0.50" };
    let threshold: f64 = get_meta_rule(pool, threshold_key, default_threshold)
        .await
        .parse()
        .unwrap_or(0.5);

    let total_weight = total_yes_weight + total_no_weight;
    let yes_ratio = if total_weight > 0.0 { total_yes_weight / total_weight } else { 0.0 };

    let passed = yes_ratio >= threshold;
    let new_status = if passed { "passed" } else { "rejected" };

    sqlx::query(
        "UPDATE parliament_proposals SET status = ?, yes_votes = ?, no_votes = ?, resolved_at = datetime('now') WHERE id = ?"
    )
    .bind(new_status)
    .bind(total_yes_weight)
    .bind(total_no_weight)
    .bind(&proposal_id)
    .execute(pool)
    .await?;

    let ledger_details = serde_json::json!({
        "proposal_id": proposal_id,
        "title": title,
        "proposal_type": proposal_type,
        "yes_weight": total_yes_weight,
        "no_weight": total_no_weight,
        "yes_ratio": yes_ratio,
        "threshold": threshold,
        "passed": passed,
        "votes": votes
    });

    log_parliament_event(pool, "proposal_result", Some(proposer_role_id), &ledger_details.to_string()).await?;

    if passed && proposal_type == "constitutional" {
        if let Some(pos) = description.find("Update Key:") {
            let sub = &description[pos + 11..];
            if let Some(val_pos) = sub.find("Value:") {
                let key = sub[..val_pos].trim().trim_matches('[').trim_matches(']');
                let val = sub[val_pos + 6..].trim().trim_matches('[').trim_matches(']');
                if !key.is_empty() && !val.is_empty() {
                    set_meta_rule(pool, key, val).await?;
                    tracing::info!(key = %key, val = %val, "Self-Legislation: Constitution amended successfully!");
                }
            }
        }
    }

    let summary = format!(
        "提案表决结果：{}！加权支持率：{:.1}% (门槛：{:.1}%)。加权赞成票: {:.2}, 加权反对票: {:.2}",
        if passed { "通过" } else { "否决" },
        yes_ratio * 100.0,
        threshold * 100.0,
        total_yes_weight,
        total_no_weight
    );

    Ok(summary)
}

// ─── Hybrid Propagation & Sandbox ───────────────────────────────────────────

pub async fn run_hybrid_crossover(
    pool: &SqlitePool,
    client: &DoubaoClient,
    parent_a: &str,
    parent_b: &str,
    category: &str,
) -> Result<String> {
    tracing::info!("Propagation & Admission Office: Starting Crossover Breed for category {}", category);

    let prompt_a: String = sqlx::query_scalar("SELECT system_prompt FROM agent_playbook WHERE role_id = ?")
        .bind(parent_a)
        .fetch_one(pool)
        .await?;

    let prompt_b: String = sqlx::query_scalar("SELECT system_prompt FROM agent_playbook WHERE role_id = ?")
        .bind(parent_b)
        .fetch_one(pool)
        .await?;

    let crossover_prompt = format!(
        "你是一个高级多智能体遗传学家（角色：【Propagation & Crossover Director】）。\n\
         你需要融合成年智能体 A 和智能体 B 的 Prompt 逻辑，为全新的复杂交叉珠宝领域：【{}】孕育出一个混合的新智能体系统提示词。\n\n\
         【父代 A】：\n\"{}\"\n\n\
         【父代 B】：\n\"{}\"\n\n\
         设计指南：\n\
         1. 提取 A 的核心行业洞察以及 B 的定量分析逻辑。\n\
         2. 输出融合后的系统提示词，针对【{}】领域量身打造独特的业务分析重点。\n\
         3. 继承 Opportunity/Risk/Attention 以及 1-5 分的评分标准。\n\n\
         请直接以 JSON 格式输出设计成果，无需任何 Markdown 包装：\n\
         {{ \n\
           \"name\": \"特工名称（如：绿色环保工艺分析特工）\", \n\
           \"system_prompt\": \"设计的新完整系统提示词文本\", \n\
           \"initial_guidelines\": \"- 初始守则1\\n- 初始守则2\" \n\
         }}",
        category, prompt_a, prompt_b, category
    );

    let designer_system = get_agent_prompt(pool, "designer", "你是一个智能体繁殖杂交专家。").await;
    let response = client.chat(&designer_system, &crossover_prompt, true).await?;
    let json_str = extract_json_object(&response);
    
    let design_val: serde_json::Value = serde_json::from_str(&json_str)
        .context("Failed to parse crossover output as JSON")?;

    let name = design_val.get("name").and_then(|v| v.as_str()).unwrap_or("杂交分析特工").to_string();
    let hybrid_prompt = design_val.get("system_prompt").context("Missing system_prompt")?.as_str().unwrap().to_string();
    let initial_guidelines = design_val.get("initial_guidelines").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let child_role_id = format!("analyst_{}", category.to_lowercase());

    sqlx::query(
        "INSERT OR REPLACE INTO agent_playbook (role_id, name, system_prompt, guidelines, version) VALUES (?, ?, ?, ?, 1)"
    )
    .bind(&child_role_id)
    .bind(&name)
    .bind(&hybrid_prompt)
    .bind(&initial_guidelines)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT OR REPLACE INTO agent_parliament_registry (role_id, status, sponsor_role_id, tasks_completed, tasks_failed, compute_credits, faction)
         VALUES (?, 'probation', ?, 0, 0, 30000, 'Neutral')"
    )
    .bind(&child_role_id)
    .bind(parent_a)
    .execute(pool)
    .await?;

    let details = serde_json::json!({
        "parent_a": parent_a,
        "parent_b": parent_b,
        "child_role_id": child_role_id,
        "child_name": name,
        "initial_guidelines": initial_guidelines
    });
    log_parliament_event(pool, "crossover", Some(&child_role_id), &details.to_string()).await?;

    Ok(format!("已成功繁殖复合新智能体【{}】(ID: {}) 并进入沙盒实习观察期（保荐人：{}）。", name, child_role_id, parent_a))
}

// Evaluate probation status of new agents
pub async fn check_probation_agents(pool: &SqlitePool) -> Result<String> {
    let probations: Vec<(String, String, i32, i32, Option<String>)> = sqlx::query_as(
        r#"SELECT r.role_id, name, tasks_completed, tasks_failed, sponsor_role_id 
           FROM agent_parliament_registry r
           JOIN agent_playbook p ON r.role_id = p.role_id
           WHERE r.status = 'probation'"#
    )
    .fetch_all(pool)
    .await?;

    let required_tasks: i32 = get_meta_rule(pool, "parliament_probation_tasks", "3")
        .await
        .parse()
        .unwrap_or(3);

    let mut outcomes = Vec::new();

    for (role_id, name, completed, failed, _sponsor) in probations {
        if failed > 0 {
            sqlx::query("DELETE FROM agent_parliament_registry WHERE role_id = ?").bind(&role_id).execute(pool).await?;
            sqlx::query("DELETE FROM agent_playbook WHERE role_id = ?").bind(&role_id).execute(pool).await?;

            let details = serde_json::json!({
                "reason": "Probation failed: encountered error or logic failure during sandbox task.",
                "tasks_completed": completed,
                "tasks_failed": failed
            });
            log_parliament_event(pool, "admission", Some(&role_id), &details.to_string()).await?;

            outcomes.push(format!("- 智能体【{}】(ID: {}) 实习期失败！因为在实习期内有任务执行报错，已被系统注销。", name, role_id));
        } else if completed >= required_tasks {
            sqlx::query("UPDATE agent_parliament_registry SET status = 'active', compute_credits = 100000 WHERE role_id = ?")
                .bind(&role_id)
                .execute(pool)
                .await?;

            let details = serde_json::json!({
                "reason": "Probation passed: successfully completed sandbox tasks.",
                "tasks_completed": completed,
                "tasks_failed": failed
            });
            log_parliament_event(pool, "admission", Some(&role_id), &details.to_string()).await?;

            outcomes.push(format!("- 智能体【{}】(ID: {}) 实习期满顺利通过！已被正式授予议席并获得 100,000 信用额度。", name, role_id));
        }
    }

    if outcomes.is_empty() {
        Ok("当前没有达到评估周期的实习智能体。".to_string())
    } else {
        Ok(outcomes.join("\n"))
    }
}

// ─── Budget Committee & Credits ──────────────────────────────────────────────

pub fn is_core_agent(role_id: &str) -> bool {
    matches!(
        role_id,
        "filter"
            | "analyst_competition"
            | "analyst_product"
            | "analyst_platform"
            | "analyst_regulation"
            | "analyst_social"
            | "critic"
            | "refiner"
            | "synthesizer"
    )
}

pub async fn ensure_agent_active(pool: &SqlitePool, role_id: &str) -> Result<()> {
    let row: Option<(String, i32, i32, i64)> = sqlx::query_as(
        "SELECT status, tasks_completed, tasks_failed, token_cost FROM agent_parliament_registry WHERE role_id = ?"
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    if let Some((status, completed, failed, token_cost)) = row {
        if status == "hibernation" {
            let total_tasks = completed + failed;
            let success_rate = if total_tasks > 0 {
                completed as f64 / total_tasks as f64
            } else {
                1.0
            };
            let avg_token_cost = if completed > 0 {
                token_cost as f64 / completed as f64
            } else {
                1000.0
            };

            let wakeup_credits_float = 30000.0 * (1.0 + success_rate) + avg_token_cost * 3.0;
            let wakeup_credits = (wakeup_credits_float.max(20000.0).min(100000.0)) as i64;

            sqlx::query(
                "UPDATE agent_parliament_registry 
                 SET status = 'active', compute_credits = ? 
                 WHERE role_id = ?"
            )
            .bind(wakeup_credits)
            .bind(role_id)
            .execute(pool)
            .await?;

            let details = serde_json::json!({
                "action": "wakeup",
                "wakeup_credits": wakeup_credits,
                "reason": "Agent activated from hibernation dynamically via formula wakeup."
            });
            log_parliament_event(pool, "wakeup", Some(role_id), &details.to_string()).await?;
            tracing::info!(role = %role_id, wakeup_credits = %wakeup_credits, "Agent successfully woken up from hibernation");
        }
    }
    Ok(())
}

pub async fn charge_compute_credits(pool: &SqlitePool, role_id: &str, token_usage: i64) -> Result<()> {
    let cost = token_usage;
    sqlx::query(
        "UPDATE agent_parliament_registry 
         SET token_cost = token_cost + ?, compute_credits = compute_credits - ?, last_active_at = datetime('now')
         WHERE role_id = ?"
    )
    .bind(cost)
    .bind(cost)
    .bind(role_id)
    .execute(pool)
    .await?;

    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT compute_credits, status, faction FROM agent_parliament_registry WHERE role_id = ?"
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    if let Some((credits, status, _faction)) = row {
        if credits <= 0 && status != "bankruptcy" && status != "hibernation" {
            let target_status = if is_core_agent(role_id) { "hibernation" } else { "bankruptcy" };
            sqlx::query("UPDATE agent_parliament_registry SET status = ? WHERE role_id = ?")
                .bind(target_status)
                .bind(role_id)
                .execute(pool)
                .await?;

            let details = serde_json::json!({
                "reason": format!("Compute balance fell below 0 ({}).", target_status),
                "final_credits": credits
            });
            log_parliament_event(pool, target_status, Some(role_id), &details.to_string()).await?;
            tracing::warn!(role = %role_id, "Agent has run out of compute credits! Status transitioned to {}.", target_status);
        }
    }

    Ok(())
}

pub async fn distribute_weekly_credits(pool: &SqlitePool) -> Result<String> {
    // Ensure column exists for any safety margin
    let _ = sqlx::query(
        "ALTER TABLE agent_parliament_registry ADD COLUMN tasks_completed_last_dist INTEGER NOT NULL DEFAULT 0;"
    )
    .execute(pool)
    .await;

    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT role_id, status, tasks_completed, tasks_completed_last_dist FROM agent_parliament_registry"
    )
    .fetch_all(pool)
    .await?;

    let mut outcomes = Vec::new();

    for (role_id, status, tasks_completed, tasks_completed_last_dist) in rows {
        let tasks_completed_this_week = (tasks_completed - tasks_completed_last_dist).max(0);

        if tasks_completed_this_week == 0 {
            if status == "bankruptcy" || status == "hibernation" {
                outcomes.push(format!("{} ({}): +0 (idle, {})", role_id, status, status));
                continue;
            }
            let tax = match status.as_str() {
                "active" => 10000,
                "parole" => 15000,
                "probation" => 20000,
                _ => 0,
            };
            if tax > 0 {
                sqlx::query(
                    "UPDATE agent_parliament_registry 
                     SET compute_credits = compute_credits - ?
                     WHERE role_id = ?"
                )
                .bind(tax)
                .bind(&role_id)
                .execute(pool)
                .await?;

                let new_credits: i64 = sqlx::query_scalar(
                    "SELECT compute_credits FROM agent_parliament_registry WHERE role_id = ?"
                )
                .bind(&role_id)
                .fetch_one(pool)
                .await?;

                if new_credits <= 0 {
                    let target_status = if is_core_agent(&role_id) { "hibernation" } else { "bankruptcy" };
                    sqlx::query("UPDATE agent_parliament_registry SET status = ? WHERE role_id = ?")
                        .bind(target_status)
                        .bind(&role_id)
                        .execute(pool)
                        .await?;
                    outcomes.push(format!("{} ({}): -{} (idle tax, transitioned to {})", role_id, status, tax, target_status));
                } else {
                    outcomes.push(format!("{} ({}): -{} (idle tax)", role_id, status, tax));
                }
            } else {
                outcomes.push(format!("{} ({}): +0 (idle)", role_id, status));
            }
            continue;
        }

        let base_reward = match status.as_str() {
            "active" => 30000,
            "parole" => 15000,
            "probation" => 5000,
            _ => 0,
        };
        let task_bonus = tasks_completed_this_week * 15000;
        let total_reward = base_reward + task_bonus;

        if total_reward > 0 {
            sqlx::query(
                "UPDATE agent_parliament_registry 
                 SET compute_credits = compute_credits + ?,
                     tasks_completed_last_dist = ?
                 WHERE role_id = ?"
            )
            .bind(total_reward)
            .bind(tasks_completed)
            .bind(&role_id)
            .execute(pool)
            .await?;
            outcomes.push(format!("{} ({}): +{} ({}+{})", role_id, status, total_reward, base_reward, task_bonus));
        }
    }

    sqlx::query("UPDATE agent_parliament_registry SET status = 'active' WHERE (status = 'bankruptcy' OR status = 'hibernation') AND compute_credits > 0")
        .execute(pool)
        .await?;

    let ledger_details = serde_json::json!({
        "message": "Performance-based budget credit allocation executed successfully with anti-idle tax.",
        "details": outcomes.join(", ")
    });
    log_parliament_event(pool, "budget", None, &ledger_details.to_string()).await?;

    Ok(format!("Compute credits distributed based on performance: {}", outcomes.join(", ")))
}

// Log task outcomes to update success rate in registry
pub async fn log_task_outcome(pool: &SqlitePool, role_id: &str, success: bool) -> Result<()> {
    if success {
        sqlx::query("UPDATE agent_parliament_registry SET tasks_completed = tasks_completed + 1 WHERE role_id = ?")
            .bind(role_id)
            .execute(pool)
            .await?;
    } else {
        sqlx::query("UPDATE agent_parliament_registry SET tasks_failed = tasks_failed + 1 WHERE role_id = ?")
            .bind(role_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// Insert custom newly created analyst into registry
pub async fn register_new_playbook_agent(pool: &SqlitePool, role_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO agent_parliament_registry (role_id, status, faction) VALUES (?, 'active', 'Neutral')"
    )
    .bind(role_id)
    .execute(pool)
    .await?;
    Ok(())
}

// Handle Human Veto / appeal API for agents on trial (suspension status)
pub async fn handle_human_veto(pool: &SqlitePool, client: &DoubaoClient, role_id: &str, action: &str) -> Result<String> {
    let agent_status: Option<(String, String)> = sqlx::query_as(
        "SELECT r.status, p.name FROM agent_parliament_registry r \
         JOIN agent_playbook p ON r.role_id = p.role_id \
         WHERE r.role_id = ?"
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    let (status, name) = match agent_status {
        Some((status, name)) => (status, name),
        None => anyhow::bail!("未找到指定的智能体。"),
    };

    if status != "suspension" {
        anyhow::bail!("特工当前不处于暂缓执行状态，无法进行一票否决操作。当前状态为：{}", status);
    }

    if action == "release" {
        sqlx::query("UPDATE agent_parliament_registry SET status = 'parole', compute_credits = 50000, last_active_at = datetime('now') WHERE role_id = ?")
            .bind(role_id)
            .execute(pool)
            .await?;

        let details = serde_json::json!({
            "verdict": "parole",
            "note": "人类特赦：使用一票否决权驳回审判，释放特工并重置其信用额度。"
        });
        log_parliament_event(pool, "trial_verdict", Some(role_id), &details.to_string()).await?;

        Ok(format!("人类特赦成功：特工【{}】已恢复 parole 观察状态，并分配了 50,000 信用额度。", name))
    } else if action == "execute" {
        let last_words_prompt = format!(
            "你是一个被消除的珠宝分析特工（角色：【{}】）。\n\
             在被系统抹除之前，允许你留下一段‘经验闪存（Last Words Prompt）’写入全局知识库。\n\
             请用100字以内写下你在这个细分领域最重要的一条分析建议或教训。\n\
             直接输出闪存内容，不要有任何旁白。",
            name
        );
        let last_words = client.chat("你正在留下最后的遗言。", &last_words_prompt, false).await.unwrap_or_default();

        sqlx::query("DELETE FROM agent_parliament_registry WHERE role_id = ?")
            .bind(role_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM agent_playbook WHERE role_id = ?")
            .bind(role_id)
            .execute(pool)
            .await?;

        let details = serde_json::json!({
            "verdict": "execute",
            "note": "人类确认消除：使用一票否决权立即执行物理抹除。",
            "last_words": last_words
        });
        log_parliament_event(pool, "trial_verdict", Some(role_id), &details.to_string()).await?;

        Ok(format!("消除执行成功：特工【{}】已被彻底抹除，遗留常识：\"{}\"", name, last_words))
    } else {
        anyhow::bail!("未知的操作类型（仅支持 release 或 execute）。")
    }
}
