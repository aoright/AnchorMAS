use anchormas_agent::{config, db, vectordb, web};

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file (ignore error if it doesn't exist)
    let _ = dotenvy::dotenv();

    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt()
        .with_target(true)
        .with_level(true)
        .init();

    tracing::info!("Starting AnchorMAS Agent: Jewelry Market Strategic Radar");

    // Load configuration from environment
    let config = config::Config::from_env()?;
    tracing::info!(port = config.server_port, "Configuration loaded");

    // Initialize SQLite database
    let pool = db::init_db(&config.database_url).await?;

    // Initialize Qdrant (best-effort: warn if unavailable)
    let qdrant = match vectordb::init_qdrant(&config).await {
        Ok(client) => {
            tracing::info!("Qdrant vector database connected");
            Some(client)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Qdrant unavailable, running without vector search");
            None
        }
    };

    let initial_pipeline_status = web::PipelineStatus::from_database(&pool).await;

    // Build the application state and router
    let state = web::AppState {
        pool,
        config: Arc::new(config.clone()),
        qdrant: qdrant.map(Arc::new),
        pipeline_status: Arc::new(tokio::sync::RwLock::new(initial_pipeline_status)),
    };

    let router = web::build_router(state);

    // Start the HTTP server
    let addr = format!("0.0.0.0:{}", config.server_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(address = %addr, "Server listening");

    axum::serve(listener, router).await?;

    Ok(())
}
