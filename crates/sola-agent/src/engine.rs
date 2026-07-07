//! Agent worker thread + the turn loop.
//!
//! `start` spawns one dedicated std thread (mirroring `pty.rs`) that owns
//! the command receiver and drives turns. The loop's core, `run_turn`,
//! takes its event sink as `&mut dyn FnMut(AgentEvent)` so it is unit-
//! testable with a local collector; `start` wires that sink to the global
//! `crate::event::emit`.
//!
//! Text-only (no tools yet): tools land in Tasks 18-23 (the individual tool
//! impls + `tools::tool_schemas`/`dispatch`) and Task 27 (the tool-executing
//! loop). Until then this layer passes an empty tools slice (`&[]`) to
//! `stream_turn` and ignores `TurnOutcome::calls` — a real provider will
//! simply never be asked to call anything it wasn't advertised.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::event::{AgentCmd, AgentEvent};
use crate::permit::Policy;
use crate::provider::{LlmStream, StreamEvent};
use crate::session::{Content, Role, Session};

/// Static configuration for one worker. `model`/`effort` are mutable at
/// runtime via `AgentCmd::SetModel`; the rest are fixed for the process.
pub struct EngineConfig {
    pub api_key: String,
    pub model: String,
    pub effort: String,
    pub project_root: std::path::PathBuf,
    pub classifier: bool,
}

/// Spawn the worker thread. It takes the process-wide command receiver
/// exactly once, then loops: on `Send` it appends the user node (and
/// forks first if `branch_from` is set), resets the abort flag, and runs
/// a turn; `Abort` trips the flag; `SetModel` swaps model/effort.
pub fn start(
    config: EngineConfig,
    provider: Arc<dyn LlmStream + Send + Sync>,
    session: Arc<Mutex<Session>>,
) {
    std::thread::spawn(move || {
        let mut config = config;
        let cmd_rx = crate::event::take_cmd_rx();
        let abort = AtomicBool::new(false);
        let mut policy = Policy {
            project_root: config.project_root.clone(),
            always: Vec::new(),
            classifier: config.classifier,
        };
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                AgentCmd::Send { text, branch_from } => {
                    {
                        let mut s = session.lock().unwrap();
                        if let Some(parent) = branch_from {
                            s.branch_from(parent);
                        }
                        s.append(Role::User, Content::Text(text), None, None);
                    }
                    abort.store(false, Ordering::SeqCst);
                    run_turn(
                        &config,
                        provider.as_ref(),
                        &session,
                        &mut policy,
                        &cmd_rx,
                        &abort,
                        &mut |ev| crate::event::emit(ev),
                    );
                }
                AgentCmd::Abort => abort.store(true, Ordering::SeqCst),
                AgentCmd::SetModel { model, effort } => {
                    config.model = model;
                    config.effort = effort;
                }
                // No turn is awaiting a decision at loop scope — ignore
                // stray approvals/denials.
                AgentCmd::Approve { .. } | AgentCmd::Deny { .. } => {}
            }
        }
    });
}

