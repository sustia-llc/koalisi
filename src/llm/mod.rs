//! LLM integration surface (stub).
//!
//! Phase 5 (SwarmAgentic-style optimisation) and Phase 6 (Active
//! Inference decision layer) both depend on an LLM provider —
//! SwarmAgentic for flaw analysis and velocity rewrites, the decision
//! layer for the LLM-augmented belief updates discussed in
//! `docs/SwarmAgentic-summary.md` §"Relevance to koalisi" idea #2.
//!
//! This module ships the trait surface so plan documents and future
//! code can reference `LlmProvider::complete(prompt)` without
//! committing to a backend. Concrete backends (OpenAI / Anthropic /
//! Ollama / local llama.cpp) land later behind a future `llm` feature
//! flag with per-backend sub-features.

use anyhow::Result;
use std::future::Future;

/// Generic LLM prompt → completion interface.
///
/// Backends are async because every realistic implementation hits the
/// network or a long-running local process. Returns `impl Future + Send`
/// rather than `async fn` so the `Send` bound on the returned future is
/// explicit and survives across trait-object boundaries (relevant when
/// the provider is captured by an actor or a `spawn`ed task).
pub trait LlmProvider: Send + Sync {
    fn complete(&self, prompt: &str) -> impl Future<Output = Result<String>> + Send;
}

/// Placeholder provider that always returns an error.
///
/// Surfaces "no LLM backend wired yet" cleanly during development so a
/// missing backend manifests as a typed error instead of a silent
/// no-op. Replace with a real backend once Phase 5 lands.
pub struct StubLlmProvider;

impl LlmProvider for StubLlmProvider {
    async fn complete(&self, _prompt: &str) -> Result<String> {
        anyhow::bail!(
            "StubLlmProvider: no LLM backend configured yet \
             (Phase 5 will wire OpenAI / Anthropic / Ollama / llama.cpp \
             behind the `llm` feature)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_error() {
        let provider = StubLlmProvider;
        let result = provider.complete("hello").await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no LLM backend"));
    }
}
