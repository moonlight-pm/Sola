//! The engine: one thread that holds the connections to Signal, writes what
//! arrives into the store, and sends what the window asks it to.
//!
//! The window reads the store itself and is told what changed. What is to
//! be sent is written to the store's outbox first, so it goes out when
//! there is a connection, across restarts.

mod files;
mod groups;
mod inbox;
mod jobs;
mod outgoing;
mod profile;
mod storage;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_core::ServiceId;
use libsignal_net::chat::server_requests::ServerEvent;
use libsignal_net::chat::{ChatConnection, ConnectError};
use libsignal_protocol::{
    ContentHint, DecryptionErrorMessage, PlaintextContent, ProtocolAddress, SenderCertificate, Timestamp,
};
use tokio::sync::mpsc;

use crate::account::{Account, Identity};
use crate::keys;
use crate::link::{self, Linked};
use crate::net::{self, Net};
use crate::proto::{self, Content, receipt_message, sync_message};
use crate::receive::{Opened, Receiver, Unreadable};
use crate::send::{self, Failure, Options, Sealed, Sender};
use crate::store::chats::{Id, Outgoing, Status};
use crate::store::{Store, now};
pub use inbox::duration;
use inbox::{Effects, Inbox};
use jobs::Job;

/// Seconds before connecting again, by failed attempt.
const BACKOFF: &[u64] = &[2, 5, 15, 30, 60, 120];

/// How often one thing is tried on connections that are up before it is
/// given up: a message then says it was not sent, and what waits behind it
/// goes.
const TRIES: usize = 4;

/// What the window asks of the engine.
#[derive(Debug)]
pub enum Command {
    /// Join an account as a linked device: the engine answers with the code
    /// to scan.
    Link { device_name: String },
    Send { conversation: Id, body: String, quote: Option<Id>, files: Vec<PathBuf> },
    /// The conversation is on screen: what is unread in it has been read.
    Read { conversation: Id },
    /// React to a message; `None` takes our reaction away.
    React { message: Id, emoji: Option<String> },
    Edit { message: Id, body: String },
    /// Take a message of ours back, for everyone.
    Retract { message: Id },
    /// Remove a message from this device only.
    Delete { message: Id },
    SetTimer { conversation: Id, seconds: u32 },
    /// Fetch an attachment's file.
    Fetch { attachment: Id },
    /// Send a message that failed again.
    Retry { message: Id },
    /// Connect now, not when the wait is over.
    Connect,
}

/// What the engine tells the window.
#[derive(Clone, Debug)]
pub enum Event {
    /// No account is linked: there is nothing to show but the way to link.
    Unlinked,
    /// The address to show as a code for the phone to scan.
    LinkCode(String),
    LinkFailed(String),
    Linked,
    Connecting,
    Online,
    Offline(String),
    /// A conversation's row or thread changed.
    Changed(Id),
    /// Names, or the list as a whole, changed.
    People,
    /// A message to tell the user of.
    Arrived { conversation: Id, title: String, text: String },
    Typing { conversation: Id, who: String, started: bool },
}

enum Input {
    Command(Command),
    Server(u64, ServerEvent),
    Up(u64, std::result::Result<Box<Wire>, ConnectError>),
    Reconnect(u64),
    /// The wait before sending again is over.
    Settle,
    LinkCode(String),
    Linked(Result<Box<Linked>>),
    /// A job's result is in the store.
    Done(Effects),
}

/// The engine's end for the window.
#[derive(Clone)]
pub struct Handle {
    inputs: mpsc::UnboundedSender<Input>,
}

impl Handle {
    pub fn send(&self, command: Command) {
        if self.inputs.send(Input::Command(command)).is_err() {
            tracing::warn!("the engine is gone; a command was dropped");
        }
    }
}

/// The two connections of a device that is online.
pub struct Wire {
    /// Signed in: what arrives, and what only we may ask.
    pub chat: ChatConnection,
    /// Not signed in: sealed sends and profiles, where the server is not
    /// told who asks.
    pub unauth: ChatConnection,
    /// Signal's word that we are who our sealed messages say. It runs out,
    /// so it is fetched again before it does.
    pub certificate: RefCell<Option<SenderCertificate>>,
}

/// What jobs share with the engine.
pub struct Shared {
    pub store: Store,
    pub wire: RefCell<Option<Rc<Wire>>>,
    pub account: RefCell<Option<Account>>,
    pub me: std::cell::Cell<Id>,
    /// Today's credential for the group server, and the day it is for.
    pub group_credential: RefCell<Option<(u64, Vec<u8>)>>,
}

/// Start the engine on its own thread. `emit` is called there with each event.
pub fn start(store: Store, emit: impl Fn(Event) + Send + 'static) -> Handle {
    let (inputs, receiver) = mpsc::unbounded_channel();
    let handle = Handle { inputs: inputs.clone() };
    std::thread::Builder::new()
        .name("sola-signal-engine".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("the engine's runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&runtime, async move {
                match Engine::new(store, inputs, Box::new(emit)) {
                    Ok(engine) => engine.run(receiver).await,
                    Err(e) => tracing::error!("the engine did not start: {e:#}"),
                }
            });
        })
        .expect("spawn the engine");
    handle
}

