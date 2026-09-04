//! Per-account IMAP write session. MOVE / empty / Sent-append run here so
//! the list/fetch session is never blocked on a trash backlog.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tracing::{info, warn};

use crate::bridge;
use crate::protocol::boxes::{self, MailboxMap};
use crate::protocol::{Account, ImapClient};

use super::cmds::MailEvent;

pub(crate) enum WriteCmd {
    Move {
        folder: String,
        uid: u32,
        dest: String,
    },
    Empty {
        folder: String,
    },
    Append {
        remote: String,
        raw: Vec<u8>,
    },
    Shutdown,
}

struct WriteTarget {
    dedicated: Option<ImapClient>,
    shared: Option<Arc<Mutex<ImapClient>>>,
}

impl WriteTarget {
    fn with_client<T>(&mut self, f: impl FnOnce(&mut ImapClient) -> T) -> Option<T> {
        if let Some(c) = self.dedicated.as_mut() {
            return Some(f(c));
        }
        if let Some(arc) = &self.shared {
            let mut c = arc.lock().unwrap_or_else(|e| e.into_inner());
            return Some(f(&mut c));
        }
        None
    }
}

pub(crate) fn spawn(
    account: Account,
    map: MailboxMap,
    dedicated: Option<ImapClient>,
    shared: Option<Arc<Mutex<ImapClient>>>,
) -> (mpsc::Sender<WriteCmd>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let id = account.id();
    let handle = std::thread::Builder::new()
        .name(format!("sola-mail-w-{id}"))
        .spawn(move || {
            let mut target = WriteTarget { dedicated, shared };
            loop {
                let cmd = match rx.recv_timeout(Duration::from_secs(240)) {
                    Ok(c) => c,
                    Err(RecvTimeoutError::Timeout) => {
                        let _ = target.with_client(|c| {
                            if let Err(e) = c.noop() {
                                warn!(account = id.as_str(), "write keepalive NOOP: {e}");
                            }
                        });
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                match cmd {
                    WriteCmd::Shutdown => break,
                    WriteCmd::Move { folder, uid, dest } => {
                        let src = boxes::remote(&map, &folder)
                            .unwrap_or(folder.as_str())
                            .to_string();
                        let dst = boxes::remote(&map, &dest)
                            .unwrap_or(dest.as_str())
                            .to_string();
                        let result = target.with_client(|c| c.move_message(&src, uid, &dst));
                        match result {
                            Some(Ok(dest_uid)) => {
                                if dest_uid.is_none() {
                                    warn!(
                                        account = id.as_str(),
                                        uid,
                                        src = src.as_str(),
                                        dst = dst.as_str(),
                                        "move: no destination UID"
                                    );
                                }
                                bridge::emit(MailEvent::Moved {
                                    account: id.clone(),
                                    uid,
                                    dest_uid,
                                });
                            }
                            Some(Err(e)) => bridge::emit(MailEvent::MoveFailed {
                                account: id.clone(),
                                uid,
                                message: e.to_string(),
                            }),
                            None => bridge::emit(MailEvent::MoveFailed {
                                account: id.clone(),
                                uid,
                                message: "No IMAP write session".into(),
                            }),
                        }
                    }
                    WriteCmd::Empty { folder } => {
                        let Some(remote) = boxes::remote(&map, &folder).map(str::to_string) else {
                            continue;
                        };
                        let result = target.with_client(|c| c.empty_folder(&remote));
                        match result {
                            Some(Ok(())) => bridge::emit(MailEvent::Emptied {
                                folder: folder.clone(),
                            }),
                            Some(Err(e)) => {
                                warn!(
                                    account = id.as_str(),
                                    remote = remote.as_str(),
                                    "empty failed: {e}"
                                );
                                bridge::emit(MailEvent::Error {
                                    context: "empty_folder".into(),
                                    message: e.to_string(),
                                });
                            }
                            None => bridge::emit(MailEvent::Error {
                                context: "empty_folder".into(),
                                message: "No IMAP write session".into(),
                            }),
                        }
                    }
                    WriteCmd::Append { remote, raw } => {
                        let _ = target.with_client(|c| {
                            if let Err(e) = c.append_to_sent(&remote, &raw) {
                                warn!("Failed to save to Sent folder: {e}");
                            }
                        });
                    }
                }
            }
            info!(account = id.as_str(), "mail write worker stopped");
        })
        .expect("spawn mail write worker");
    (tx, handle)
}
