use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Deserializer};
use std::env;
use std::str::FromStr;
use std::sync::{LazyLock, OnceLock};
use tracing::level_filters::LevelFilter;

/// The mode file loaded when `RUN_MODE` is unset.
pub const DEFAULT_RUN_MODE: &str = "development";

/// The prefix of the environment variables that `Settings::new` reads.
pub const ENV_PREFIX: &str = "KOALISI";

/// The process-wide settings, loaded by `Settings::new` on first access.
///
/// A load failure panics with a message that names the working directory,
/// the value of `RUN_MODE` and the `ConfigError`.
pub static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    Settings::new().unwrap_or_else(|error| panic!("{}", load_failure_message(&error)))
});

/// The `[logger]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct Logger {
    /// The maximum level `setup_logging` installs. It accepts `off`, `error`,
    /// `warn`, `info`, `debug` and `trace` in any letter case, and the digits
    /// `0` to `5`; any other value, the empty string included, fails
    /// deserialization.
    #[serde(deserialize_with = "deserialize_level")]
    pub level: LevelFilter,
}

/// The koalisi settings.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub environment: String,
    pub logger: Logger,
}

impl Settings {
    /// Loads the settings from three layers, each overriding the one before:
    /// `config/default`, `config/<RUN_MODE>` (`RUN_MODE` unset selects
    /// `development`) and the environment variables named `KOALISI_<KEY>`,
    /// where `__` separates nested keys (`KOALISI_LOGGER__LEVEL` sets
    /// `logger.level`). Both paths are relative to the working directory.
    /// Variables without the prefix and variables with an empty value are
    /// skipped.
    ///
    /// # Errors
    ///
    /// Returns a `ConfigError` if `RUN_MODE` is not valid Unicode, if either
    /// file is missing or malformed, if a `KOALISI_` variable holds
    /// non-Unicode data, or if the merged values do not deserialize into
    /// `Settings`, an invalid `logger.level` included.
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = run_mode()?;

        Config::builder()
            .add_source(File::with_name("config/default"))
            .add_source(File::with_name(&format!("config/{run_mode}")))
            .add_source(environment())
            .build()?
            .try_deserialize()
    }
}

/// The `RUN_MODE` value, or `DEFAULT_RUN_MODE` when it is unset; a
/// non-Unicode value is a `ConfigError`.
fn run_mode() -> Result<String, ConfigError> {
    match env::var("RUN_MODE") {
        Ok(mode) => Ok(mode),
        Err(env::VarError::NotPresent) => Ok(DEFAULT_RUN_MODE.to_owned()),
        Err(env::VarError::NotUnicode(raw)) => Err(ConfigError::Message(format!(
            "RUN_MODE is not valid Unicode: {raw:?}"
        ))),
    }
}

/// The environment source of `Settings::new`.
fn environment() -> Environment {
    Environment::with_prefix(ENV_PREFIX)
        .prefix_separator("_")
        .separator("__")
        .ignore_empty(true)
}

/// Parses a level string as `Logger::level` documents, rejecting the empty
/// string that `LevelFilter::from_str` maps to `ERROR`.
fn deserialize_level<'de, D>(deserializer: D) -> Result<LevelFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw.is_empty() {
        return Err(serde::de::Error::custom("invalid logger.level: empty"));
    }
    LevelFilter::from_str(&raw)
        .map_err(|error| serde::de::Error::custom(format!("invalid logger.level {raw:?}: {error}")))
}

/// The panic message of a failed `SETTINGS` load: the working directory,
/// the `RUN_MODE` value and the error.
fn load_failure_message(error: &ConfigError) -> String {
    let cwd = env::current_dir().map_or_else(
        |cwd_error| format!("<unreadable working directory: {cwd_error}>"),
        |path| path.display().to_string(),
    );
    let run_mode = env::var_os("RUN_MODE").map_or_else(
        || format!("unset (selects {DEFAULT_RUN_MODE:?})"),
        |mode| format!("{mode:?}"),
    );
    format!("failed to load settings from {cwd}/config with RUN_MODE {run_mode}: {error}")
}

/// The error `setup_logging` returns.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoggingError {
    /// A global `tracing` subscriber or a `log` logger was installed before
    /// the first `setup_logging` call; `reason` is the `tracing-subscriber`
    /// error text.
    #[error("a global tracing subscriber or log logger is already installed: {reason}")]
    AlreadyInstalled { reason: String },
}

static LOGGING: OnceLock<Result<(), LoggingError>> = OnceLock::new();

/// Installs a global `tracing_subscriber` fmt subscriber whose maximum level
/// is `SETTINGS.logger.level`, writing through the test writer.
///
/// Only the first call in a process attempts the install; every call returns
/// the outcome of that first attempt.
///
/// # Errors
///
/// Returns `LoggingError::AlreadyInstalled` if a global subscriber or a `log`
/// logger was already installed when the first call ran.
///
/// # Panics
///
/// Panics if `SETTINGS` fails to load.
pub fn setup_logging() -> Result<(), LoggingError> {
    LOGGING
        .get_or_init(|| {
            tracing_subscriber::fmt()
                .with_max_level(SETTINGS.logger.level)
                .with_test_writer()
                .try_init()
                .map_err(|error| LoggingError::AlreadyInstalled {
                    reason: error.to_string(),
                })
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::FileFormat;

    fn deserialize_toml(toml: &str) -> Result<Settings, ConfigError> {
        Config::builder()
            .add_source(File::from_str(toml, FileFormat::Toml))
            .build()?
            .try_deserialize()
    }

    #[test]
    fn empty_logger_level_is_a_config_error() {
        let result = deserialize_toml("environment = \"x\"\n[logger]\nlevel = \"\"\n");
        let message = match result {
            Ok(settings) => panic!(
                "an empty logger.level deserialized to {:?}",
                settings.logger.level
            ),
            Err(error) => error.to_string(),
        };
        assert!(
            message.contains("invalid logger.level: empty"),
            "an empty logger.level failed with {message:?}"
        );
    }

    #[test]
    fn load_failure_message_names_cwd_and_run_mode() {
        let cwd = env::current_dir()
            .expect("current_dir")
            .display()
            .to_string();
        let message = load_failure_message(&ConfigError::Message("boom".into()));
        assert!(
            message.contains(&format!("{cwd}/config")) && message.contains("RUN_MODE"),
            "load_failure_message returned {message:?}, cwd {cwd:?}"
        );
        assert!(
            message.ends_with(": boom"),
            "load_failure_message returned {message:?}"
        );
    }
}
