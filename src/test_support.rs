//! Helpers shared by the crate's unit tests.

/// A `tracing` layer that records every event's level and fields, for tests
/// that pin a log site.
pub(crate) mod log_capture {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::{Event, Level, Subscriber};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    /// One captured event: its level and its fields by name (`message`
    /// included), each rendered as text.
    #[derive(Debug, Clone)]
    pub(crate) struct CapturedEvent {
        pub(crate) level: Level,
        pub(crate) fields: HashMap<&'static str, String>,
    }

    /// The shared event list; clones record into the same list.
    #[derive(Clone, Default)]
    pub(crate) struct Capture(Arc<Mutex<Vec<CapturedEvent>>>);

    impl Capture {
        /// Make this capture the current thread's subscriber until the guard
        /// drops.
        pub(crate) fn install(&self) -> tracing::subscriber::DefaultGuard {
            tracing::subscriber::set_default(tracing_subscriber::registry().with(self.clone()))
        }

        /// The captured events whose `message` contains `needle`.
        pub(crate) fn matching(&self, needle: &str) -> Vec<CapturedEvent> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.fields.get("message").is_some_and(|m| m.contains(needle)))
                .cloned()
                .collect()
        }
    }

    struct Fields<'a>(&'a mut HashMap<&'static str, String>);

    impl Visit for Fields<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name(), value.to_string());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name(), format!("{value:?}"));
        }
    }

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = HashMap::new();
            event.record(&mut Fields(&mut fields));
            self.0.lock().unwrap().push(CapturedEvent {
                level: *event.metadata().level(),
                fields,
            });
        }
    }
}
