//! Per-account read session: lists, counts, search, bodies, flags.
//! Never waits on a MOVE (that is the write session's job).

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;

use tracing::warn;

use super::cmds::{Flag, MailEvent, Op};
use super::merge::AccountCounts;
use super::{Input, KEEPALIVE, Part, Rules};
use crate::bridge;
use crate::protocol::boxes::{self, CANONICAL, MailboxMap};
use crate::protocol::imap::Rows;
use crate::protocol::rules::is_smart;
use crate::protocol::{Folder, FolderRef, ImapClient, MailId};

pub enum ReadReq {
    List {
        job: u64,
        folder: FolderRef,
        before: Option<u32>,
        limit: u32,
    },
    Counts {
        job: u64,
    },
    Search {
        job: u64,
        query: String,
        folders: Vec<String>,
    },
    Body(MailId),
    Flag {
        id: MailId,
        flag: Flag,
        on: bool,
    },
    Stop,
}

struct Reader {
    client: ImapClient,
    account: String,
    map: MailboxMap,
    rules: Rules,
    coord: Sender<Input>,
}

pub fn spawn(
    client: ImapClient,
    account: String,
    map: MailboxMap,
    rules: Rules,
    rx: Receiver<ReadReq>,
    coord: Sender<Input>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("sola-mail-r-{account}"))
        .spawn(move || {
            let mut r = Reader {
                client,
                account,
                map,
                rules,
                coord,
            };
            loop {
                let first = match rx.recv_timeout(KEEPALIVE) {
                    Ok(req) => req,
                    Err(RecvTimeoutError::Timeout) => {
                        if let Err(e) = r.client.noop() {
                            warn!(account = r.account.as_str(), "keepalive NOOP: {e}");
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                for req in prioritize(batch) {
                    if matches!(req, ReadReq::Stop) {
                        return;
                    }
                    r.handle(req);
                }
            }
        })
        .expect("spawn mail read session")
}

/// Bodies and flags before lists, so the selected letter is never stuck
/// behind a refresh. Only the newest body request survives a burst
/// (rapid arrow-key / delete-and-advance).
fn prioritize(batch: Vec<ReadReq>) -> Vec<ReadReq> {
    if batch.iter().any(|r| matches!(r, ReadReq::Stop)) {
        return vec![ReadReq::Stop];
    }
    let last_body = batch.iter().rposition(|r| matches!(r, ReadReq::Body(_)));
    let (mut urgent, mut rest) = (Vec::new(), Vec::new());
    for (i, req) in batch.into_iter().enumerate() {
        match req {
            ReadReq::Body(_) if Some(i) != last_body => {}
            ReadReq::Body(_) | ReadReq::Flag { .. } => urgent.push(req),
            _ => rest.push(req),
        }
    }
    urgent.extend(rest);
    urgent
}

impl Reader {
    fn handle(&mut self, req: ReadReq) {
        match req {
            ReadReq::List {
                job,
                folder,
                before,
                limit,
            } => {
                let page = self.list(&folder, before, limit).map_err(|e| e.to_string());
                self.reply(job, Part::Rows(page));
            }
            ReadReq::Counts { job } => {
                let counts = self.counts().map_err(|e| e.to_string());
                self.reply(job, Part::Counts(counts));
            }
            ReadReq::Search {
                job,
                query,
                folders,
            } => {
                let mut rows = Vec::new();
                let mut capped = false;
                for canonical in &folders {
                    let Some(remote) = boxes::remote(&self.map, canonical) else {
                        continue;
                    };
                    match self.client.search(remote, canonical, &query) {
                        Ok(found) => {
                            capped |= found.len() >= crate::protocol::imap::SEARCH_CAP;
                            rows.extend(found);
                        }
                        Err(e) => warn!(account = self.account.as_str(), remote, "search: {e}"),
                    }
                }
                self.reply(job, Part::Search { rows, capped });
            }
            ReadReq::Body(id) => {
                let result = match boxes::remote(&self.map, &id.folder) {
                    Some(remote) => self.client.fetch_body(remote, &id),
                    None => Err(anyhow::anyhow!("this account has no {} mailbox", id.folder)),
                };
                match result {
                    Ok(body) => bridge::emit(MailEvent::Body(Box::new(body))),
                    Err(e) => bridge::emit(MailEvent::Failed {
                        op: Op::Open(id),
                        message: e.to_string(),
                    }),
                }
            }
            ReadReq::Flag { id, flag, on } => {
                let result = match boxes::remote(&self.map, &id.folder) {
                    Some(remote) => self.client.set_flag(remote, id.uid, flag.imap(), on),
                    None => Err(anyhow::anyhow!("this account has no {} mailbox", id.folder)),
                };
                if let Err(e) = result {
                    bridge::emit(MailEvent::Failed {
                        op: Op::Flag(id),
                        message: e.to_string(),
                    });
                }
            }
            ReadReq::Stop => {}
        }
    }

    fn list(
        &mut self,
        folder: &FolderRef,
        before: Option<u32>,
        limit: u32,
    ) -> anyhow::Result<Rows> {
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner()).clone();
        match folder {
            FolderRef::Smart(name) => match rules.iter().find(|r| is_smart(r) && r.name == *name) {
                Some(rule) => self.client.smart_page(rule, before, limit),
                None => Ok(empty_page()),
            },
            FolderRef::Mailbox(name) => {
                let Some(remote) = boxes::remote(&self.map, name) else {
                    return Ok(empty_page());
                };
                // Smart-mailbox rows leave INBOX (they show under their rule).
                let exclude = if name == "INBOX" && rules.iter().any(is_smart) {
                    self.client.smart_uids(&rules).unwrap_or_else(|e| {
                        warn!(account = self.account.as_str(), "smart UIDs: {e}");
                        HashSet::new()
                    })
                } else {
                    HashSet::new()
                };
                self.client.page(remote, name, before, limit, &exclude)
            }
        }
    }

    fn counts(&mut self) -> anyhow::Result<AccountCounts> {
        let mut out = Vec::new();
        for canonical in CANONICAL {
            let Some(remote) = boxes::remote(&self.map, canonical) else {
                continue;
            };
            match self.client.status(remote) {
                Ok((unread, total)) => out.push(Folder {
                    name: (*canonical).to_string(),
                    unread,
                    total,
                }),
                Err(e) => warn!(account = self.account.as_str(), remote, "STATUS: {e}"),
            }
        }
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner()).clone();
        let smart = self.client.smart_counts(&rules)?;
        Ok(AccountCounts { boxes: out, smart })
    }

    fn reply(&self, job: u64, part: Part) {
        let _ = self.coord.send(Input::Part {
            job,
            account: self.account.clone(),
            part,
        });
    }
}