struct Engine {
    shared: Rc<Shared>,
    net: Rc<Net>,
    inputs: mpsc::UnboundedSender<Input>,
    emit: Box<dyn Fn(Event)>,
    receiver: Option<Receiver>,
    /// Counts connections, so that word from an old one is dropped.
    epoch: u64,
    failed: usize,
    jobs: mpsc::UnboundedSender<Job>,
    /// Delivery receipts owed, sent when the queue is quiet.
    owed: Vec<(Id, u64)>,
    linking: bool,
    /// What the socket said before the connections were both up.
    early: Vec<ServerEvent>,
    /// The outbox's oldest row, and how often it has failed on a
    /// connection that was up.
    stuck: Option<(Id, usize)>,
    /// Nothing is sent before this: the wait after a send was put off.
    hold: Option<tokio::time::Instant>,
}

impl Engine {
    fn new(store: Store, inputs: mpsc::UnboundedSender<Input>, emit: Box<dyn Fn(Event)>) -> Result<Self> {
        let shared = Rc::new(Shared {
            store,
            wire: RefCell::new(None),
            account: RefCell::new(None),
            me: std::cell::Cell::new(0),
            group_credential: RefCell::new(None),
        });
        let jobs = jobs::start(shared.clone(), inputs.clone());
        Ok(Self {
            shared,
            net: Rc::new(Net::new()),
            inputs,
            emit,
            receiver: None,
            epoch: 0,
            failed: 0,
            jobs,
            owed: Vec::new(),
            linking: false,
            early: Vec::new(),
            stuck: None,
            hold: None,
        })
    }

    fn store(&self) -> &Store {
        &self.shared.store
    }

    fn inbox(&self) -> Inbox {
        Inbox { store: self.store().clone(), me: self.shared.me.get() }
    }

    async fn run(mut self, mut inputs: mpsc::UnboundedReceiver<Input>) {
        if let Err(e) = self.boot() {
            tracing::error!("the engine did not start: {e:#}");
            return;
        }
        loop {
            let expiry = self.store().next_expiry().ok().flatten();
            let wait = expiry.map(|at| Duration::from_millis(at.saturating_sub(now()) + 50));
            let input = tokio::select! {
                input = inputs.recv() => input,
                _ = tokio::time::sleep(wait.unwrap_or(Duration::MAX)), if wait.is_some() => {
                    self.expire();
                    continue;
                }
            };
            let Some(input) = input else { break };
            if let Err(e) = self.handle(input).await {
                tracing::warn!("{e:#}");
            }
            // The queue is quiet: send what was put off while it was not.
            if inputs.is_empty() {
                self.settle().await;
            }
        }
    }

    /// Take up the stored account, if there is one.
    fn boot(&mut self) -> Result<()> {
        self.store().reset_fetching()?;
        match self.store().account()? {
            Some(account) => {
                self.take_account(account)?;
                self.connect();
            }
            None => (self.emit)(Event::Unlinked),
        }
        Ok(())
    }

    fn take_account(&mut self, account: Account) -> Result<()> {
        let me = self.store().recipient_for(account.aci)?;
        self.store().set_profile_key(me, &account.profile_key)?;
        self.shared.me.set(me);
        *self.shared.group_credential.borrow_mut() = None;
        self.receiver = Some(Receiver::new(self.store(), account.clone())?);
        *self.shared.account.borrow_mut() = Some(account);
        Ok(())
    }

    fn account(&self) -> Result<Account> {
        self.shared.account.borrow().clone().context("not linked")
    }

    fn wire(&self) -> Option<Rc<Wire>> {
        self.shared.wire.borrow().clone()
    }

    /// Open both connections, off the loop; `Input::Up` says how it went.
    fn connect(&mut self) {
        let Ok(account) = self.account() else { return };
        // For looking at the window over an invented store (`demo`).
        if std::env::var_os("SOLA_SIGNAL_OFFLINE").is_some() {
            (self.emit)(Event::Offline("kept offline".into()));
            return;
        }
        self.epoch += 1;
        self.early.clear();
        *self.shared.wire.borrow_mut() = None;
        (self.emit)(Event::Connecting);
        let (epoch, net, inputs) = (self.epoch, self.net.clone(), self.inputs.clone());
        tokio::task::spawn_local(async move {
            let events = inputs.clone();
            let listener = Box::new(move |e| match ServerEvent::try_from(e) {
                Ok(e) => drop(events.send(Input::Server(epoch, e))),
                Err(e) => tracing::debug!("chat socket: {e}"),
            });
            let wire = async {
                let chat = net.connect(net::CHAT_WEBSOCKET_PATH, Some(net::sign_in(&account)), listener, "chat").await?;
                // Sealed sends go on this one; when it closes is worth knowing.
                let closed = Box::new(|e| {
                    if let Ok(ServerEvent::Stopped(cause)) = ServerEvent::try_from(e) {
                        tracing::info!(?cause, "the connection that is not signed in closed");
                    }
                });
                let unauth = net.connect(net::CHAT_WEBSOCKET_PATH, None, closed, "unauth").await?;
                let certificate = match certificate(&chat).await {
                    Ok(certificate) => Some(certificate),
                    Err(e) => {
                        tracing::warn!("no sender certificate, so nothing goes sealed: {e:#}");
                        None
                    }
                };
                Ok(Box::new(Wire { chat, unauth, certificate: RefCell::new(certificate) }))
            }
            .await;
            let _ = inputs.send(Input::Up(epoch, wire));
        });
    }

