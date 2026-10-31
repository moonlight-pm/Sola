//! Requests and their answers, as futures an iced `Task` can wait on. The
//! socket is blocking, so it lives on threads: one worker keeps a
//! connection for the quick requests, in order; a slow request gets a
//! thread and a connection of its own.
//!
//! Nothing here logs a request or an answer: either can hold a secret.

use std::future::Future;
use std::sync::OnceLock;
use std::sync::mpsc;

use iced::futures::channel::oneshot;
use sola_vault_proto::{Client, ErrorCode, Reply, Request, VaultError, call};

use super::CLIENT;

pub type Answer = Result<Reply, VaultError>;

struct Job {
    req: Request,
    tx: oneshot::Sender<Answer>,
}

/// Ask on the worker's connection; answers come in the order asked.
pub fn ask(req: Request) -> impl Future<Output = Answer> + Send + 'static {
    let (tx, rx) = oneshot::channel();
    let sent = worker().send(Job { req, tx }).is_ok();
    async move {
        if !sent {
            return Err(gone());
        }
        rx.await.unwrap_or_else(|_| Err(gone()))
    }
}

/// Ask on a connection of its own, for a request that may take a while.
pub fn ask_alone(req: Request) -> impl Future<Output = Answer> + Send + 'static {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("vault-ask".into())
        .spawn(move || {
            let _ = tx.send(call(CLIENT, req));
        })
        .is_ok();
    async move {
        if !spawned {
            return Err(gone());
        }
        rx.await.unwrap_or_else(|_| Err(gone()))
    }
}

fn gone() -> VaultError {
    VaultError::new(ErrorCode::Unavailable, "The vault service isn't running.")
}

fn worker() -> &'static mpsc::Sender<Job> {
    static TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("vault-worker".into())
            .spawn(move || run(rx))
            .expect("spawn the vault worker");
        tx
    })
}

fn run(rx: mpsc::Receiver<Job>) {
    let mut conn: Option<Client> = None;
    for Job { req, tx } in rx {
        // A read can be asked again on a new connection when the old one
        // died (the daemon restarted); a write cannot — it may have landed.
        let again = read_only(&req).then(|| req.clone());
        let mut answer = attempt(&mut conn, req);
        if let (Err(e), Some(req)) = (&answer, again)
            && e.code == ErrorCode::Unavailable
        {
            answer = attempt(&mut conn, req);
        }
        let _ = tx.send(answer);
    }
}

fn attempt(conn: &mut Option<Client>, req: Request) -> Answer {
    if conn.is_none() {
        *conn = Some(Client::connect(CLIENT)?);
        tracing::info!("connected to the vault service");
    }
    let Some(client) = conn.as_mut() else {
        return Err(gone());
    };
    let answer = client.call(req);
    if matches!(&answer, Err(e) if e.code == ErrorCode::Unavailable) {
        *conn = None;
    }
    answer
}

fn read_only(req: &Request) -> bool {
    matches!(
        req,
        Request::Status
            | Request::Settings
            | Request::List { .. }
            | Request::Get { .. }
            | Request::Draft { .. }
            | Request::Code { .. }
            | Request::Folders
            | Request::Organizations
            | Request::Collections
    )
}
