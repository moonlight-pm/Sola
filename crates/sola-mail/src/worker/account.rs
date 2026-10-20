//! One connected IMAP account: read session, write session, IDLE.

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use tracing::{info, warn};

use super::cmds::MailEvent;
use super::read::{self, ReadReq};
use super::write::{self, Notify, WriteCmd};
use super::{Input, Rules};
use crate::bridge;
use crate::protocol::boxes::{self, MailboxMap};
use crate::protocol::rules::move_dest;
use crate::protocol::{Account, IdleChange, IdleHandle, ImapClient, start_idle};

/// Newest INBOX envelopes the move rules scan on connect / IDLE
/// reconnect. Older matches wait for a later sweep.
const APPLY_ON_CONNECT: u32 = 500;
/// At least this many newest rows when rules run on new mail.
const APPLY_ON_ARRIVAL: u32 = 20;

pub struct AccountHandle {
    pub account: Account,
    pub map: MailboxMap,
    read: Sender<ReadReq>,
    write: Sender<WriteCmd>,
    threads: Vec<JoinHandle<()>>,
    idle: Option<IdleHandle>,
}

impl AccountHandle {
    /// Log in, map mailboxes (creating Gmail's Archive label), then start
    /// the read / write / IDLE threads. Blocking: run off the coordinator.
    pub fn connect(account: Account, rules: Rules, coord: Sender<Input>) -> Result<Self, String> {
        let id = account.id();
        let mut client = ImapClient::connect(&account).map_err(|e| e.to_string())?;
        let map = map_mailboxes(&mut client, &account).map_err(|e| e.to_string())?;
        info!(account = id.as_str(), boxes = map.len(), "IMAP mailbox map");

        let (read_tx, read_rx) = mpsc::channel();
        let (write_tx, write_rx) = mpsc::channel();
        let threads = vec![
            read::spawn(
                client,
                id.clone(),
                map.clone(),
                rules.clone(),
                read_rx,
                coord.clone(),
            ),
            write::spawn(account.clone(), map.clone(), rules.clone(), write_rx, coord),
        ];
        let _ = write_tx.send(WriteCmd::ApplyRules {
            page: APPLY_ON_CONNECT,
            notify: Notify::IfMoved,
        });
        let idle_write = write_tx.clone();
        let idle = start_idle(account.clone(), move |change| {
            let has_rules = rules
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|r| move_dest(r).is_some());
            let sweep = |page| {
                let _ = idle_write.send(WriteCmd::ApplyRules {
                    page,
                    notify: Notify::Always,
                });
            };
            match change {
                IdleChange::Arrived { new_count } if has_rules => {
                    sweep(new_count.max(APPLY_ON_ARRIVAL))
                }
                IdleChange::Reconnected if has_rules => sweep(APPLY_ON_CONNECT),
                IdleChange::Arrived { .. } | IdleChange::Reconnected | IdleChange::Removed => {
                    bridge::emit(MailEvent::NewMail)
                }
            }
        });
        Ok(Self {
            account,
            map,
            read: read_tx,
            write: write_tx,
            threads,
            idle: Some(idle),
        })
    }

    pub fn id(&self) -> String {
        self.account.id()
    }

    pub fn has(&self, canonical: &str) -> bool {
        boxes::remote(&self.map, canonical).is_some()
    }

    pub fn read(&self, req: ReadReq) -> bool {
        self.read.send(req).is_ok()
    }

    pub fn write(&self, cmd: WriteCmd) -> bool {
        self.write.send(cmd).is_ok()
    }

    pub fn writer(&self) -> Sender<WriteCmd> {
        self.write.clone()
    }

    /// Stop every thread and wait. Prompt: IDLE shuts its socket; the
    /// sessions finish only the command they are on.
    pub fn shutdown(mut self) {
        let _ = self.read.send(ReadReq::Stop);
        let _ = self.write.send(WriteCmd::Stop);
        if let Some(idle) = self.idle.take() {
            idle.stop();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn map_mailboxes(client: &mut ImapClient, account: &Account) -> anyhow::Result<MailboxMap> {
    let listed = client.list_mailbox_names()?;
    let map = boxes::map_mailboxes(&listed);
    if !boxes::should_create_archive(&map, &account.imap_host, &listed) {
        return Ok(map);
    }
    match client.create_mailbox("Archive") {
        Ok(()) => {
            info!(account = account.id().as_str(), "created Archive mailbox");
            Ok(boxes::map_mailboxes(&client.list_mailbox_names()?))
        }
        Err(e) => {
            warn!(account = account.id().as_str(), "CREATE Archive failed: {e}");
            Ok(map)
        }
    }
}
