//! Permission policy (pure). Populated in the permit layer: `Rule`,
//! `Policy`, `StaticDecision`, `static_decision`, `Risk`, `classify`,
//! `remember`.
//!
//! Task 14 note: the engine's `run_turn` threads a `&mut Policy` through the
//! turn loop ahead of the permit gate itself landing (Task 24 adds
//! `static_decision`'s real logic; Tasks 25-26 add `remember`/`classify`).
//! The two data types below are forward-declared here, field-for-field
//! identical to the Task 24 plan, so that task's implementation slots in
//! without changing either struct's shape.

use std::path::PathBuf;

/// One session-policy grant, e.g. `{ tool: "bash", scope: "always" }`.
#[derive(Debug, Clone)]
pub struct Rule {
    pub tool: String,
    pub scope: String,
}

/// The active conversation's permission policy. Task 14 only carries this
/// through the loop; `static_decision`/`remember`/`classify` (Tasks 24-26)
/// add the actual gating logic.
#[derive(Debug, Clone)]
pub struct Policy {
    pub project_root: PathBuf,
    pub always: Vec<Rule>,
    pub classifier: bool,
}
