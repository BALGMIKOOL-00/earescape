use chrono::NaiveDate;
use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub database_path: String,
    pub llm_provider: String,
    pub gemini_api_key: Option<String>,
    pub gemini_chat_model: String,
    pub gemini_embed_model: String,
    pub reference_date: NaiveDate,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(3000);

        let database_path =
            env::var("DATABASE_PATH").unwrap_or_else(|_| "data/earescape.db".to_string());

        let llm_provider = env::var("LLM_PROVIDER").unwrap_or_else(|_| "gemini".to_string());

        let gemini_api_key = env::var("GEMINI_API_KEY")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let gemini_chat_model =
            env::var("GEMINI_CHAT_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());

        let gemini_embed_model =
            env::var("GEMINI_EMBED_MODEL").unwrap_or_else(|_| "text-embedding-004".to_string());

        let reference_date = env::var("REFERENCE_DATE")
            .ok()
            .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .unwrap_or_else(|| chrono::Local::now().date_naive());

        Self {
            port,
            database_path,
            llm_provider,
            gemini_api_key,
            gemini_chat_model,
            gemini_embed_model,
            reference_date,
        }
    }
}
