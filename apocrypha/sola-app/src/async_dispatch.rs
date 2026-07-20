use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, mpsc as std_mpsc};
use std::time::Duration;

use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;

/// Trait implemented by async command handlers used with `AsyncDispatcher`.
/// Runs on a dedicated tokio runtime thread.
#[async_trait::async_trait]
pub trait AppHandler: Send + Sync + 'static {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value;
}

struct AsyncCmd {
    id: u64,
    cmd: String,
    args: Value,
}

type ReplyCallback = Box<dyn FnOnce(Value)>;

/// Bridges async commands from the GTK main thread to a tokio runtime
/// thread and back. Apps construct one in `new()`, stash it on the app
/// struct, and forward from `on_js_command`.
pub struct AsyncDispatcher {
    cmd_tx: tokio_mpsc::UnboundedSender<AsyncCmd>,
    pending: Rc<RefCell<HashMap<u64, ReplyCallback>>>,
    next_id: Rc<Cell<u64>>,
}

impl AsyncDispatcher {
    pub fn spawn<H: AppHandler>(handler: H) -> Self {
        let (cmd_tx, mut cmd_rx) = tokio_mpsc::unbounded_channel::<AsyncCmd>();
        let (reply_tx, reply_rx) = std_mpsc::channel::<(u64, Value)>();

        std::thread::spawn(move || {
            let rt = Runtime::new().expect("failed to create tokio runtime for AsyncDispatcher");
            rt.block_on(async move {
                let handler = Arc::new(handler);
                while let Some(AsyncCmd { id, cmd, args }) = cmd_rx.recv().await {
                    let handler = handler.clone();
                    let reply_tx = reply_tx.clone();
                    tokio::spawn(async move {
                        let result = handler.dispatch(&cmd, &args).await;
                        let _ = reply_tx.send((id, result));
                    });
                }
            });
        });

        let pending: Rc<RefCell<HashMap<u64, ReplyCallback>>> =
            Rc::new(RefCell::new(HashMap::new()));

        // Bridge tokio replies back to the main loop. 5ms poll matches the
        // existing bridge.rs pattern; acceptable for now.
        let pending_for_bridge = pending.clone();
        glib::timeout_add_local(Duration::from_millis(5), move || {
            while let Ok((id, result)) = reply_rx.try_recv() {
                if let Some(cb) = pending_for_bridge.borrow_mut().remove(&id) {
                    cb(result);
                }
            }
            glib::ControlFlow::Continue
        });

        Self {
            cmd_tx,
            pending,
            next_id: Rc::new(Cell::new(0)),
        }
    }

    /// Dispatch a command. `reply` runs on the main thread with the
    /// handler's return value.
    pub fn dispatch(&self, cmd: String, args: Value, reply: impl FnOnce(Value) + 'static) {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.pending.borrow_mut().insert(id, Box::new(reply));
        if self.cmd_tx.send(AsyncCmd { id, cmd, args }).is_err() {
            self.pending.borrow_mut().remove(&id);
            tracing::error!("AsyncDispatcher runtime thread is dead");
        }
    }
}
