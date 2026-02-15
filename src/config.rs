use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub clickhouse: ClickHouseConfig,
    pub exex: ExExConfig,
    pub backfill: BackfillConfig,
    pub cursor: CursorConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClickHouseConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub database: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExExConfig {
    pub buffer_size: usize,
    pub flush_interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackfillConfig {
    pub enabled: bool,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CursorConfig {
    pub file_path: String,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .set_default("clickhouse.url", "http://localhost:8123")?
            .set_default("clickhouse.username", "default")?
            .set_default("clickhouse.password", "")?
            .set_default("clickhouse.database", "shadow_index")?
            .set_default("exex.buffer_size", 10_000)?
            .set_default("exex.flush_interval_ms", 100)?
            .set_default("backfill.enabled", true)?
            .set_default("backfill.batch_size", 100)?
            .set_default("cursor.file_path", "shadow-index.cursor")?
            .add_source(File::with_name("config.toml").required(false))
            .add_source(
                Environment::with_prefix("SHADOW_INDEX")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        let app_config: AppConfig = config.try_deserialize()?;

        app_config.validate()?;

        Ok(app_config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.clickhouse.url.trim().is_empty() {
            return Err(ConfigError::Message(
                "ClickHouse URL cannot be empty".to_string(),
            ));
        }

        if self.clickhouse.database.trim().is_empty() {
            return Err(ConfigError::Message(
                "ClickHouse database name cannot be empty".to_string(),
            ));
        }

        if self.exex.buffer_size == 0 {
            return Err(ConfigError::Message(
                "ExEx buffer_size must be greater than 0".to_string(),
            ));
        }

        if self.exex.flush_interval_ms == 0 {
            return Err(ConfigError::Message(
                "ExEx flush_interval_ms must be greater than 0".to_string(),
            ));
        }

        if self.backfill.enabled && self.backfill.batch_size == 0 {
            return Err(ConfigError::Message(
                "Backfill batch_size must be greater than 0 when backfill is enabled".to_string(),
            ));
        }

        if self.cursor.file_path.trim().is_empty() {
            return Err(ConfigError::Message(
                "Cursor file_path cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl ClickHouseConfig {
    pub fn connection_url(&self) -> String {
        if self.username.is_empty() && self.password.is_empty() {
            self.url.clone()
        } else {
            format!(
                "{}?user={}&password={}",
                self.url, self.username, self.password
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation_valid() {
        let config = AppConfig {
            clickhouse: ClickHouseConfig {
                url: "http://localhost:8123".to_string(),
                username: "default".to_string(),
                password: "".to_string(),
                database: "shadow_index".to_string(),
            },
            exex: ExExConfig {
                buffer_size: 10_000,
                flush_interval_ms: 100,
            },
            backfill: BackfillConfig {
                enabled: true,
                batch_size: 100,
            },
            cursor: CursorConfig {
                file_path: "shadow-index.cursor".to_string(),
            },
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_empty_url() {
        let config = AppConfig {
            clickhouse: ClickHouseConfig {
                url: "".to_string(),
                username: "default".to_string(),
                password: "".to_string(),
                database: "shadow_index".to_string(),
            },
            exex: ExExConfig {
                buffer_size: 10_000,
                flush_interval_ms: 100,
            },
            backfill: BackfillConfig {
                enabled: true,
                batch_size: 100,
            },
            cursor: CursorConfig {
                file_path: "shadow-index.cursor".to_string(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_buffer_size() {
        let config = AppConfig {
            clickhouse: ClickHouseConfig {
                url: "http://localhost:8123".to_string(),
                username: "default".to_string(),
                password: "".to_string(),
                database: "shadow_index".to_string(),
            },
            exex: ExExConfig {
                buffer_size: 0,
                flush_interval_ms: 100,
            },
            backfill: BackfillConfig {
                enabled: true,
                batch_size: 100,
            },
            cursor: CursorConfig {
                file_path: "shadow-index.cursor".to_string(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_connection_url_without_credentials() {
        let config = ClickHouseConfig {
            url: "http://localhost:8123".to_string(),
            username: "".to_string(),
            password: "".to_string(),
            database: "shadow_index".to_string(),
        };

        assert_eq!(config.connection_url(), "http://localhost:8123");
    }

    #[test]
    fn test_connection_url_with_credentials() {
        let config = ClickHouseConfig {
            url: "http://localhost:8123".to_string(),
            username: "admin".to_string(),
            password: "secret".to_string(),
            database: "shadow_index".to_string(),
        };

        assert_eq!(
            config.connection_url(),
            "http://localhost:8123?user=admin&password=secret"
        );
    }
}
