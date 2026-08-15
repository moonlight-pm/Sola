//! Call-plane helper — advertise methods and deliver invokes into iced.
//!
//! Parallel to [`crate::app::BusSetup`]. Kit apps register once in `main`,
//! then fold [`call_subscription`] into their iced `subscription`.
//!
//! ```ignore
//! sola_kit::CallSetup::new("ws", App::APP_ID)
//!     .methods(my_methods())
//!     .install();
//!
//! fn subscription(&self) -> Subscription<Msg> {
//!     sola_kit::call_subscription().map(Msg::Call)
//! }
//! ```
//!
//! Or hang it off [`crate::app::BusSetup::calls`] so one `install()` does both.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use iced::futures::Stream;
use iced::Subscription;
use sola_call::{Incoming, MethodSpec};

static CALL_RX: Mutex<Option<mpsc::Receiver<Incoming>>> = Mutex::new(None);
static CALL_STREAM_TX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedSender<Incoming>>,
> = Mutex::new(None);
static CALL_POLLER_STARTED: AtomicBool = AtomicBool::new(false);

/// Builder: owner (CLI noun) + app_id + advertised methods.
pub struct CallSetup {
    owner: String,
    app_id: String,
    methods: Vec<MethodSpec>,
}

impl CallSetup {
    pub fn new(owner: impl Into<String>, app_id: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            app_id: app_id.into(),
            methods: Vec::new(),
        }
    }

    pub fn method(mut self, spec: MethodSpec) -> Self {
        self.methods.push(spec);
        self
    }

    pub fn methods(mut self, specs: impl IntoIterator<Item = MethodSpec>) -> Self {
        self.methods.extend(specs);
        self
    }

    /// Start the reconnecting provider. Safe to call once per process.
    pub fn install(self) {
        install_provider(self.owner, self.app_id, self.methods);
    }
}

pub fn install_provider(owner: String, app_id: String, methods: Vec<MethodSpec>) {
    let rx = sola_call::start_provider(&owner, &app_id, methods);
    match CALL_RX.lock() {
        Ok(mut slot) => *slot = Some(rx),
        Err(poisoned) => {
            *poisoned.into_inner() = Some(rx);
        }
    }
    tracing::info!(%owner, %app_id, "call provider installed");
    ensure_call_poller();
}

/// Iced subscription of incoming invokes. Use **or** a manual drain of
/// the provider receiver — not both.
pub fn call_subscription() -> Subscription<Incoming> {
    Subscription::run(call_stream)
}

fn call_stream() -> impl Stream<Item = Incoming> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Incoming>();
    match CALL_STREAM_TX.lock() {
        Ok(mut slot) => *slot = Some(tx),
        Err(poisoned) => {
            *poisoned.into_inner() = Some(tx);
        }
    }
    ensure_call_poller();
    rx
}

fn ensure_call_poller() {
    if CALL_POLLER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::Builder::new()
        .name("sola-kit-call".into())
        .spawn(|| loop {
            let next = match CALL_RX.lock() {
                Ok(guard) => guard.as_ref().and_then(|rx| rx.try_recv().ok()),
                Err(poisoned) => {
                    let guard = poisoned.into_inner();
                    let out = guard.as_ref().and_then(|rx| rx.try_recv().ok());
                    CALL_RX.clear_poison();
                    out
                }
            };
            match next {
                Some(inc) => {
                    let mut slot = CALL_STREAM_TX.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(tx) = slot.as_ref() {
                        if tx.unbounded_send(inc).is_err() {
                            *slot = None;
                        }
                    }
                    // No iced subscription yet: drop. The caller is waiting;
                    // they will get a host timeout rather than a silent hang
                    // after iced starts. Apps must wire `call_subscription`.
                }
                None => std::thread::sleep(Duration::from_millis(8)),
            }
        })
        .ok();
}