/// Drive one turn. This layer handles text-only turns: stream, forward
/// display events, append the assistant node, emit `TurnEnd`. The tool-
/// executing loop is added in the next task (same signature).
///
/// `_policy`/`_cmd_rx` are threaded through now so the signature matches
/// Task 27's tool-executing widening exactly; this layer doesn't consult
/// them yet (no tool calls are ever requested since `tools` is empty).
fn run_turn(
    config: &EngineConfig,
    provider: &(dyn LlmStream + Send + Sync),
    session: &Arc<Mutex<Session>>,
    _policy: &mut Policy,
    _cmd_rx: &Receiver<AgentCmd>,
    abort: &AtomicBool,
    emit: &mut dyn FnMut(AgentEvent),
) {
    if abort.load(Ordering::SeqCst) {
        return;
    }
    let input = { session.lock().unwrap().to_input() };
    let stream_id = uuid::Uuid::new_v4().to_string();
    let outcome = {
        let mut sink = |ev: StreamEvent| match ev {
            StreamEvent::TextDelta(t) => {
                emit(AgentEvent::Delta { node_id: stream_id.clone(), text: t })
            }
            StreamEvent::Reasoning(t) => emit(AgentEvent::Reasoning { text: t }),
            StreamEvent::Error(m) => emit(AgentEvent::Error { message: m }),
            _ => {}
        };
        // Tools are not wired yet (see module doc) — empty slice.
        provider.stream_turn(
            &config.model,
            &config.effort,
            &input,
            &[],
            &mut sink,
        )
    };
    match outcome {
        Ok(o) => {
            if !o.assistant_text.is_empty() {
                session.lock().unwrap().append(
                    Role::Assistant,
                    Content::Text(o.assistant_text.clone()),
                    Some(config.model.clone()),
                    Some(o.usage),
                );
            }
            emit(AgentEvent::TurnEnd { usage: o.usage });
        }
        Err(e) => emit(AgentEvent::Error { message: e }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AgentCmd, AgentEvent};
    use crate::permit::Policy;
    use crate::provider::{
        FunctionCall, InputItem, LlmStream, StreamEvent, TurnOutcome,
    };
    use crate::session::{Content, Role, Session, Usage};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// Redirect `$XDG_CONFIG_HOME` to a fresh temp dir so `Session` JSONL
    /// persistence never touches the real `~/.config`, and return an
    /// (also-temp) project root for the tools' `ToolCtx`.
    fn hermetic_root(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir()
            .join(format!("sola-agent-{tag}-{}", uuid::Uuid::new_v4()));
        let cfg = base.join("config");
        let root = base.join("project");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY (edition 2024): test setup only; the value is always an
        // absolute temp path, so a successful persist can never land in
        // the real $HOME. Concurrent tests each point it at their own
        // temp dir — a benign race (assertions read the in-memory tree).
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &cfg) };
        root
    }

    /// Streams two text deltas + a completed event, then returns the
    /// aggregated turn with no tool calls.
    struct TextFake;
    impl LlmStream for TextFake {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            _input: &[InputItem],
            _tools: &[serde_json::Value],
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutcome, String> {
            sink(StreamEvent::TextDelta("he".into()));
            sink(StreamEvent::TextDelta("llo".into()));
            sink(StreamEvent::Completed {
                usage: Usage { input_tokens: 3, output_tokens: 4 },
            });
            Ok(TurnOutcome {
                assistant_text: "hello".into(),
                calls: Vec::new(),
                usage: Usage { input_tokens: 3, output_tokens: 4 },
            })
        }
    }

    #[test]
    fn text_only_turn_streams_and_appends_assistant() {
        let root = hermetic_root("text");
        let session = Arc::new(Mutex::new(Session::new(root.clone())));
        // Simulate the Send handler having already appended the user node.
        session
            .lock()
            .unwrap()
            .append(Role::User, Content::Text("hi".into()), None, None);

        let config = EngineConfig {
            api_key: String::new(),
            model: "fugu".into(),
            effort: "high".into(),
            project_root: root.clone(),
            classifier: false,
        };
        let fake = TextFake;
        let mut policy = Policy {
            project_root: root.clone(),
            always: Vec::new(),
            classifier: false,
        };
        let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AgentCmd>();
        let abort = AtomicBool::new(false);

        let mut events: Vec<AgentEvent> = Vec::new();
        {
            let mut emit = |ev| events.push(ev);
            run_turn(
                &config, &fake, &session, &mut policy, &cmd_rx, &abort,
                &mut emit,
            );
        }

        assert_eq!(
            events.len(),
            3,
            "expected two deltas and a turn-end, got {events:?}"
        );
        assert!(matches!(
            &events[0],
            AgentEvent::Delta { text, .. } if text.as_str() == "he"
        ));
        assert!(matches!(
            &events[1],
            AgentEvent::Delta { text, .. } if text.as_str() == "llo"
        ));
        assert!(matches!(
            &events[2],
            AgentEvent::TurnEnd { usage }
                if usage.input_tokens == 3 && usage.output_tokens == 4
        ));

        let s = session.lock().unwrap();
        let path = s.path_to_leaf();
        let last = path.last().expect("session has at least one node");
        assert!(
            matches!(last.role, Role::Assistant),
            "leaf should be the assistant node"
        );
        assert!(matches!(
            &last.content,
            Content::Text(t) if t.as_str() == "hello"
        ));
    }
}
