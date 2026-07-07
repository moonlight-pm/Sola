//! Agent worker thread + the turn loop.
//!
//! `start` spawns one dedicated std thread (mirroring `pty.rs`) that owns
//! the command receiver and drives turns. The loop's core, `run_turn`,
//! takes its event sink as `&mut dyn FnMut(AgentEvent)` so it is unit-
//! testable with a local collector; `start` wires that sink to the global
//! `crate::event::emit`.
//!
//! Tool-executing loop: `run_turn` advertises `tools::tool_schemas()`, streams
//! a turn, then for every requested call runs the permit gate (static →
//! optional classifier → user prompt), `dispatch`es it, appends the model's
//! `function_call` node + the tool's `function_call_output` node, and loops
//! `stream_turn` until a turn completes with no calls. A prompted call blocks
//! in `wait_for_decision`, which pulls `Approve`/`Deny`/`Abort` off the same
//! command receiver the worker owns and matches on `call_id`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::event::{AgentCmd, AgentEvent};
use crate::permit::{classify, remember, static_decision, Policy, Risk, StaticDecision};
use crate::provider::{LlmStream, StreamEvent};
use crate::session::{Content, Role, Session};
use crate::tools::{dispatch, tool_schemas, ToolCtx, ToolDetail, ToolResult};

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

/// Drive one agent turn to completion: stream, forward display events, then for
/// every requested tool call run the permit gate → dispatch → feed a
/// `function_call_output` back, looping `stream_turn` until a turn completes
/// with no calls. `abort` is checked between steps.
fn run_turn(
    config: &EngineConfig,
    provider: &(dyn LlmStream + Send + Sync),
    session: &Arc<Mutex<Session>>,
    policy: &mut Policy,
    cmd_rx: &Receiver<AgentCmd>,
    abort: &AtomicBool,
    emit: &mut dyn FnMut(AgentEvent),
) {
    let tools = tool_schemas();
    loop {
        if abort.load(Ordering::SeqCst) {
            return;
        }
        let input = { session.lock().unwrap().to_input() };
        let stream_id = uuid::Uuid::new_v4().to_string();
        let outcome = {
            let mut sink = |ev: StreamEvent| match ev {
                StreamEvent::TextDelta(t) => emit(AgentEvent::Delta {
                    node_id: stream_id.clone(),
                    text: t,
                }),
                StreamEvent::Reasoning(t) => emit(AgentEvent::Reasoning { text: t }),
                StreamEvent::Error(m) => emit(AgentEvent::Error { message: m }),
                _ => {}
            };
            provider.stream_turn(&config.model, &config.effort, &input, &tools, &mut sink)
        };
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                emit(AgentEvent::Error { message: e });
                return;
            }
        };

        // Attribute this step's usage to the first assistant-authored node we
        // append (prose if any, else the first call node).
        let mut usage_slot = Some(outcome.usage);
        if !outcome.assistant_text.is_empty() {
            let u = usage_slot.take();
            session.lock().unwrap().append(
                Role::Assistant,
                Content::Text(outcome.assistant_text.clone()),
                Some(config.model.clone()),
                u,
            );
        }

        // No calls → the whole agent turn is done.
        if outcome.calls.is_empty() {
            emit(AgentEvent::TurnEnd { usage: outcome.usage });
            return;
        }

        for call in &outcome.calls {
            if abort.load(Ordering::SeqCst) {
                return;
            }
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
            emit(AgentEvent::ToolStart {
                call_id: call.call_id.clone(),
                tool: call.name.clone(),
                args: args.clone(),
            });
            // Record the model's function_call node regardless of gate outcome.
            {
                let u = usage_slot.take();
                session.lock().unwrap().append(
                    Role::Assistant,
                    Content::FunctionCall {
                        call_id: call.call_id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    },
                    Some(config.model.clone()),
                    u,
                );
            }

            // Permit gate: static → optional classifier → user prompt.
            let allowed = match static_decision(policy, &call.name, &args) {
                StaticDecision::AutoAllow => true,
                StaticDecision::NeedsPrompt { preview } => {
                    let cleared = config.classifier
                        && matches!(classify(provider, &call.name, &args), Risk::Safe);
                    if cleared {
                        true
                    } else {
                        emit(AgentEvent::ApprovalRequest {
                            call_id: call.call_id.clone(),
                            tool: call.name.clone(),
                            preview,
                        });
                        wait_for_decision(cmd_rx, &call.call_id, policy, &call.name, abort)
                    }
                }
            };

            let result = if allowed {
                let ctx = ToolCtx { project_root: config.project_root.clone() };
                dispatch(&call.name, &args, &ctx)
            } else {
                let msg = format!("Tool call `{}` was declined by the user.", call.name);
                ToolResult {
                    model_text: msg.clone(),
                    ui_detail: ToolDetail::Text(msg),
                }
            };

            // Feed the output back into the transcript, then surface it.
            session.lock().unwrap().append(
                Role::Tool,
                Content::FunctionCallOutput {
                    call_id: call.call_id.clone(),
                    output: result.model_text.clone(),
                },
                None,
                None,
            );
            emit(AgentEvent::ToolEnd {
                call_id: call.call_id.clone(),
                result,
            });
        }
        // Loop: the appended outputs are now part of `to_input()`.
    }
}

