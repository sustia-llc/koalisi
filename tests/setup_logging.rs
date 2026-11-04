//! `setup_logging` after another global subscriber is installed. The installed
//! subscriber is process-global, so this binary holds this one test only.

use std::error::Error;
use std::sync::Arc;

use koalisi::core::config::{LoggingError, setup_logging};

/// Over an installed subscriber, `setup_logging` returns `AlreadyInstalled`
/// whose `source()` is the `TryInitError`, and a second call returns the same
/// error, sharing its source.
#[test]
fn setup_logging_reports_an_installed_subscriber() {
    tracing_subscriber::fmt()
        .try_init()
        .expect("the first subscriber in this process installs");

    let first = setup_logging();
    let Err(LoggingError::AlreadyInstalled {
        source: first_source,
    }) = &first
    else {
        panic!("setup_logging over an installed subscriber returned {first:?}");
    };
    let chained = first
        .as_ref()
        .err()
        .and_then(|e| e.source())
        .map(ToString::to_string);
    assert_eq!(
        chained,
        Some(first_source.to_string()),
        "Error::source of the first error"
    );
    let second = setup_logging();
    assert!(
        matches!(
            &second,
            Err(LoggingError::AlreadyInstalled { source }) if Arc::ptr_eq(source, first_source)
        ),
        "a second setup_logging call returned {second:?}, the first {first:?}"
    );
}
