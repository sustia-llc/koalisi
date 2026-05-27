# SwarmAgentic — summary (arXiv:2506.15672v1, June 2025)

**Authors:** Yao Zhang, Chenyang Lin, Shijie Tang, Haokun Chen, Shijie Zhou, Yunpu Ma, Volker Tresp (LMU Munich / TU Munich / MCML)
**Code:** github.com/SwarmAgentic
**PDF on disk:** `docs/SwarmAgentic-2506.15672v1.pdf`

## TL;DR

SwarmAgentic is a framework that **generates and iteratively refines whole
multi-agent systems from a task description alone**, using Particle Swarm
Optimization (PSO) lifted into a language space: each particle is an
entire agentic system (agent set + collaboration graph) encoded in
structured text; velocity and position updates are LLM rewrites guided
by failure analysis, personal-best, and global-best references. Reports
**+261.8% relative gain over ADAS on TravelPlanner** with 5 particles
× 10 iterations (vs. ADAS's 30 iterations).

## The gap they identify — three autonomy properties

Existing agentic-system frameworks tick at most two of three boxes:

| Property | What it means |
|---|---|
| **From-Scratch Agent Generation** | The framework synthesises complete agents (roles, decision logic, internal structure) from the task description, without hard-coded operators, fixed personas, or seed agents. |
| **Self-Optimizing Agent Functionality** | An agent's own role/policy/responsibility is automatically refined during execution based on feedback, not just its outputs. |
| **Self-Optimizing Agent Collaboration** | The collaboration topology itself (sequencing, dependencies, routing) is restructured at run time. |

Per the paper's Table 1 / Appendix A: SPP, EvoAgent, AgentSquare, AutoAgents,
AFlow, Agent Symbolic Learning, ADAS — none satisfy all three. SwarmAgentic
is the only framework that does.

## How the algorithm works

### Encoding (everything in language)

A particle = a candidate agentic system `S_i^{(t)}` =
- Agent set `A_i^{(t)} = {A_{i,1}, ..., A_{i,m}}`, each `A_{i,k} = (I_{i,k}, R_{i,k}, P_{i,k})` (identifier, responsibility, execution policy)
- Collaboration structure `W_i^{(t)} = {W_{i,1}, ..., W_{i,n}}` assigning agents to ordered steps

All of this lives in **structured natural language**, not numerical
vectors. Velocity and position updates are LLM-driven text rewrites
rather than vector arithmetic — PSO without numbers.

### Four-stage loop

```
Initialization
  → Flaw Identification
    → Failure-Aware Velocity Update
      → Position Update
        → (loop until stopping criterion)
```

1. **Particle Initialization.** LLM synthesises a diverse initial population
   from the task description, using a *temperature-controlled sampling*
   strategy: low-temp particles stay near established patterns, high-temp
   particles explore unconventional structures.

2. **Flaw Identification.** Each particle is executed against the
   objective and an LLM diagnoses the result, separating agent-level
   flaws (missing/redundant agents, ambiguous policies) from
   collaboration-structure flaws (disruptive ordering, redundant steps,
   missing context, misaligned outputs). Output: a structured flaw
   description `f_i^{(t+1)}`.

3. **Failure-Aware Velocity Update.** Three LLM-driven terms balance
   exploration vs. exploitation:
   - `c_f · r_f · F(v_i^{(t)})` — **failure-driven adjustment**:
     `LLM_fail(v_i^{(t)}, f_i^{(t)}, f_i^{(t+1)})` rewrites the previous
     velocity to avoid repeating the same failure mode.
   - `c_p · r_p · (p*_i − x_i^{(t)})` — **personal-best guidance**:
     `LLM_pers(x_i^{(t)}, p*_i, f_i^{(t+1)})` proposes corrections that
     bring the current position toward its own historical best.
   - `c_g · r_g · (g − x_i^{(t)})` — **global-best guidance**:
     `LLM_glob(x_i^{(t)}, g, f_i^{(t+1)})` proposes corrections drawn
     from the swarm's best-performing system.

4. **Position Update.** A final LLM call `LLM_pos(x_i^{(t)}, v_i^{(t+1)})`
   applies the composite velocity as concrete structural edits:
   agent-level (add/remove/modify roles & policies) and
   collaboration-level (reorder/add/remove coordination steps).

Stop when iteration budget exhausted; return the global best `g`.

## Experimental results

**Optimizer:** GPT-4o-mini-0718. **Executors:** GPT-3.5-turbo-0125, GPT-4o,
Claude-3.5-sonnet-0620, DeepSeek-V3, Gemini-1.5-Pro.
**Budget:** 5 particles × 10 iterations (ADAS gets 30 iterations).

| Benchmark | Domain | Best baseline (GPT-4o) | SwarmAgentic (GPT-4o) |
|---|---|---|---|
| TravelPlanner | Constrained planning | 8.9 (ADAS) | **32.2** (+261.8% over ADAS) |
| NP — Trip Planning | Open-ended | 9.0 (CoT) | **13.1** |
| NP — Meeting Planning | Open-ended | 50.0 (CoT) | **56.0** |
| NP — Calendar Scheduling | Open-ended | 66.0 (ADAS) | **82.0** |
| Creative Writing | Multi-paragraph generation | 7.6 (SPP) | **8.5** |
| MGSM | Math reasoning | 87.3 (Direct) | **88.4** |

**Cross-model transfer:** systems discovered with GPT-4o-mini transfer
well to Claude-3.5-sonnet, DeepSeek-V3, Gemini-1.5; re-optimising
directly on the target executor adds a further bump.

**Ablation (Creative Writing, +Δ vs. Direct baseline 6.2):**
- Remove Failure-Driven Adjustments → 6.7 (+8.1%) *— biggest loss*
- Remove Agent-Level Adaptation → 7.3 (+17.7%)
- Remove Collaborative-Structure Reconfiguration → 8.4 (+35.5%)
- Full SwarmAgentic → 8.8 (+41.9%)

Iterations and particle count both monotonically help (5 particles ×
10 iters yields +12.9% over 5 × 3; 5 × 5 yields +11.3% over 1 × 5).

## Limitations (per §"Limitations")

- **No inductive priors.** Designed for open-ended, structurally
  unconstrained tasks — domain-specific templates could speed convergence
  in structured environments but aren't currently supported.
- **LLM-inherited factual hallucination.** Errors in flaw-identification
  or velocity rewrites can propagate through optimisation cycles. No
  external knowledge-source integration.
- **Text-only.** No perception, no action grounding, no multimodal I/O —
  not suitable for embodied or sensor-rich scenarios as-is.

## Relevance to koalisi

SwarmAgentic and koalisi sit at adjacent levels of the agent-systems
stack and are plausibly complementary:

| Dimension | koalisi | SwarmAgentic |
|---|---|---|
| **What gets optimised** | Coalition formation *within* a fixed runtime (form / join / leave / merge across known agents) | The agent system *itself* (which agents exist, how they collaborate) |
| **Optimisation primitive** | `ValueCalculator` (additive / synergistic / multiplicative / weighted) + AIPA partition search + DCVC | Three-term LLM-driven velocity (failure / personal-best / global-best) operating on language-encoded system definitions |
| **Search space** | Integer partitions of `n` agents into coalitions (combinatorial) | Open-ended structured language (non-numeric, non-differentiable) |
| **Feedback signal** | Numeric value/cost from `ValueCalculator` | LLM-diagnosed flaws + numeric objective |
| **Time discipline** | Event-sourced; `TemporalHypergraph` records every mutation | Generation-based; no temporal layer |

### Concrete integration ideas worth flagging for Phase 6+

1. **SwarmAgentic as a koalisi *configurator*.** Use SwarmAgentic offline
   to discover an initial coalition topology (which agents, what
   responsibilities, what coordination edges) from a task description,
   then hand the result to `CoalitionManager` for runtime execution +
   temporal tracking. The agent set `A_i^{(t)}` maps directly to
   koalisi vertices; the collaboration structure `W_i^{(t)}` maps to
   hyperedges.

2. **Failure-aware velocity ↔ Active Inference EFE.** The planned Phase 6
   decision layer (port of `coalition_aif`'s EFE calculator) is one
   formal mechanism for self-optimization; SwarmAgentic's flaw-driven
   velocity update is a less-formal, LLM-driven alternative. They could
   coexist — EFE for fast, gradient-flavoured within-coalition decisions;
   SwarmAgentic-style rewrites for slower, structural between-iteration
   topology changes.

3. **`ValueCalculator` extension.** SwarmAgentic's three coefficients
   (`c_f`, `c_p`, `c_g`) are direct analogues of `WeightedCalculator`'s
   `size_weight`/`capability_weight`/`trust_weight`/`synergy_weight`.
   Adding a "history" weight derived from `agent_coalition_history` and
   a "failure" weight derived from past coalition outcomes would close
   the loop with their feedback-driven design.

4. **Population-based search atop AIPA.** AIPA enumerates integer
   partitions; SwarmAgentic maintains a *population* of full system
   designs. A natural hybrid: AIPA generates candidate partitions, a
   SwarmAgentic-style swarm evolves the *agent assignments + collaboration
   policies* per partition, and `TemporalHypergraph` records the trajectory
   so good lineages can be replayed.

5. **Cross-model transferability as a koalisi value-prop.** SwarmAgentic
   shows discovered systems transfer across LLMs. If koalisi's runtime
   layer (kameo actors, PubSub, lifecycle) is provider-agnostic, a
   SwarmAgentic-discovered coalition spec could be re-instantiated under
   different LLM backends without re-running the search.

### Where the analogy breaks

- **No temporal layer in SwarmAgentic.** Every iteration is a fresh
  population evaluation; there's no event log, no time-travel, no
  snapshot replay. koalisi's `TemporalHypergraph` is genuinely novel
  relative to this line of work.
- **No runtime lifecycle.** SwarmAgentic optimises *designs*; it doesn't
  address graceful shutdown, supervision trees, or remote messaging.
  koalisi's `CoalitionRuntime` + libp2p gateway is in a different layer.
- **Optimiser ≠ executor.** SwarmAgentic uses one LLM to optimise and
  another to execute. koalisi has no built-in LLM dependency at all —
  the value calculators are deterministic Rust. Bringing in
  SwarmAgentic-style optimisation would introduce an LLM dependency
  scoped to Phase 6 (decision layer).

## Citation

> Zhang, Y., Lin, C., Tang, S., Chen, H., Zhou, S., Ma, Y., Tresp, V.
> (2025). *SwarmAgentic: Towards Fully Automated Agentic System
> Generation via Swarm Intelligence.* arXiv:2506.15672v1.
