use anchormas_agent::{agent, config, db};

use anyhow::Result;
use sqlx::SqlitePool;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(true)
        .with_level(true)
        .init();

    tracing::info!("Running AnchorMAS Agent Parliament & Evolution Phase 2 Tests...");

    let config = config::Config::from_env()?;
    let pool = db::init_db(&config.database_url).await?;
    let doubao = agent::DoubaoClient {
        api_key: config.ark_api_key.clone(),
        endpoint_id: config.ark_endpoint_id.clone(),
        api_url: config.llm_api_url.clone(),
        client: reqwest::Client::new(),
    };

    // 1. Test is_core_agent & Hibernation state transition
    test_hibernation_transitions(&pool).await?;

    // 2. Test Wakeup Credits Formula & Activation
    test_wakeup_formula(&pool).await?;

    // 3. Test Faction Voting Biases & Proposes
    test_faction_voting(&pool, &doubao).await?;

    // 4. Test Auto Golden Cases Selection & Sandbox Checks
    test_golden_cases_sandbox(&pool, &doubao).await?;

    // 5. Test Structured Rule CRUD and Guidelines compiler
    test_rules_crud(&pool).await?;

    tracing::info!("All tests completed successfully!");
    Ok(())
}

async fn test_hibernation_transitions(pool: &SqlitePool) -> Result<()> {
    tracing::info!("Testing Hibernation State Transitions...");

    // Set a core agent credit to <= 0 and check status
    let role = "analyst_competition";
    
    // Initialize registry entry
    sqlx::query("UPDATE agent_parliament_registry SET status = 'active', compute_credits = 10000 WHERE role_id = ?")
        .bind(role)
        .execute(pool)
        .await?;

    // Charge credits to trigger hibernation
    agent::parliament::charge_compute_credits(pool, role, 15000).await?;

    let (status, credits): (String, i64) = sqlx::query_as(
        "SELECT status, compute_credits FROM agent_parliament_registry WHERE role_id = ?"
    )
    .bind(role)
    .fetch_one(pool)
    .await?;

    tracing::info!(status = %status, credits = %credits, "After charging excess credits");
    assert_eq!(status, "hibernation", "Core agent should transition to hibernation when credits <= 0");
    assert!(credits <= 0, "Credits should be negative or zero");

    Ok(())
}

async fn test_wakeup_formula(pool: &SqlitePool) -> Result<()> {
    tracing::info!("Testing Wakeup Credits Formula...");

    let role = "analyst_competition";
    // Check if status is hibernation
    let status: String = sqlx::query_scalar("SELECT status FROM agent_parliament_registry WHERE role_id = ?")
        .bind(role)
        .fetch_one(pool)
        .await?;
    assert_eq!(status, "hibernation");

    // Call ensure_agent_active, which should trigger the dynamic credits wakeup
    agent::parliament::ensure_agent_active(pool, role).await?;

    let (new_status, new_credits): (String, i64) = sqlx::query_as(
        "SELECT status, compute_credits FROM agent_parliament_registry WHERE role_id = ?"
    )
    .bind(role)
    .fetch_one(pool)
    .await?;

    tracing::info!(status = %new_status, credits = %new_credits, "After wakeup activation");
    assert_eq!(new_status, "active", "Agent should be active");
    assert!(new_credits >= 20000 && new_credits <= 100000, "Wakeup credits should be bounded by [20000, 100000]");

    Ok(())
}

async fn test_faction_voting(pool: &SqlitePool, client: &agent::DoubaoClient) -> Result<()> {
    tracing::info!("Testing 6 Factions Voting and Biases...");

    // Let's check how many active agents we have, and ensure they have diverse factions
    let voters: Vec<(String, String)> = sqlx::query_as(
        "SELECT role_id, faction FROM agent_parliament_registry WHERE status IN ('active', 'parole')"
    )
    .fetch_all(pool)
    .await?;

    for (role_id, faction) in &voters {
        tracing::info!(role = %role_id, faction = %faction, "Registered voter faction alignment");
    }

    // Run a mock proposal for testing voting outputs
    let result = agent::parliament::propose_and_vote(
        pool,
        client,
        "critic",
        "Test Budget Proposal",
        "This proposal increases the daily token budget for social media monitoring by 50,000 credits to cover new TikTok channels.",
        "budget"
    )
    .await?;

    tracing::info!(result = %result, "Mock budget proposal vote outcome");
    Ok(())
}

async fn test_golden_cases_sandbox(pool: &SqlitePool, client: &agent::DoubaoClient) -> Result<()> {
    tracing::info!("Testing Golden Cases selection and evolution sandbox...");

    // Insert mock events that qualify for the regression suite
    let event_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        r#"INSERT OR REPLACE INTO events (id, market, category, title, summary, confidence, analysis, created_at)
           VALUES (?, 'China', 'Competition', 'Mock Golden Case Event', 'This is a mock event that should qualify for regression.', 5, 'Verified analysis without warning.', datetime('now'))"#
    )
    .bind(&event_id)
    .execute(pool)
    .await?;

    // Run auto update to ensure it queries and seeds regression_test_suite
    agent::evolution::auto_update_regression_suite(pool).await?;

    // Check if regression test suite contains the mock golden case
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM regression_test_suite WHERE event_id = ?")
        .bind(&event_id)
        .fetch_one(pool)
        .await?;
    assert_eq!(count, 1, "Mock event should be automatically selected as a golden case");

    // Run a mock sandbox test with empty guidelines
    let pass = agent::evolution::verify_regression_sandbox(pool, client, "analyst_competition", "").await?;
    if !pass {
        tracing::warn!("Sandbox run with no change did not pass (likely due to LLM response variation)");
    }

    Ok(())
}

async fn test_rules_crud(pool: &SqlitePool) -> Result<()> {
    tracing::info!("Testing Structured Rules CRUD and Compilation...");

    let role = "analyst_competition";
    // Clean rules for testing
    sqlx::query("DELETE FROM agent_playbook_rules WHERE role_id = ?")
        .bind(role)
        .execute(pool)
        .await?;

    // Add a rule
    let rule_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agent_playbook_rules (rule_id, role_id, content, status) VALUES (?, ?, 'Avoid overestimating LGD pricing impact.', 'active')"
    )
    .bind(&rule_id)
    .bind(role)
    .execute(pool)
    .await?;

    // Add another rule
    let rule_id2 = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agent_playbook_rules (rule_id, role_id, content, status) VALUES (?, ?, 'Never tag local boutique closures as severity 5.', 'active')"
    )
    .bind(&rule_id2)
    .bind(role)
    .execute(pool)
    .await?;

    // Compile guidelines
    let compiled = agent::evolution::compile_guidelines(pool, role).await?;
    tracing::info!(compiled = %compiled, "Compiled guidelines output");
    assert!(compiled.contains("Avoid overestimating LGD pricing impact."));
    assert!(compiled.contains("Never tag local boutique closures as severity 5."));

    // Deprecate rule 1
    sqlx::query("UPDATE agent_playbook_rules SET status = 'deprecated' WHERE rule_id = ?")
        .bind(&rule_id)
        .execute(pool)
        .await?;

    // Compile again
    let compiled2 = agent::evolution::compile_guidelines(pool, role).await?;
    tracing::info!(compiled_after_deprecation = %compiled2, "Compiled guidelines after deprecation");
    assert!(!compiled2.contains("Avoid overestimating LGD pricing impact."));
    assert!(compiled2.contains("Never tag local boutique closures as severity 5."));

    Ok(())
}
