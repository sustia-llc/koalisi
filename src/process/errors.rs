//! Error type for the process layer.
//!
//! Hand-rolled `Display` + [`std::error::Error`] + `From<CatgraphError>`, in the
//! same style as `topology::errors` and `persistence::errors` — no `thiserror`,
//! no `anyhow`. (Plain code spans rather than intra-doc links: `topology::errors`
//! is private and `persistence::errors` only exists under its own feature, so a
//! link would break this module's own rustdoc build.)

use catgraph_magnitude::CatgraphError;

/// What can go wrong while verifying a declared writing.
///
/// Upstream failures are **propagated, never panicked on**: a task whose
/// optimization or verification fails is a decline-and-count at the battery
/// level (prereg §4 placement), not an aborted run.
#[derive(Debug)]
pub enum ProcessError {
    /// The upstream engine rejected something — an ill-formed morphism, a step
    /// naming a rule outside the given slice, a recorded match that is not a
    /// convex match of that rule, or a readback that does not re-check.
    Catgraph(CatgraphError),

    /// The trace replayed cleanly but landed somewhere other than the
    /// representative the outcome reported.
    ///
    /// This is the S-sound gate's failure mode: an arm may not grade its own
    /// homework by declaring a writing its own trace does not derive. Defensive
    /// against an engine-invariant break rather than expected in normal
    /// operation — [`replay`] re-validates every step before applying it, so a
    /// trace that survives replay should land on the reported content.
    ///
    /// [`replay`]: catgraph_applied::prop::presentation::rewrite::replay
    Unsound {
        /// Human-readable detail naming what differed.
        message: String,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Catgraph(inner) => write!(f, "process: upstream rejection: {inner}"),
            Self::Unsound { message } => write!(f, "process: unsound declared writing: {message}"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Catgraph(inner) => Some(inner),
            Self::Unsound { .. } => None,
        }
    }
}

impl From<CatgraphError> for ProcessError {
    fn from(inner: CatgraphError) -> Self {
        Self::Catgraph(inner)
    }
}