    fn connect_later(&mut self) {
        let wait = BACKOFF[self.failed.min(BACKOFF.len() - 1)];
        self.failed += 1;
        self.connect_after(wait);
    }

    fn connect_after(&mut self, seconds: u64) {
        let (epoch, inputs) = (self.epoch, self.inputs.clone());
        tokio::task::spawn_local(async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            let _ = inputs.send(Input::Reconnect(epoch));
        });
    }

    /// Close both connections. The count moves on, so that what the closed
    /// ones still say is not heard.
    async fn hang_up(&mut self, why: &str) {
        self.epoch += 1;
        let wire = self.shared.wire.borrow_mut().take();
        if let Some(wire) = wire {
            wire.chat.disconnect().await;
            wire.unauth.disconnect().await;
        }
        (self.emit)(Event::Offline(why.into()));
    }

    async fn handle(&mut self, input: Input) -> Result<()> {
        match input {
            Input::Command(command) => self.command(command).await,
            // The socket delivers before both connections are up: what it
            // says waits until they are.
            Input::Server(epoch, event) if epoch == self.epoch && self.wire().is_none() => {
                match event {
                    ServerEvent::Stopped(_) => self.server(event).await?,
                    event => self.early.push(event),
                }
                Ok(())
            }
            Input::Server(epoch, event) if epoch == self.epoch => self.server(event).await,
            Input::Server(..) => Ok(()),
            Input::Up(epoch, wire) if epoch == self.epoch => self.up(wire).await,
            Input::Up(..) => Ok(()),
            Input::Reconnect(epoch) => {
                if epoch == self.epoch && self.wire().is_none() {
                    self.connect();
                }
                Ok(())
            }
            // The loop sends what waits once this is handled.
            Input::Settle => Ok(()),
            Input::LinkCode(url) => {
                (self.emit)(Event::LinkCode(url));
                Ok(())
            }
            Input::Linked(linked) => self.linked(linked),
            Input::Done(effects) => self.apply(effects).await,
        }
    }

    async fn up(&mut self, wire: std::result::Result<Box<Wire>, ConnectError>) -> Result<()> {
        let wire = match wire {
            Ok(wire) => Rc::new(*wire),
            Err(ConnectError::DeviceDeregistered) => {
                // The phone took this device off its list.
                tracing::warn!("this device is no longer linked");
                *self.shared.account.borrow_mut() = None;
                self.receiver = None;
                (self.emit)(Event::Unlinked);
                return Ok(());
            }
            Err(e) => {
                (self.emit)(Event::Offline(e.to_string()));
                self.connect_later();
                return Ok(());
            }
        };
        self.failed = 0;
        *self.shared.wire.borrow_mut() = Some(wire.clone());
        (self.emit)(Event::Online);

        let account = self.account()?;
        let aci = self.store().protocol(Identity::Aci, account.aci_keys.clone());
        keys::replenish(&wire.chat, &aci, Identity::Aci, &account.aci_keys).await?;
        if let Some((_, keys)) = &account.pni {
            let pni = self.store().protocol(Identity::Pni, keys.clone());
            keys::replenish(&wire.chat, &pni, Identity::Pni, keys).await?;
        }
        // Once per link: ask the phone for what it knows.
        if self.store().setting("asked")?.is_none() {
            use sync_message::request::Type;
            for kind in [Type::Contacts, Type::Blocked, Type::Configuration, Type::Keys] {
                let request = sync_message::Request { r#type: Some(kind as i32) };
                self.enqueue_sync(sync_message::Content::Request(request))?;
            }
            self.store().set_setting("asked", &[1])?;
        }
        let _ = self.jobs.send(Job::Profile(self.shared.me.get()));
        let _ = self.jobs.send(Job::Storage);
        let _ = self.jobs.send(Job::Stale);
        for event in std::mem::take(&mut self.early) {
            if let Err(e) = self.server(event).await {
                tracing::warn!("{e:#}");
            }
        }
        Ok(())
    }

    async fn server(&mut self, event: ServerEvent) -> Result<()> {
        match event {
            ServerEvent::IncomingMessage { envelope, send_ack, .. } => {
                if let Err(e) = self.incoming(&envelope).await {
                    tracing::warn!("an envelope was dropped: {e:#}");
                }
                // Answered once it is kept: until then the server holds it.
                send_ack(http::StatusCode::OK).map_err(|e| anyhow!("ack: {e}"))?;
            }
            ServerEvent::QueueEmpty => tracing::info!("caught up with the server's queue"),
            ServerEvent::ServerTimestamp(_) => {}
            ServerEvent::Alerts(alerts) => {
                if !alerts.is_empty() {
                    tracing::warn!(?alerts, "the server has alerts for this device");
                }
            }
            ServerEvent::Stopped(cause) => {
                tracing::info!(?cause, "the chat socket closed");
                *self.shared.wire.borrow_mut() = None;
                (self.emit)(Event::Offline("the connection closed".into()));
                self.connect_later();
            }
        }
        Ok(())
    }

    async fn incoming(&mut self, envelope: &[u8]) -> Result<()> {
        let receiver = self.receiver.as_ref().context("not linked")?;
        match receiver.open(envelope).await? {
            Opened::Message(received) => {
                let effects = self.inbox().take(*received)?;
                self.apply(effects).await
            }
            Opened::Nothing => Ok(()),
            Opened::Unreadable(unreadable) => self.ask_again(unreadable).await,
        }
    }

    /// Do what a message left to do beyond the store.
    async fn apply(&mut self, fx: Effects) -> Result<()> {
        for file in &fx.remove {
            let _ = std::fs::remove_file(file);
        }
        for conversation in &fx.changed {
            (self.emit)(Event::Changed(*conversation));
        }
        if fx.people {
            if let Some(account) = self.store().account()? {
                *self.shared.account.borrow_mut() = Some(account);
            }
            (self.emit)(Event::People);
        }
        for arrived in fx.arrived {
            let chats = self.store().chats()?;
            let Some(chat) = chats.iter().find(|chat| chat.id == arrived.conversation) else { continue };
            if chat.muted {
                continue;
            }
            let sender = self.store().recipient(arrived.sender)?.name();
            let title = if chat.group { format!("{sender} in {}", chat.title) } else { sender };
            (self.emit)(Event::Arrived { conversation: arrived.conversation, title, text: arrived.text });
        }
        if let Some((conversation, who, started)) = fx.typing {
            let who = self.store().recipient(who)?.name();
            (self.emit)(Event::Typing { conversation, who, started });
        }
        self.owed.extend(fx.receipts);
        for group in fx.groups {
            let _ = self.jobs.send(Job::Group(group));
        }
        for profile in fx.profiles {
            let _ = self.jobs.send(Job::Profile(profile));
        }
        if let Some(pointer) = fx.contacts {
            let _ = self.jobs.send(Job::Contacts(Box::new(pointer)));
        }
        for attachment in fx.fetch {
            jobs::fetch(self.shared.clone(), self.inputs.clone(), attachment);
        }
        if fx.storage {
            let _ = self.jobs.send(Job::Storage);
        }
        if let Some((who, device, notice)) = fx.unread_by {
            self.send_again(who, device, &notice).await?;
        }
        Ok(())
    }

    /// The queue is quiet: receipts owed, then the outbox.
    async fn settle(&mut self) {
        if self.wire().is_none() {
            return;
        }
        let mut owed = std::mem::take(&mut self.owed);
        owed.sort();
        for receipts in owed.chunk_by(|a, b| a.0 == b.0) {
            let receipt = proto::ReceiptMessage {
                r#type: Some(receipt_message::Type::Delivery as i32),
                timestamp: receipts.iter().map(|(_, timestamp)| *timestamp).collect(),
            };
            let content = Content { content: Some(proto::content::Content::ReceiptMessage(receipt)), ..Default::default() };
            if let Err(e) = self.send_to(receipts[0].0, &content, now(), false, ContentHint::Implicit, None).await {
                tracing::debug!("a delivery receipt was not sent: {e}");
            }
        }
        if let Err(e) = self.drain().await {
            tracing::warn!("the outbox: {e:#}");
        }
    }

    /// Remove what has disappeared.
    fn expire(&mut self) {
        match self.store().expire(now()) {
            Ok((conversations, files)) => {
                for file in files {
                    let _ = std::fs::remove_file(file);
                }
                for conversation in conversations {
                    (self.emit)(Event::Changed(conversation));
                }
            }
            Err(e) => tracing::warn!("expiring messages: {e:#}"),
        }
    }

    // Sending.

    /// Send one `Content` to one account, sealed if we hold their profile key.
    async fn send_to(
        &self,
        to: Id,
        content: &Content,
        timestamp: u64,
        urgent: bool,
        hint: ContentHint,
        group: Option<&[u8]>,
    ) -> std::result::Result<(ServiceId, bool), Failure> {
        let never = |e: anyhow::Error| Failure::Never(e);
        let wire = self.wire().ok_or_else(|| Failure::Later(anyhow!("not connected")))?;
        let account = self.account().map_err(never)?;
        let recipient = self.store().recipient(to).map_err(never)?;
        let aci = recipient.aci.context("someone with no account id").map_err(never)?;
        let certificate = self.certificate(&wire).await;
        let sealed = certificate.as_ref().map(|certificate| Sealed { chat: &wire.unauth, certificate });
        let options = Options {
            timestamp,
            urgent,
            access_key: recipient.profile_key.as_deref().and_then(profile::access_key),
            hint,
            group,
        };
        let sender = Sender::new(self.store(), &account, &wire.chat, sealed);
        let sealed = sender.deliver(aci.into(), content, &options).await?;
        Ok((aci.into(), sealed))
    }

    /// The sender certificate, fetched again when it has under an hour left.
    async fn certificate(&self, wire: &Wire) -> Option<SenderCertificate> {
        let held = wire.certificate.borrow().clone();
        let soon = Timestamp::from_epoch_millis(now() + 3_600_000);
        if let Some(certificate) = &held
            && certificate.expiration().is_ok_and(|expires| expires > soon)
        {
            return held;
        }
        let fresh = certificate(&wire.chat).await.inspect_err(|e| tracing::warn!("no sender certificate: {e:#}")).ok();
        *wire.certificate.borrow_mut() = fresh.clone();
        fresh
    }

    /// Send what is in the outbox, oldest first, until something has to
    /// wait. What fails on a connection that is up is tried again, on a new
    /// connection if this one did not carry it, and given up after `TRIES`.
    async fn drain(&mut self) -> Result<()> {
        if self.hold.is_some_and(|until| tokio::time::Instant::now() < until) {
            return Ok(());
        }
        self.hold = None;
        let mut after = 0;
        while let Some(item) = self.store().next_outgoing(after)? {
            after = item.id;
            match self.send_item(&item).await {
                Ok(()) => {
                    self.stuck = None;
                    self.store().sent(item.id)?;
                }
                Err(Failure::Never(e)) => {
                    tracing::warn!("not sent: {e:#}");
                    self.give_up(&item)?;
                }
                Err(wait) if self.wire().is_none() => {
                    tracing::info!("sending waits for a connection: {wait}");
                    break;
                }
                Err(wait) => {
                    let tries = match self.stuck {
                        Some((id, tries)) if id == item.id => tries + 1,
                        _ => 1,
                    };
                    self.stuck = Some((item.id, tries));
                    if tries >= TRIES {
                        tracing::warn!(tries, "not sent, and given up: {wait}");
                        self.give_up(&item)?;
                    } else {
                        let seconds = BACKOFF[(tries - 1).min(BACKOFF.len() - 1)];
                        match wait {
                            Failure::Dropped(e) => {
                                tracing::warn!(tries, "the connection did not carry what was sent; opening another: {e:#}");
                                self.hang_up("a send was not carried").await;
                                self.connect_after(seconds);
                            }
                            wait => {
                                tracing::info!(tries, "sending is put off: {wait}");
                                self.hold = Some(tokio::time::Instant::now() + Duration::from_secs(seconds));
                                let inputs = self.inputs.clone();
                                tokio::task::spawn_local(async move {
                                    tokio::time::sleep(Duration::from_secs(seconds)).await;
                                    let _ = inputs.send(Input::Settle);
                                });
                            }
                        }
                        break;
                    }
                }
            }
            if let Some(conversation) = item.conversation {
                (self.emit)(Event::Changed(conversation));
            }
        }
        Ok(())
    }

    /// Take out of the outbox what will not go; a message says so.
    fn give_up(&mut self, item: &Outgoing) -> Result<()> {
        self.stuck = None;
        self.store().sent(item.id)?;
        if let Some(message) = item.message {
            self.store().raise_status(message, Status::Failed)?;
        }
        Ok(())
    }

    async fn send_item(&mut self, item: &Outgoing) -> std::result::Result<(), Failure> {
        let never = |e: anyhow::Error| Failure::Never(e);
        let me = self.shared.me.get();
        let content = match &item.content {
            Some(bytes) => <Content as prost::Message>::decode(bytes.as_slice()).context("outbox").map_err(never)?,
            None => {
                let message = item.message.context("an outbox row with nothing to send").map_err(never)?;
                let conversation = item.conversation.context("a message with no conversation").map_err(never)?;
                outgoing::message(&self.shared, conversation, message, item.timestamp).await?
            }
        };
        let (targets, group) = match (item.conversation, item.recipient) {
            (Some(conversation), _) => {
                let conversation = self.store().conversation(conversation).map_err(never)?;
                match (conversation.recipient, conversation.group) {
                    (Some(recipient), _) => (vec![recipient], None),
                    (_, Some(group)) => {
                        // Never read from the server: we do not know who is
                        // in it, and it must not go to no one.
                        if self.store().group(group).map_err(never)?.revision < 0 {
                            jobs::group(&self.shared, group, &self.jobs).await.map_err(Failure::Later)?;
                        }
                        let group = self.store().group(group).map_err(never)?;
                        let identifier = inbox::group_identifier(&group.master_key).map_err(never)?;
                        (group.members.into_iter().filter(|member| *member != me).collect(), Some(identifier))
                    }
                    _ => return Err(never(anyhow!("a conversation with no one in it"))),
                }
            }
            (None, Some(recipient)) => (vec![recipient], None),
            (None, None) => return Err(never(anyhow!("an outbox row for no one"))),
        };
        let said = matches!(
            content.content,
            Some(proto::content::Content::DataMessage(_) | proto::content::Content::EditMessage(_))
        );
        let hint = if said { ContentHint::Resendable } else { ContentHint::Implicit };

        let mut delivered = Vec::new();
        let mut refused = None;
        for target in targets.iter().filter(|target| **target != me || !said) {
            let group = group.as_ref().map(|g| g.as_slice());
            match self.send_to(*target, &content, item.timestamp, item.urgent, hint, group).await {
                Ok(done) => delivered.push(done),
                Err(Failure::Never(e)) => {
                    tracing::warn!("not sent to one recipient: {e:#}");
                    refused = Some(e);
                }
                Err(wait) => return Err(wait),
            }
        }
        if delivered.is_empty()
            && let Some(e) = refused
        {
            return Err(Failure::Never(e));
        }
        if said {
            // Our other devices are told, so the phone shows it too. For a
            // Note to Self this is all that goes out.
            let mut sent = sync_message::Sent::default();
            let mut expires = false;
            match content.content {
                Some(proto::content::Content::DataMessage(message)) => {
                    expires = message.expire_timer() > 0;
                    sent.message = Some(message);
                }
                Some(proto::content::Content::EditMessage(edit)) => sent.edit_message = Some(edit),
                _ => {}
            }
            sent.expiration_start_timestamp = expires.then(now);
            let to = match (group.is_some(), targets.first()) {
                (false, Some(target)) => self.store().recipient(*target).map_err(never)?.aci.map(ServiceId::from),
                _ => None,
            };
            let transcript = send::transcript(to, &delivered, sent, item.timestamp);
            match self.send_to(me, &transcript, item.timestamp, false, ContentHint::Implicit, None).await {
                Ok(_) => {}
                // What was said has gone out; saying it again to everyone
                // for the transcript's sake would be worse than its lack.
                Err(e) if !delivered.is_empty() => tracing::warn!("no transcript for our other devices: {e}"),
                Err(e) => return Err(e),
            }
        }
        if let Some(message) = item.message {
            self.store().raise_status(message, Status::Sent).map_err(never)?;
            self.store().start_timer(message, now()).map_err(never)?;
        }
        Ok(())
    }

    fn enqueue_sync(&self, content: sync_message::Content) -> Result<()> {
        let content = send::sync(content);
        self.store().enqueue(&Outgoing {
            id: 0,
            conversation: None,
            recipient: Some(self.shared.me.get()),
            message: None,
            timestamp: now(),
            content: Some(prost::Message::encode_to_vec(&content)),
            urgent: false,
        })?;
        Ok(())
    }

    /// Ask the sender of something unreadable to send it again.
    async fn ask_again(&mut self, unreadable: Unreadable) -> Result<()> {
        tracing::warn!(
            from = %unreadable.sender.service_id_string(),
            device = u8::from(unreadable.sender_device),
            "a message did not decrypt ({}); asking for it again",
            unreadable.why
        );
        let (Some(wire), Ok(account)) = (self.wire(), self.account()) else { return Ok(()) };
        let notice = DecryptionErrorMessage::for_original(
            &unreadable.ciphertext,
            unreadable.kind,
            Timestamp::from_epoch_millis(unreadable.timestamp),
            u8::from(unreadable.sender_device).into(),
        )
        .map_err(|e| anyhow!("retry request: {e}"))?;
        let sender = Sender::new(self.store(), &account, &wire.chat, None);
        sender
            .notice(unreadable.sender, &PlaintextContent::from(notice), now())
            .await
            .map_err(|e| anyhow!("retry request: {e}"))
    }

    /// A device could not read something we sent: end the session it failed
    /// under and send the message again on a new one.
    async fn send_again(&mut self, who: Id, device: libsignal_core::DeviceId, notice: &[u8]) -> Result<()> {
        let notice = DecryptionErrorMessage::try_from(notice).map_err(|e| anyhow!("retry request: {e}"))?;
        let account = self.account()?;
        if notice.device_id() != u32::from(u8::from(account.device_id)) {
            return Ok(());
        }
        let recipient = self.store().recipient(who)?;
        let Some(aci) = recipient.aci else { return Ok(()) };
        let address = ProtocolAddress::new(aci.service_id_string(), device);
        self.store()
            .protocol(Identity::Aci, account.aci_keys.clone())
            .archive_session(&address)
            .await
            .map_err(|e| anyhow!("{address}: {e}"))?;
        let timestamp = notice.timestamp().epoch_millis();
        let Some((message, conversation)) = self.store().find_message(self.shared.me.get(), timestamp)? else {
            return Ok(());
        };
        tracing::info!("sending a message again that a device could not read");
        let content = outgoing::message(&self.shared, conversation, message, timestamp).await.map_err(|e| anyhow!("{e}"))?;
        // Only to the account that asked; the rest of a group has it.
        self.send_to(who, &content, timestamp, true, ContentHint::Resendable, None).await.map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }

    // Linking.

    fn linked(&mut self, linked: Result<Box<Linked>>) -> Result<()> {
        self.linking = false;
        match linked {
            Ok(linked) => {
                self.store().save_link(&linked).context("keeping the link")?;
                self.take_account(linked.account)?;
                (self.emit)(Event::Linked);
                self.connect();
            }
            Err(e) => (self.emit)(Event::LinkFailed(format!("{e:#}"))),
        }
        Ok(())
    }

    // What the window asks.

    async fn command(&mut self, command: Command) -> Result<()> {
        let me = self.shared.me.get();
        match command {
            Command::Link { device_name } => {
                if self.linking || self.shared.account.borrow().is_some() {
                    return Ok(());
                }
                self.linking = true;
                let (net, inputs) = (self.net.clone(), self.inputs.clone());
                tokio::task::spawn_local(async move {
                    let code = inputs.clone();
                    let linked = link::link(&net, &device_name, move |url| drop(code.send(Input::LinkCode(url.to_owned())))).await;
                    let _ = inputs.send(Input::Linked(linked.map(Box::new)));
                });
            }
            Command::Connect => {
                if self.wire().is_none() && self.shared.account.borrow().is_some() {
                    self.failed = 0;
                    self.connect();
                }
            }
            Command::Send { conversation, body, quote, files } => {
                outgoing::compose(&self.shared, conversation, &body, quote, &files)?;
                (self.emit)(Event::Changed(conversation));
            }
            Command::Read { conversation } => {
                let read = self.store().read_conversation(conversation, now())?;
                if read.is_empty() {
                    return Ok(());
                }
                (self.emit)(Event::Changed(conversation));
                self.tell_of_reading(read)?;
            }
            Command::React { message, emoji } => {
                let (conversation, author, target) = self.store().message_address(message)?;
                let before = self.store().reaction_of(message, me)?;
                let Some(shown) = emoji.clone().or(before) else { return Ok(()) };
                let author = self.store().recipient(author)?.aci.context("a message from no one")?;
                let reaction = proto::data_message::Reaction {
                    emoji: Some(shown),
                    remove: Some(emoji.is_none()),
                    target_author_aci: Some(author.service_id_string()),
                    target_sent_timestamp: Some(target),
                    ..Default::default()
                };
                let timestamp = now();
                self.store().set_reaction(message, me, emoji.as_deref(), timestamp)?;
                let mut said = outgoing::frame(&self.shared, conversation, timestamp)?;
                said.reaction = Some(reaction);
                outgoing::enqueue(&self.shared, conversation, None, timestamp, proto::content::Content::DataMessage(said))?;
                (self.emit)(Event::Changed(conversation));
            }
            Command::Edit { message, body } => {
                let (conversation, author, target) = self.store().message_address(message)?;
                if author != me {
                    return Ok(());
                }
                let timestamp = now();
                let mut said = outgoing::frame(&self.shared, conversation, timestamp)?;
                said.body = Some(body.clone());
                let edit = proto::EditMessage { target_sent_timestamp: Some(target), data_message: Some(said) };
                self.store().edit_message(message, timestamp, &body)?;
                outgoing::enqueue(&self.shared, conversation, None, timestamp, proto::content::Content::EditMessage(edit))?;
                (self.emit)(Event::Changed(conversation));
            }
            Command::Retract { message } => {
                let (conversation, author, target) = self.store().message_address(message)?;
                if author != me {
                    return Ok(());
                }
                let timestamp = now();
                let mut said = outgoing::frame(&self.shared, conversation, timestamp)?;
                said.delete = Some(proto::data_message::Delete { target_sent_timestamp: Some(target) });
                for file in self.store().retract(message)? {
                    let _ = std::fs::remove_file(file);
                }
                outgoing::enqueue(&self.shared, conversation, None, timestamp, proto::content::Content::DataMessage(said))?;
                (self.emit)(Event::Changed(conversation));
            }
            Command::Delete { message } => {
                let (conversation, ..) = self.store().message_address(message)?;
                for file in self.store().delete_message(message)? {
                    let _ = std::fs::remove_file(file);
                }
                (self.emit)(Event::Changed(conversation));
            }
            Command::SetTimer { conversation, seconds } => {
                let version = self.store().conversation(conversation)?.expire_timer_version + 1;
                if !self.store().set_timer(conversation, seconds, version)? {
                    return Ok(());
                }
                let timestamp = now();
                let mut said = outgoing::frame(&self.shared, conversation, timestamp)?;
                said.flags = Some(proto::data_message::Flags::ExpirationTimerUpdate as u32);
                said.expire_timer = Some(seconds);
                outgoing::enqueue(&self.shared, conversation, None, timestamp, proto::content::Content::DataMessage(said))?;
                let text = match seconds {
                    0 => "You turned off disappearing messages.".to_owned(),
                    _ => format!("You set messages to disappear after {}.", inbox::duration(seconds)),
                };
                let mut fx = Effects::default();
                self.inbox().note(conversation, me, &text, now(), false, &mut fx)?;
                (self.emit)(Event::Changed(conversation));
            }
            Command::Fetch { attachment } => jobs::fetch(self.shared.clone(), self.inputs.clone(), attachment),
            Command::Retry { message } => {
                let (conversation, _, timestamp) = self.store().message_address(message)?;
                self.store().retry(message)?;
                self.store().enqueue(&Outgoing {
                    id: 0,
                    conversation: Some(conversation),
                    recipient: None,
                    message: Some(message),
                    timestamp,
                    content: None,
                    urgent: true,
                })?;
                (self.emit)(Event::Changed(conversation));
            }
        }
        Ok(())
    }

    /// Tell the senders (if the account sends read receipts) and our other
    /// devices that messages were read here.
    fn tell_of_reading(&mut self, mut read: Vec<(Id, u64)>) -> Result<()> {
        let me = self.shared.me.get();
        read.retain(|(sender, _)| *sender != me);
        read.sort();
        let receipts = self.account()?.read_receipts;
        let mut seen = Vec::new();
        for by_sender in read.chunk_by(|a, b| a.0 == b.0) {
            let Some(aci) = self.store().recipient(by_sender[0].0)?.aci else { continue };
            let timestamps: Vec<u64> = by_sender.iter().map(|(_, timestamp)| *timestamp).collect();
            seen.extend(timestamps.iter().map(|timestamp| sync_message::Read {
                sender_aci: Some(aci.service_id_string()),
                timestamp: Some(*timestamp),
                ..Default::default()
            }));
            if receipts {
                let receipt = proto::ReceiptMessage { r#type: Some(receipt_message::Type::Read as i32), timestamp: timestamps };
                let content = Content { content: Some(proto::content::Content::ReceiptMessage(receipt)), ..Default::default() };
                self.store().enqueue(&Outgoing {
                    id: 0,
                    conversation: None,
                    recipient: Some(by_sender[0].0),
                    message: None,
                    timestamp: now(),
                    content: Some(prost::Message::encode_to_vec(&content)),
                    urgent: false,
                })?;
            }
        }
        if !seen.is_empty() {
            let mut content = send::sync(sync_message::Content::Request(Default::default()));
            if let Some(proto::content::Content::SyncMessage(sync)) = &mut content.content {
                sync.content = None;
                sync.read = seen;
            }
            self.store().enqueue(&Outgoing {
                id: 0,
                conversation: None,
                recipient: Some(me),
                message: None,
                timestamp: now(),
                content: Some(prost::Message::encode_to_vec(&content)),
                urgent: false,
            })?;
        }
        Ok(())
    }
}

/// Signal's certificate that this device is who its sealed messages say,
/// without our phone number in it.
async fn certificate(chat: &ChatConnection) -> Result<SenderCertificate> {
    #[derive(serde::Deserialize)]
    struct Answer {
        certificate: String,
    }
    let response = net::request(chat, http::Method::GET, "/v1/certificate/delivery?include-e164=false", None, None).await?;
    let answer: Answer = serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("certificate")?;
    SenderCertificate::deserialize(&BASE64.decode(answer.certificate)?).map_err(|e| anyhow!("certificate: {e}"))
}

/// Try against Signal what a conversation with someone else would try,
/// with no message to anyone: sign in, fetch our own profile, put a file
/// on the file server and take it back.
pub async fn selftest(store: Store) -> Result<()> {
    let account = store.account()?.context("not linked")?;
    let net = Net::new();
    let chat = net.connect(net::CHAT_WEBSOCKET_PATH, Some(net::sign_in(&account)), Box::new(|_| {}), "chat").await?;
    let unauth = net.connect(net::CHAT_WEBSOCKET_PATH, None, Box::new(|_| {}), "unauth").await?;
    println!("signed in");
    let certificate = certificate(&chat).await?;
    println!("sender certificate: until {}", certificate.expiration().map_err(|e| anyhow!("{e}"))?.epoch_millis());
    let me = store.recipient_for(account.aci)?;
    let wire = Rc::new(Wire { chat, unauth, certificate: RefCell::new(Some(certificate)) });
    let named = profile::name(&wire, account.aci, &account.profile_key).await?.is_some();
    println!("own profile: {}", if named { "read, with a name" } else { "read, no name" });
    let shared = Shared {
        store,
        wire: RefCell::new(Some(wire)),
        account: RefCell::new(Some(account)),
        me: std::cell::Cell::new(me),
        group_credential: RefCell::new(None),
    };

    let plain: Vec<u8> = (0..70_000u32).map(|n| (n * 31 % 251) as u8).collect();
    let path = std::env::temp_dir().join(format!("sola-signal-selftest-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, &plain)?;
    let attachment = crate::store::chats::Attachment {
        id: 0,
        message: 0,
        content_type: "application/octet-stream".into(),
        file_name: None,
        size: plain.len() as u64,
        width: 0,
        height: 0,
        pointer: None,
        path: Some(path.to_string_lossy().into_owned()),
        state: crate::store::chats::Fetch::Here,
    };
    let uploaded = files::upload(&shared, &attachment).await;
    let _ = std::fs::remove_file(&path);
    let pointer = uploaded.map_err(|e| anyhow!("upload: {e}"))?;
    println!("uploaded {} bytes to file server {}", plain.len(), pointer.cdn_number());
    let back = files::download(&pointer).await.context("download")?;
    anyhow::ensure!(back == plain, "what came back is not what went up");
    println!("downloaded and decrypted: the same bytes");
    Ok(())
}
