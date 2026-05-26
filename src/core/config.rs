use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::env;
use std::str::FromStr;
use std::sync::{LazyLock, Once};
use tracing::Level;

pub static SETTINGS: LazyLock<Settings> =
    LazyLock::new(|| Settings::new().expect("Failed to setup settings"));

#[derive(Debug, Clone, Deserialize)]
pub struct Logger {
    pub level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoalitionSettings {
    pub threshold_bps: f64,
    pub history_capacity: usize,
    /// "guaranteed" or "best_effort"
    pub delivery_strategy: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub environment: String,
    pub logger: Logger,
    // Accept both "swarm" and "coalition" keys from config files for
    // backward compatibility during the rename transition.
    #[serde(alias = "swarm")]
    pub coalition: CoalitionSettings,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let builder = Config::builder()
            .add_source(File::with_name("config/default"))
            .add_source(File::with_name(&format!("config/{run_mode}")).required(false))
            .add_source(Environment::default().separator("__"));

        builder.build()?.try_deserialize()
    }
}

static INIT: Once = Once::new();

/// Initialise `tracing_subscriber` from `SETTINGS.logger.level`.
///
/// Idempotent: calling this multiple times (e.g. from `main` and from each
/// integration test) is safe — only the first call installs a global
/// subscriber.
pub fn setup_logging() {
    INIT.call_once(|| {
        let level = Level::from_str(SETTINGS.logger.level.as_str()).unwrap_or_else(|_| {
            eprintln!(
                "Invalid log level: {}, defaulting to INFO",
                SETTINGS.logger.level
            );
            Level::INFO
        });

        let _ = tracing_subscriber::fmt()
            .with_max_level(level)
            .with_test_writer()
            .try_init();
    });
}
