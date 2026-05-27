//! Timestamp and time range types for temporal hypergraph operations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A monotonically increasing timestamp.
///
/// Timestamps are used to track when events occurred in the temporal hypergraph.
/// They can be either logical (incrementing counter) or wall-clock based.
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Timestamp(pub(crate) u64);

impl Timestamp {
    /// Create a new timestamp from a raw value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Get the raw timestamp value.
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// Create a timestamp from the current wall-clock time (microseconds since Unix epoch).
    pub fn now() -> Self {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards");
        Self(duration.as_micros() as u64)
    }

    /// The epoch timestamp (0).
    pub const EPOCH: Timestamp = Timestamp(0);

    /// The maximum possible timestamp.
    pub const MAX: Timestamp = Timestamp(u64::MAX);
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::EPOCH
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// A range of timestamps (half-open interval: [start, end)).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeRange {
    /// The start of the range (inclusive). None means unbounded start.
    pub start: Option<Timestamp>,
    /// The end of the range (exclusive). None means unbounded end.
    pub end: Option<Timestamp>,
}

impl TimeRange {
    /// Create a new time range.
    pub const fn new(start: Option<Timestamp>, end: Option<Timestamp>) -> Self {
        Self { start, end }
    }

    /// Create an unbounded range (all time).
    pub const fn all() -> Self {
        Self {
            start: None,
            end: None,
        }
    }

    /// Create a range from start to infinity.
    pub const fn from(start: Timestamp) -> Self {
        Self {
            start: Some(start),
            end: None,
        }
    }

    /// Create a range from the beginning to end.
    pub const fn until(end: Timestamp) -> Self {
        Self {
            start: None,
            end: Some(end),
        }
    }

    /// Create a bounded range [start, end).
    pub const fn between(start: Timestamp, end: Timestamp) -> Self {
        Self {
            start: Some(start),
            end: Some(end),
        }
    }

    /// Check if a timestamp is contained within this range.
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        let after_start = match self.start {
            Some(s) => timestamp >= s,
            None => true,
        };
        let before_end = match self.end {
            Some(e) => timestamp < e,
            None => true,
        };
        after_start && before_end
    }

    /// Check if this range overlaps with another.
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        let start1 = self.start.unwrap_or(Timestamp::EPOCH);
        let end1 = self.end.unwrap_or(Timestamp::MAX);
        let start2 = other.start.unwrap_or(Timestamp::EPOCH);
        let end2 = other.end.unwrap_or(Timestamp::MAX);

        start1 < end2 && start2 < end1
    }
}

impl Default for TimeRange {
    fn default() -> Self {
        Self::all()
    }
}

/// A logical clock for generating monotonically increasing timestamps.
///
/// This is thread-safe and can be shared across async tasks.
pub struct Clock {
    counter: AtomicU64,
}

impl Clock {
    /// Create a new clock starting at 0.
    pub const fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }

    /// Create a clock starting at a specific value.
    pub fn starting_at(value: u64) -> Self {
        Self {
            counter: AtomicU64::new(value),
        }
    }

    /// Get the next timestamp, incrementing the clock.
    pub fn tick(&self) -> Timestamp {
        Timestamp(self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the current timestamp without incrementing.
    pub fn current(&self) -> Timestamp {
        Timestamp(self.counter.load(Ordering::Acquire))
    }

    /// Advance the clock to at least the given timestamp.
    /// Returns the new current value.
    pub fn advance_to(&self, timestamp: Timestamp) -> Timestamp {
        loop {
            let current = self.counter.load(Ordering::Acquire);
            if current >= timestamp.0 {
                return Timestamp(current);
            }
            if self
                .counter
                .compare_exchange(current, timestamp.0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return timestamp;
            }
        }
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_ordering() {
        assert!(Timestamp(1) < Timestamp(2));
        assert!(Timestamp(10) > Timestamp(5));
        assert_eq!(Timestamp(5), Timestamp(5));
    }

    #[test]
    fn time_range_contains() {
        let range = TimeRange::between(Timestamp(10), Timestamp(20));
        assert!(!range.contains(Timestamp(9)));
        assert!(range.contains(Timestamp(10)));
        assert!(range.contains(Timestamp(15)));
        assert!(!range.contains(Timestamp(20)));
        assert!(!range.contains(Timestamp(21)));
    }

    #[test]
    fn time_range_unbounded() {
        let all = TimeRange::all();
        assert!(all.contains(Timestamp(0)));
        assert!(all.contains(Timestamp(u64::MAX)));

        let from = TimeRange::from(Timestamp(10));
        assert!(!from.contains(Timestamp(9)));
        assert!(from.contains(Timestamp(10)));
        assert!(from.contains(Timestamp(u64::MAX)));

        let until = TimeRange::until(Timestamp(10));
        assert!(until.contains(Timestamp(0)));
        assert!(until.contains(Timestamp(9)));
        assert!(!until.contains(Timestamp(10)));
    }

    #[test]
    fn clock_tick() {
        let clock = Clock::new();
        assert_eq!(clock.tick(), Timestamp(0));
        assert_eq!(clock.tick(), Timestamp(1));
        assert_eq!(clock.tick(), Timestamp(2));
        assert_eq!(clock.current(), Timestamp(3));
    }

    #[test]
    fn clock_advance() {
        let clock = Clock::new();
        clock.tick(); // 0
        clock.tick(); // 1
        assert_eq!(clock.advance_to(Timestamp(10)), Timestamp(10));
        assert_eq!(clock.tick(), Timestamp(10));
        assert_eq!(clock.current(), Timestamp(11));
    }
}