/// Block on the command receiver until a decision arrives for `call_id`.
/// Approve → true (and persist an always-allow rule on `remember`); Deny →
/// false; Abort → trip the flag and treat as deny; unrelated commands are
/// skipped; a closed channel is treated as a deny.
fn wait_for_decision(
    cmd_rx: &Receiver<AgentCmd>,
    call_id: &str,
    policy: &mut Policy,
    tool: &str,
    abort: &AtomicBool,
) -> bool {
    loop {
        match cmd_rx.recv() {
            Ok(AgentCmd::Approve { call_id: id, remember: r }) if id == call_id => {
                if r {
                    remember(policy, tool);
                }
                return true;
            }
            Ok(AgentCmd::Deny { call_id: id, .. }) if id == call_id => return false,
            Ok(AgentCmd::Abort) => {
                abort.store(true, Ordering::SeqCst);
                return false;
            }
            Ok(_) => continue,
            Err(_) => return false,
        }
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

    /// Two-call fake: first turn asks to `read`, second turn (after the output
    /// is fed back) returns final text. Records every `input` it was given.
    struct ToolFake {
        step: Mutex<usize>,
        inputs: Mutex<Vec<Vec<InputItem>>>,
    }
    impl ToolFake {
        fn new() -> Self {
            Self { step: Mutex::new(0), inputs: Mutex::new(Vec::new()) }
        }
    }
    impl LlmStream for ToolFake {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            input: &[InputItem],
            _tools: &[serde_json::Value],
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutcome, String> {
            self.inputs.lock().unwrap().push(input.to_vec());
            let idx = {
                let mut step = self.step.lock().unwrap();
                let i = *step;
                *step += 1;
                i
            };
            if idx == 0 {
                let args = "{\"path\":\"note.txt\"}".to_string();
                sink(StreamEvent::FunctionCallStarted { call_id: "c1".into(), name: "read".into() });
                sink(StreamEvent::FunctionCallDone {
                    call_id: "c1".into(),
                    name: "read".into(),
                    arguments: args.clone(),
                });
                Ok(TurnOutcome {
                    assistant_text: String::new(),
                    calls: vec![FunctionCall {
                        call_id: "c1".into(),
                        name: "read".into(),
                        arguments: args,
                    }],
                    usage: Usage { input_tokens: 1, output_tokens: 1 },
                })
            } else {
                sink(StreamEvent::TextDelta("done".into()));
                Ok(TurnOutcome {
                    assistant_text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage { input_tokens: 2, output_tokens: 2 },
                })
            }
        }
    }

    #[test]
    fn tool_call_executes_and_feeds_output_back() {
        let root = hermetic_root("tool");
        std::fs::write(root.join("note.txt"), "hello file").unwrap();

        let session = Arc::new(Mutex::new(Session::new(root.clone())));
        session.lock().unwrap().append(
            Role::User,
            Content::Text("read note.txt".into()),
            None,
            None,
        );

        let config = EngineConfig {
            api_key: String::new(),
            model: "fugu".into(),
            effort: "high".into(),
            project_root: root.clone(),
            classifier: false,
        };
        let mut policy = Policy {
            project_root: root.clone(),
            always: Vec::new(),
            classifier: false,
        };
        let fake = ToolFake::new();
        let abort = AtomicBool::new(false);
        let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AgentCmd>();

        let mut events: Vec<AgentEvent> = Vec::new();
        {
            let mut emit = |ev| events.push(ev);
            run_turn(&config, &fake, &session, &mut policy, &cmd_rx, &abort, &mut emit);
        }

        let starts = events.iter().filter(|e| matches!(e, AgentEvent::ToolStart { .. })).count();
        let ends = events.iter().filter(|e| matches!(e, AgentEvent::ToolEnd { .. })).count();
        assert_eq!(starts, 1, "read should start exactly once: {events:?}");
        assert_eq!(ends, 1, "read should end exactly once: {events:?}");

        let ran = events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolEnd { result, .. } if result.model_text.contains("hello file")
        ));
        assert!(ran, "read must have executed and returned file contents: {events:?}");

        let inputs = fake.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 2, "engine must loop back for a second turn");
        let fed_back = inputs[1].iter().any(|it| matches!(
            it,
            InputItem::FunctionCallOutput { call_id, .. } if call_id.as_str() == "c1"
        ));
        assert!(fed_back, "second turn input must include c1's function_call_output");

        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::TurnEnd { .. })),
            "the final call-free turn should emit TurnEnd"
        );
    }

    /// A `bash` call always needs a prompt; a queued `Deny` must skip dispatch
    /// and feed a "declined" output back so the model can adapt.
    struct BashFake {
        step: Mutex<usize>,
        inputs: Mutex<Vec<Vec<InputItem>>>,
    }
    impl BashFake {
        fn new() -> Self {
            Self { step: Mutex::new(0), inputs: Mutex::new(Vec::new()) }
        }
    }
    impl LlmStream for BashFake {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            input: &[InputItem],
            _tools: &[serde_json::Value],
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutcome, String> {
            self.inputs.lock().unwrap().push(input.to_vec());
            let idx = {
                let mut step = self.step.lock().unwrap();
                let i = *step;
                *step += 1;
                i
            };
            if idx == 0 {
                let args = "{\"command\":\"echo SENTINEL_RAN\"}".to_string();
                sink(StreamEvent::FunctionCallDone {
                    call_id: "b1".into(),
                    name: "bash".into(),
                    arguments: args.clone(),
                });
                Ok(TurnOutcome {
                    assistant_text: String::new(),
                    calls: vec![FunctionCall {
                        call_id: "b1".into(),
                        name: "bash".into(),
                        arguments: args,
                    }],
                    usage: Usage { input_tokens: 1, output_tokens: 1 },
                })
            } else {
                sink(StreamEvent::TextDelta("ok".into()));
                Ok(TurnOutcome {
                    assistant_text: "ok".into(),
                    calls: Vec::new(),
                    usage: Usage { input_tokens: 2, output_tokens: 2 },
                })
            }
        }
    }

    #[test]
    fn denied_call_feeds_declined_output_and_skips_dispatch() {
        let root = hermetic_root("deny");
        let session = Arc::new(Mutex::new(Session::new(root.clone())));
        session.lock().unwrap().append(
            Role::User,
            Content::Text("run echo".into()),
            None,
            None,
        );

        let config = EngineConfig {
            api_key: String::new(),
            model: "fugu".into(),
            effort: "high".into(),
            project_root: root.clone(),
            classifier: false,
        };
        let mut policy = Policy {
            project_root: root.clone(),
            always: Vec::new(),
            classifier: false,
        };
        let fake = BashFake::new();
        let abort = AtomicBool::new(false);
        // Pre-queue the user's decision on the same channel the worker reads;
        // `wait_for_decision`'s blocking `recv` picks it up. Keep `cmd_tx` alive.
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AgentCmd>();
        cmd_tx
            .send(AgentCmd::Deny { call_id: "b1".into(), reason: None })
            .unwrap();

        let mut events: Vec<AgentEvent> = Vec::new();
        {
            let mut emit = |ev| events.push(ev);
            run_turn(&config, &fake, &session, &mut policy, &cmd_rx, &abort, &mut emit);
        }

        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::ApprovalRequest { call_id, .. } if call_id == "b1")),
            "bash should have prompted for approval: {events:?}"
        );

        let declined = events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolEnd { result, .. } if result.model_text.contains("declined")
        ));
        assert!(declined, "denied call must surface a declined result: {events:?}");

        let ran = events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolEnd { result, .. } if result.model_text.contains("SENTINEL_RAN")
        ));
        assert!(!ran, "bash must NOT have executed after a deny: {events:?}");

        // The declined output is fed back so the model can adapt on turn 2.
        let inputs = fake.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 2, "engine loops back after a declined call");
        let fed_declined = inputs[1].iter().any(|it| matches!(
            it,
            InputItem::FunctionCallOutput { call_id, output } if call_id == "b1" && output.contains("declined")
        ));
        assert!(fed_declined, "second turn must carry the declined function_call_output");
    }
}
