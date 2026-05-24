use anyhow::{Context, Result};

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub ark_api_key: String,
    pub ark_endpoint_id: String,
    pub database_url: String,
    pub server_port: u16,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub llm_api_url: String,
    pub embedding_api_url: String,
    pub embedding_model: String,
    pub tts_model: String,
    pub tts_voice: String,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let ark_api_key =
            std::env::var("ARK_API_KEY").context("ARK_API_KEY must be set")?;
        let ark_endpoint_id =
            std::env::var("ARK_ENDPOINT_ID").context("ARK_ENDPOINT_ID must be set")?;
        let database_url =
            std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
        let server_port: u16 = std::env::var("SERVER_PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .context("SERVER_PORT must be a valid u16")?;
        let qdrant_url =
            std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
        let qdrant_collection =
            std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "market_intelligence".to_string());
        let llm_api_url = std::env::var("LLM_API_URL").unwrap_or_else(|_| {
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string()
        });
        let embedding_api_url = std::env::var("EMBEDDING_API_URL").unwrap_or_else(|_| {
            "https://dashscope.aliyuncs.com/compatible-mode/v1/embeddings".to_string()
        });
        let embedding_model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| {
            "text-embedding-v3".to_string()
        });
        let tts_model = std::env::var("TTS_MODEL").unwrap_or_else(|_| {
            "cosyvoice-v3-flash".to_string()
        });
        let tts_voice = std::env::var("TTS_VOICE").unwrap_or_else(|_| {
            "longxiaochun_v3".to_string()
        });

        Ok(Self {
            ark_api_key,
            ark_endpoint_id,
            database_url,
            server_port,
            qdrant_url,
            qdrant_collection,
            llm_api_url,
            embedding_api_url,
            embedding_model,
            tts_model,
            tts_voice,
        })
    }
}
