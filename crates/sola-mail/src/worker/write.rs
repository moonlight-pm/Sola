//! Per-account write session: MOVE / delete / erase / append / move
//! rules, so the read session is never blocked on a trash backlog.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;

use imap::types::Flag as ImapFlag;
use tracing::{info, warn};

use super::cmds::{MailEvent, Op};
use super::{Input, KEEPALIVE, Part, Rules};
use crate::bridge;
use crate::protocol::boxes::{self, MailboxMap};
use crate::protocol::rules::{move_dest, rule_matches};
use crate::protocol::{Account, ImapClient, MailId};

/// Tell the UI after a rules pass?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    /// New mail arrived: always refresh.
    Always,
    /// Connect-time sweep: only if something moved.
    IfMoved,
}

pub enum WriteCmd {
    Move {
        id: MailId,
        dest: String,
    },
    Delete(MailId),
    Empty {
        job: u64,
        folder: String,
    },
    Append {
        folder: String,
        raw: Vec<u8>,
        /// Report the result (a saved draft); a Sent copy only logs.
        draft: bool,
        /// Older copy on this account to delete once the append lands.
        replaces: Option<MailId>,
    },
    ApplyRules {
        page: u32,
        notify: Notify,
    },
    Stop,
}

struct Writer {
    account: Account,
    id: String,
    /// Connected lazily (and again after a failed login).
    client: Option<ImapClient>,
    map: MailboxMap,
    rules: Rules,
    coord: Sender<Input>,
}

pub fn spawn(
    account: Account,
    map: MailboxMap,
    rules: Rules,
    rx: Receiver<WriteCmd>,
    coord: Sender<Input>,
) -> JoinHandle<()> {
    let id = account.id();
    std::thread::Builder::new()
        .name(format!("sola-mail-w-{id}"))
        .spawn(move || {
            let mut w = Writer {
                account,
                id,
                client: None,
                map,
                rules,
                coord,
            };
            loop {
                match rx.recv_timeout(KEEPALIVE) {
                    Ok(WriteCmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(cmd) => w.handle(cmd),
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(c) = w.client.as_mut()
                            && let Err(e) = c.noop()
                        {
                            warn!(account = w.id.as_str(), "write keepalive NOOP: {e}");
                        }
                    }
                }
            }
            info!(account = w.id.as_str(), "mail write session stopped");
        })
        .expect("spawn mail write session")
}

impl Writer {
    fn client(&mut self) -> anyhow::Result<&mut ImapClient> {
        if self.client.is_none() {
            self.client = Some(ImapClient::connect(&self.account)?);
        }
        Ok(self.client.as_mut().expect("just connected"))
    }

    fn remote(&self, canonical: &str) -> anyhow::Result<String> {
        boxes::remote(&self.map, canonical)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("this account has no {canonical} mailbox"))
    }

    fn handle(&mut self, cmd: WriteCmd) {
        match cmd {
            WriteCmd::Move { id, dest } => match self.move_one(&id, &dest) {
                Ok(dest_uid) => bridge::emit(MailEvent::Moved { id, dest, dest_uid }),
                Err(e) => bridge::emit(MailEvent::MoveFailed {
                    id,
                    message: e.to_string(),
                }),
            },
            WriteCmd::Delete(id) => {
                let result = self
                    .remote(&id.folder)
                    .and_then(|remote| self.client()?.delete_message(&remote, id.uid));
                if let Err(e) = result {
                    warn!(account = self.id.as_str(), uid = id.uid, "delete: {e}");
                }
            }
            WriteCmd::Empty { job, folder } => {
                let result = self
                    .remote(&folder)
                    .and_then(|remote| self.client()?.empty_folder(&remote))
                    .map_err(|e| e.to_string());
                let _ = self.coord.send(Input::Part {
                    job,
                    account: self.id.clone(),
                    part: Part::Emptied(result),
                });
            }
            WriteCmd::Append {
                folder,
                raw,
                draft,
                replaces,
            } => {
                let flags: &[ImapFlag<'_>] = if draft {
                    &[ImapFlag::Seen, ImapFlag::Draft]
                } else {
                    &[ImapFlag::Seen]
                };
                let result = self
                    .remote(&folder)
                    .and_then(|remote| self.client()?.append(&remote, &raw, flags));
                if result.is_ok()
                    && let Some(old) = replaces
                {
                    self.handle(WriteCmd::Delete(old));
                }
                match (result, draft) {
                    (Ok(()), true) => bridge::emit(MailEvent::DraftSaved),
                    (Ok(()), false) => {}
                    (Err(e), true) => {
                        warn!(
                            account = self.id.as_str(),
                            folder, "saving the draft failed: {e}"
                        );
                        bridge::emit(MailEvent::Failed {
                            op: Op::SaveDraft,
                            message: e.to_string(),
                        });
                    }
                    (Err(e), false) => {
                        warn!(
                            account = self.id.as_str(),
                            folder, "saving a copy failed: {e}"
                        )
                    }
                }
            }
            WriteCmd::ApplyRules { page, notify } => {
                let moved = self.apply_move_rules(page);
                if notify == Notify::Always || moved > 0 {
                    bridge::emit(MailEvent::NewMail);
                }
            }
            WriteCmd::Stop => {}
        }
    }

    fn move_one(&mut self, id: &MailId, dest: &str) -> anyhow::Result<Option<u32>> {
        let src = self.remote(&id.folder)?;
        let dst = self.remote(dest)?;
        let dest_uid = self.client()?.move_message(&src, id.uid, &dst)?;
        Ok(dest_uid)
    }

    /// Run `move` rules over the newest `page` INBOX rows. Returns how
    /// many messages moved.
    fn apply_move_rules(&mut self, page: u32) -> u32 {
        let rules: Vec<_> = self
            .rules
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|r| move_dest(r).is_some())
            .cloned()
            .collect();
        if rules.is_empty() || page == 0 {
            return 0;
        }
        let Ok(inbox) = self.remote("INBOX") else {
            return 0;
        };
        let map = self.map.clone();
        let id = self.id.clone();
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                warn!(account = id.as_str(), "move rules: {e}");
                return 0;
            }
        };
        let rows = match client.page(&inbox, "INBOX", None, page, &HashSet::new()) {
            Ok(p) => p.rows,
            Err(e) => {
                warn!(
                    account = id.as_str(),
                    "move rules: listing INBOX failed: {e}"
                );
                return 0;
            }
        };
        let mut moved = 0;
        for msg in &rows {
            let Some((rule, dest)) = rules.iter().find_map(|r| {
                rule_matches(r, &msg.from, &msg.subject, &msg.to)
                    .then(|| (r, move_dest(r).unwrap_or_default()))
            }) else {
                continue;
            };
            let dest_remote = boxes::remote(&map, dest).unwrap_or(dest);
            match client.move_message(&inbox, msg.id.uid, dest_remote) {
                Ok(_) => {
                    info!(
                        account = id.as_str(),
                        uid = msg.id.uid,
                        dest = dest_remote,
                        rule = rule.name.as_str(),
                        "move rule applied"
                    );
                    moved += 1;
                }
                Err(e) => warn!(
                    account = id.as_str(),
                    uid = msg.id.uid,
                    rule = rule.name.as_str(),
                    "move rule to {dest_remote}: {e}"
                ),
            }
        }
        moved
    }
}
