//! `setup_logging` after another global subscriber is installed. The installed
//! subscriber is process-global, so this binary holds this one test only.

use koalisi::core::config::{LoggingError, setup_logging};

#[test]
fn setup_logging_reports_an_installed_subscriber() {
    tracing_subscriber::fmt()
        .try_init()
        .expect("the first subscriber in this process installs");

    let first = setup_logging();
    assert!(
        matches!(first, Err(LoggingError::AlreadyInstalled { .. })),
        "setup_logging over an installed subscriber returned {first:?}"
    );
    let second = setup_logging();
    assert_eq!(second, first, "a second setup_logging call");
}
