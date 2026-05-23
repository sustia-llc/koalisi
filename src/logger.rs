use crate::settings::SETTINGS;
use std::str::FromStr;
use std::sync::Once;
use tracing::Level;

static INIT: Once = Once::new();

/// Initialise `tracing_subscriber` from `SETTINGS.logger.level`.
///
/// Idempotent: calling this multiple times (e.g. from `main` and from each
/// integration test) is safe — only the first call installs a global
/// subscriber. Tests that pass `cargo test` in parallel share one subscriber.
pub fn setup() {
    INIT.call_once(|| {
        let level = Level::from_str(SETTINGS.logger.level.as_str()).unwrap_or_else(|_| {
            eprintln!(
                "Invalid log level: {}, defaulting to INFO",
                SETTINGS.logger.level
            );
            Level::INFO
        });

        // `try_init` because something upstream (test harness, kameo's own
        // setup, etc.) may have already installed a subscriber.
        let _ = tracing_subscriber::fmt()
            .with_max_level(level)
            .with_test_writer()
            .try_init();
    });
}
