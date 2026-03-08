use std::env;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Config {
    pub environment: String,
    pub app_name: String,
    pub log_level: String,
    pub port: u16,
    pub store_backend: String,
    pub devices_table: String,
    pub messages_table: String,
    pub sqlite_db_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        let port_str = env::var("PORT")
            .or_else(|_| env::var("AWS_LWA_PORT"))
            .unwrap_or_else(|_| "8000".into());
        let port: u16 = port_str
            .parse()
            .map_err(|e| AppError::Config(format!("invalid port '{port_str}': {e}")))?;

        Ok(Self {
            environment: env::var("ENVIRONMENT").unwrap_or_else(|_| "dev".into()),
            app_name: env::var("APP_NAME").unwrap_or_else(|_| "supervictor".into()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
            port,
            store_backend: env::var("STORE_BACKEND").unwrap_or_else(|_| "sqlite".into()),
            devices_table: env::var("DEVICES_TABLE").unwrap_or_else(|_| "devices".into()),
            messages_table: env::var("MESSAGES_TABLE").unwrap_or_else(|_| "messages".into()),
            sqlite_db_path: env::var("SQLITE_DB_PATH").unwrap_or_else(|_| ":memory:".into()),
        })
    }
}
