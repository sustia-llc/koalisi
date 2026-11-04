//! `Settings::new` against the checkout's `config/` directory and the process
//! environment. Every test mutates the process environment, so each one holds
//! `ENV_LOCK` for its whole body and restores every variable it touched
//! through `EnvGuard`; this serializes them under `cargo test`'s threads and
//! is a no-op under nextest's process-per-test runner. Run with the package
//! root as the working directory.

use koalisi::core::config::Settings;
use std::env;
use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tracing::level_filters::LevelFilter;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Variables whose outer values would change what `Settings::new` reads.
const TOUCHED: &[&str] = &[
    "RUN_MODE",
    "KOALISI_LOGGER__LEVEL",
    "KOALISI_ENVIRONMENT",
    "LOGGER__LEVEL",
    "ENVIRONMENT",
    "NOT_KOALISI_BYTES",
];

/// Holds `ENV_LOCK`, clears every `TOUCHED` variable on creation and restores
/// their outer values on drop.
struct EnvGuard {
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let saved = TOUCHED.iter().map(|&k| (k, env::var_os(k))).collect();
        for &key in TOUCHED {
            // SAFETY: every test in this binary holds ENV_LOCK while it reads
            // or writes the environment.
            unsafe { env::remove_var(key) };
        }
        Self { saved, _lock: lock }
    }

    fn set(&self, key: &str, value: impl AsRef<OsStr>) {
        assert!(TOUCHED.contains(&key), "{key} is not restored by EnvGuard");
        // SAFETY: as in `new`; `self` holds ENV_LOCK.
        unsafe { env::set_var(key, value) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            // SAFETY: as in `new`; the lock is released after this loop.
            unsafe {
                match value {
                    Some(v) => env::set_var(key, v),
                    None => env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn settings_new_loads_the_checkout_config() {
    let _env = EnvGuard::new();
    let settings = match Settings::new() {
        Ok(settings) => settings,
        Err(error) => panic!("Settings::new failed on the checkout config: {error}"),
    };
    assert_eq!(settings.environment, "development");
    assert_eq!(settings.logger.level, LevelFilter::INFO);
}

#[test]
fn unknown_logger_level_is_a_config_error() {
    let env = EnvGuard::new();
    env.set("KOALISI_LOGGER__LEVEL", "verbose");
    match Settings::new() {
        Ok(settings) => panic!(
            "logger.level \"verbose\" loaded as {:?}",
            settings.logger.level
        ),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("invalid logger.level \"verbose\""),
                "logger.level \"verbose\" failed with {message:?}"
            );
        }
    }
}

#[test]
fn off_logger_level_is_accepted() {
    let env = EnvGuard::new();
    for raw in ["off", "OFF"] {
        env.set("KOALISI_LOGGER__LEVEL", raw);
        match Settings::new() {
            Ok(settings) => assert_eq!(
                settings.logger.level,
                LevelFilter::OFF,
                "logger.level {raw:?}"
            ),
            Err(error) => panic!("logger.level {raw:?} failed: {error}"),
        }
    }
}

#[test]
fn only_nonempty_prefixed_variables_override() {
    let env = EnvGuard::new();
    env.set("LOGGER__LEVEL", "bogus");
    env.set("ENVIRONMENT", "unprefixed");
    env.set("KOALISI_LOGGER__LEVEL", "");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        env.set("NOT_KOALISI_BYTES", OsStr::from_bytes(b"\xff"));
    }
    let settings = match Settings::new() {
        Ok(settings) => settings,
        Err(error) => panic!("unprefixed or empty variables broke Settings::new: {error}"),
    };
    assert_eq!(settings.environment, "development");
    assert_eq!(settings.logger.level, LevelFilter::INFO);

    env.set("KOALISI_ENVIRONMENT", "prefixed");
    let settings = match Settings::new() {
        Ok(settings) => settings,
        Err(error) => panic!("KOALISI_ENVIRONMENT broke Settings::new: {error}"),
    };
    assert_eq!(settings.environment, "prefixed");
}

#[test]
fn run_mode_without_a_config_file_is_a_config_error() {
    let env = EnvGuard::new();
    env.set("RUN_MODE", "no-such-mode");
    match Settings::new() {
        Ok(settings) => panic!(
            "RUN_MODE \"no-such-mode\" loaded environment {:?}",
            settings.environment
        ),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("config/no-such-mode"),
                "RUN_MODE \"no-such-mode\" failed with {message:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_run_mode_is_a_config_error() {
    use std::os::unix::ffi::OsStrExt;
    let env = EnvGuard::new();
    env.set("RUN_MODE", OsStr::from_bytes(b"dev\xff"));
    match Settings::new() {
        Ok(settings) => panic!(
            "a non-Unicode RUN_MODE loaded environment {:?}",
            settings.environment
        ),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("RUN_MODE is not valid Unicode"),
                "a non-Unicode RUN_MODE failed with {message:?}"
            );
        }
    }
}
