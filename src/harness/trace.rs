//! A decision trace: [`TracedPolicy`] wraps one policy and records every
//! `should_join` / `should_leave` result as a [`TraceEntry`], in call order.

use std::sync::Mutex;

use crate::algorithms::AgentCapabilities;
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext, TaskStart};

/// One recorded membership decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    /// `true` for a `should_leave` call, `false` for a `should_join` call.
    pub leave: bool,
    /// The decision's `act`.
    pub act: bool,
    /// The decision's `score` as `f64::to_bits`.
    pub score_bits: u64,
}

/// A policy that forwards `should_join`, `should_leave`, `begin_task` and
/// `observe_outcome` to `inner` unchanged and appends one [`TraceEntry`] per
/// `should_join` / `should_leave` call.
pub struct TracedPolicy<'a> {
    inner: &'a dyn CoalitionDecisionPolicy,
    entries: Mutex<Vec<TraceEntry>>,
}

impl<'a> TracedPolicy<'a> {
    /// Wrap `inner` with an empty trace.
    #[must_use]
    pub fn new(inner: &'a dyn CoalitionDecisionPolicy) -> Self {
        Self {
            inner,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// The recorded decisions, in call order.
    ///
    /// # Panics
    ///
    /// Only on a poisoned trace mutex.
    #[must_use]
    pub fn entries(&self) -> Vec<TraceEntry> {
        self.entries
            .lock()
            .expect("invariant: no panic holds the trace lock")
            .clone()
    }

    fn record(&self, leave: bool, decision: Decision) -> Decision {
        self.entries
            .lock()
            .expect("invariant: no panic holds the trace lock")
            .push(TraceEntry {
                leave,
                act: decision.act,
                score_bits: decision.score.to_bits(),
            });
        decision
    }
}

impl CoalitionDecisionPolicy for TracedPolicy<'_> {
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        self.record(false, self.inner.should_join(agent, coalition, ctx))
    }

    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        self.record(true, self.inner.should_leave(agent, coalition, ctx))
    }

    fn begin_task(&self, task: &TaskStart<'_>) {
        self.inner.begin_task(task);
    }

    fn observe_outcome(&self, required: u32, per_bit_success: &[bool]) {
        self.inner.observe_outcome(required, per_bit_success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::CapabilityAgent;

    /// One call the inner policy received.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Join(usize),
        Leave(usize),
        Begin { required: u32, steps: Vec<(u8, u8)> },
        Outcome { required: u32, per_bit: Vec<bool> },
    }

    /// Answers call `k` (joins and leaves counted together) with `script[k]`
    /// and records every call.
    struct Scripted {
        script: Vec<Decision>,
        calls: Mutex<Vec<Call>>,
    }

    impl Scripted {
        fn new(script: Vec<Decision>) -> Self {
            Self {
                script,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn answer(&self, call: Call) -> Decision {
            let mut calls = self.calls.lock().unwrap();
            let k = calls
                .iter()
                .filter(|c| matches!(c, Call::Join(_) | Call::Leave(_)))
                .count();
            calls.push(call);
            self.script[k]
        }
    }

    impl CoalitionDecisionPolicy for Scripted {
        fn should_join(
            &self,
            agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.answer(Call::Join(agent.agent_id()))
        }

        fn should_leave(
            &self,
            agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.answer(Call::Leave(agent.agent_id()))
        }

        fn begin_task(&self, task: &TaskStart<'_>) {
            self.calls.lock().unwrap().push(Call::Begin {
                required: task.required,
                steps: task.steps.to_vec(),
            });
        }

        fn observe_outcome(&self, required: u32, per_bit_success: &[bool]) {
            self.calls.lock().unwrap().push(Call::Outcome {
                required,
                per_bit: per_bit_success.to_vec(),
            });
        }
    }

    fn decision(act: bool, score: f64) -> Decision {
        Decision { act, score }
    }

    #[test]
    fn entries_follow_call_order_and_decisions_pass_through() {
        let inner = Scripted::new(vec![
            decision(true, 0.25),
            decision(false, -0.5),
            decision(true, 0.125),
        ]);
        let traced = TracedPolicy::new(&inner);
        let a = CapabilityAgent::new(3, 0b01, 50);
        let b = CapabilityAgent::new(4, 0b10, 50);
        let ctx = DecisionContext::default();

        assert!(traced.entries().is_empty());
        let returned = [
            traced.should_join(&a, &[], &ctx),
            traced.should_leave(&b, &[&b], &ctx),
            traced.should_join(&b, &[&a], &ctx),
        ];
        assert_eq!(
            returned,
            [
                decision(true, 0.25),
                decision(false, -0.5),
                decision(true, 0.125)
            ]
        );
        assert_eq!(
            traced.entries(),
            vec![
                TraceEntry {
                    leave: false,
                    act: true,
                    score_bits: 0.25f64.to_bits()
                },
                TraceEntry {
                    leave: true,
                    act: false,
                    score_bits: (-0.5f64).to_bits()
                },
                TraceEntry {
                    leave: false,
                    act: true,
                    score_bits: 0.125f64.to_bits()
                },
            ]
        );
        assert_eq!(
            *inner.calls.lock().unwrap(),
            vec![Call::Join(3), Call::Leave(4), Call::Join(4)]
        );
    }

    #[test]
    fn both_hooks_are_forwarded_and_record_no_entry() {
        let inner = Scripted::new(Vec::new());
        let traced = TracedPolicy::new(&inner);
        let steps = [(0u8, 1u8), (2, 0)];
        traced.begin_task(&TaskStart {
            required: 0b101,
            steps: &steps,
        });
        traced.observe_outcome(0b101, &[true, false, true]);
        assert_eq!(
            *inner.calls.lock().unwrap(),
            vec![
                Call::Begin {
                    required: 0b101,
                    steps: vec![(0, 1), (2, 0)]
                },
                Call::Outcome {
                    required: 0b101,
                    per_bit: vec![true, false, true]
                },
            ]
        );
        assert!(traced.entries().is_empty());
    }

    #[test]
    fn score_bits_separate_values_that_compare_equal() {
        let inner = Scripted::new(vec![decision(false, 0.0), decision(false, -0.0)]);
        let traced = TracedPolicy::new(&inner);
        let a = CapabilityAgent::new(0, 0b1, 50);
        let ctx = DecisionContext::default();
        let _ = traced.should_join(&a, &[], &ctx);
        let _ = traced.should_join(&a, &[], &ctx);
        let bits: Vec<u64> = traced.entries().iter().map(|e| e.score_bits).collect();
        assert_eq!(bits, vec![0u64, 1u64 << 63]);
    }
}
