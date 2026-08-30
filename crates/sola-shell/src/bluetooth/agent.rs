//! BlueZ Agent1 — PIN / passkey / confirm, served on the shell's system-bus
//! connection. Prompts are forwarded to the menubar popover; the D-Bus
//! method waits on a oneshot until the UI replies.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use zbus::zvariant::ObjectPath;

use super::{AgentKind, AgentPrompt, AgentReply, Event};

pub const AGENT_PATH: &str = "/org/sola/shell/agent";

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum AgentError {
    Rejected(String),
    Canceled(String),
    #[zbus(error)]
    ZBus(zbus::Error),
}

pub struct AgentInner {
    pub event_tx: iced::futures::channel::mpsc::UnboundedSender<Event>,
    pending: tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<AgentReply>>>,
    next_id: AtomicU64,
    /// path → display name, filled from the latest snapshot so prompts
    /// can name the device without another D-Bus round-trip.
    pub names: std::sync::Mutex<HashMap<String, String>>,
}

impl AgentInner {
    pub fn new(event_tx: iced::futures::channel::mpsc::UnboundedSender<Event>) -> Arc<Self> {
        Arc::new(Self {
            event_tx,
            pending: tokio::sync::Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            names: std::sync::Mutex::new(HashMap::new()),
        })
    }

    pub async fn complete(&self, id: u64, reply: AgentReply) {
        if let Some(tx) = self.pending.lock().await.remove(&id) {
            let _ = tx.send(reply);
        }
    }

    pub async fn cancel_all(&self) {
        let mut g = self.pending.lock().await;
        for (_, tx) in g.drain() {
            let _ = tx.send(AgentReply::Reject);
        }
        let _ = self.event_tx.unbounded_send(Event::AgentCleared);
    }

    fn name_for(&self, path: &str) -> String {
        self.names
            .lock()
            .ok()
            .and_then(|g| g.get(path).cloned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).replace('_', ":"))
    }

    async fn ask(&self, device_path: String, kind: AgentKind) -> Result<AgentReply, AgentError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let prompt = AgentPrompt {
            id,
            device_name: self.name_for(&device_path),
            device_path,
            kind,
        };
        if self
            .event_tx
            .unbounded_send(Event::AgentPrompt(prompt))
            .is_err()
        {
            self.pending.lock().await.remove(&id);
            return Err(AgentError::Canceled("shell gone".into()));
        }
        match rx.await {
            Ok(reply) => {
                let _ = self.event_tx.unbounded_send(Event::AgentCleared);
                Ok(reply)
            }
            Err(_) => Err(AgentError::Canceled("dropped".into())),
        }
    }
}

#[derive(Clone)]
pub struct Agent {
    pub inner: Arc<AgentInner>,
}

impl Agent {
    fn path_str(device: &ObjectPath<'_>) -> String {
        device.as_str().to_string()
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(&self) -> Result<(), AgentError> {
        self.inner.cancel_all().await;
        Ok(())
    }

    async fn cancel(&self) -> Result<(), AgentError> {
        self.inner.cancel_all().await;
        Ok(())
    }

    async fn request_pin_code(&self, device: ObjectPath<'_>) -> Result<String, AgentError> {
        match self
            .inner
            .ask(Self::path_str(&device), AgentKind::RequestPin)
            .await?
        {
            AgentReply::Pin(s) if !s.is_empty() => Ok(s),
            AgentReply::Accept => Err(AgentError::Rejected("no pin".into())),
            _ => Err(AgentError::Rejected("rejected".into())),
        }
    }

    async fn request_passkey(&self, device: ObjectPath<'_>) -> Result<u32, AgentError> {
        match self
            .inner
            .ask(Self::path_str(&device), AgentKind::RequestPasskey)
            .await?
        {
            AgentReply::Passkey(n) => Ok(n),
            AgentReply::Accept => Err(AgentError::Rejected("no passkey".into())),
            _ => Err(AgentError::Rejected("rejected".into())),
        }
    }

    async fn request_confirmation(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
    ) -> Result<(), AgentError> {
        match self
            .inner
            .ask(Self::path_str(&device), AgentKind::ConfirmPasskey(passkey))
            .await?
        {
            AgentReply::Accept => Ok(()),
            _ => Err(AgentError::Rejected("rejected".into())),
        }
    }

    async fn request_authorization(&self, device: ObjectPath<'_>) -> Result<(), AgentError> {
        match self
            .inner
            .ask(Self::path_str(&device), AgentKind::Authorize)
            .await?
        {
            AgentReply::Accept => Ok(()),
            _ => Err(AgentError::Rejected("rejected".into())),
        }
    }

    async fn display_pin_code(
        &self,
        device: ObjectPath<'_>,
        pincode: String,
    ) -> Result<(), AgentError> {
        let _ = self
            .inner
            .event_tx
            .unbounded_send(Event::AgentPrompt(AgentPrompt {
                id: 0,
                device_path: Self::path_str(&device),
                device_name: self.inner.name_for(device.as_str()),
                kind: AgentKind::DisplayPin(pincode),
            }));
        Ok(())
    }

    async fn display_passkey(
        &self,
        device: ObjectPath<'_>,
        passkey: u32,
        entered: u16,
    ) -> Result<(), AgentError> {
        let _ = self
            .inner
            .event_tx
            .unbounded_send(Event::AgentPrompt(AgentPrompt {
                id: 0,
                device_path: Self::path_str(&device),
                device_name: self.inner.name_for(device.as_str()),
                kind: AgentKind::DisplayPasskey { passkey, entered },
            }));
        Ok(())
    }

    async fn authorize_service(
        &self,
        _device: ObjectPath<'_>,
        _uuid: String,
    ) -> Result<(), AgentError> {
        // User-initiated pair/connect from this panel: allow profiles.
        Ok(())
    }
}