fn empty_page() -> Rows {
    Rows {
        rows: Vec::new(),
        total: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(uid: u32) -> ReadReq {
        ReadReq::Body(MailId::new("a@x", "INBOX", uid))
    }

    fn kinds(reqs: &[ReadReq]) -> Vec<String> {
        reqs.iter()
            .map(|r| match r {
                ReadReq::Body(id) => format!("body{}", id.uid),
                ReadReq::Flag { id, .. } => format!("flag{}", id.uid),
                ReadReq::List { job, .. } => format!("list{job}"),
                ReadReq::Counts { job } => format!("counts{job}"),
                ReadReq::Search { job, .. } => format!("search{job}"),
                ReadReq::Stop => "stop".into(),
            })
            .collect()
    }

    #[test]
    fn newest_body_jumps_the_queue() {
        let out = prioritize(vec![
            ReadReq::Counts { job: 1 },
            body(1),
            ReadReq::Flag {
                id: MailId::new("a@x", "INBOX", 1),
                flag: Flag::Seen,
                on: true,
            },
            body(2),
            ReadReq::Counts { job: 2 },
        ]);
        assert_eq!(kinds(&out), vec!["flag1", "body2", "counts1", "counts2"]);
    }

    #[test]
    fn stop_wins() {
        assert_eq!(
            kinds(&prioritize(vec![body(1), ReadReq::Stop])),
            vec!["stop"]
        );
    }
}
