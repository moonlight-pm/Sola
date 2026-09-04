//! Typed commands and events between UI and mail worker.

use sola_bus::topics::MailConfig;

use crate::protocol::{Folder, MailAttachment, MessageBody, MessageSummary};

#[derive(Debug, Clone)]
pub enum MailCmd {
    /// Push latest bus config; reconnects if already connected or credentials changed.
    Reconfigure(MailConfig),
    ListFolders,
    ListMessages {
        folder: String,
        offset: u32,
        limit: u32,
    },
    Search {
        query: String,
    },
    FetchBody {
        folder: String,
        uid: u32,
    },
    MarkRead {
        folder: String,
        uid: u32,
    },
    Move {
        folder: String,
        uid: u32,
        dest: String,
    },
    EmptyFolder {
        folder: String,
    },
    Send {
        from: String,
        to: String,
        cc: String,
        subject: String,
        body: String,
        in_reply_to: Option<String>,
        attachments: Vec<MailAttachment>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum MailEvent {
    Connected {
        folders: Vec<Folder>,
        smart_counts: Vec<Folder>,
        from_addresses: Vec<String>,
        rules: Vec<sola_bus::topics::MailRule>,
    },
    Folders {
        folders: Vec<Folder>,
        smart_counts: Vec<Folder>,
    },
    Messages {
        folder: String,
        messages: Vec<MessageSummary>,
        total: u32,
        offset: u32,
    },
    SearchResults {
        messages: Vec<MessageSummary>,
        total: u32,
    },
    Body(MessageBody),
    Sent,
    Moved {
        uid: u32,
        /// UID in the destination mailbox (IMAP UIDs are per-folder).
        dest_uid: Option<u32>,
    },
    MoveFailed {
        uid: u32,
        message: String,
    },
    Emptied {
        folder: String,
    },
    /// IDLE saw remaining new mail after move-rules.
    NewMail,
    Error {
        context: String,
        message: String,
    },
    /// Config present but incomplete — UI shows settings prompt.
    NotConfigured,
}

/// Collapse a burst of UI commands so rapid deletes are not stuck
/// behind body fetches / duplicate folder refreshes.
pub(crate) fn compact_cmds(cmds: Vec<MailCmd>) -> Vec<MailCmd> {
    if cmds.is_empty() {
        return cmds;
    }
    if let Some(i) = cmds.iter().position(|c| matches!(c, MailCmd::Shutdown)) {
        let mut kept: Vec<MailCmd> = cmds.into_iter().take(i).collect();
        kept = compact_cmds_inner(kept);
        kept.push(MailCmd::Shutdown);
        return kept;
    }
    compact_cmds_inner(cmds)
}

fn compact_cmds_inner(cmds: Vec<MailCmd>) -> Vec<MailCmd> {
    use std::collections::HashSet;

    let moved: HashSet<u32> = cmds
        .iter()
        .filter_map(|c| match c {
            MailCmd::Move { uid, .. } => Some(*uid),
            _ => None,
        })
        .collect();

    let last_fetch = cmds.iter().enumerate().rev().find_map(|(i, c)| match c {
        MailCmd::FetchBody { uid, .. } if !moved.contains(uid) => Some(i),
        _ => None,
    });

    let mut saw_list_folders = false;
    let mut saw_reconfigure = false;
    let mut saw_search = false;
    let mut saw_list_zero: HashSet<String> = HashSet::new();
    let mut saw_list_page: HashSet<(String, u32)> = HashSet::new();

    let mut out = Vec::with_capacity(cmds.len());
    for (i, cmd) in cmds.into_iter().enumerate().rev() {
        match &cmd {
            MailCmd::FetchBody { uid, .. } => {
                if moved.contains(uid) || last_fetch != Some(i) {
                    continue;
                }
            }
            MailCmd::MarkRead { uid, .. } => {
                if moved.contains(uid) {
                    continue;
                }
            }
            MailCmd::ListFolders => {
                if saw_list_folders {
                    continue;
                }
                saw_list_folders = true;
            }
            MailCmd::Reconfigure(_) => {
                if saw_reconfigure {
                    continue;
                }
                saw_reconfigure = true;
            }
            MailCmd::Search { .. } => {
                if saw_search {
                    continue;
                }
                saw_search = true;
            }
            MailCmd::ListMessages { folder, offset, .. } => {
                if *offset == 0 {
                    if !saw_list_zero.insert(folder.clone()) {
                        continue;
                    }
                } else if !saw_list_page.insert((folder.clone(), *offset)) {
                    continue;
                }
            }
            MailCmd::Shutdown => continue,
            _ => {}
        }
        out.push(cmd);
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn move_cmd(uid: u32) -> MailCmd {
        MailCmd::Move {
            folder: "INBOX".into(),
            uid,
            dest: "Trash".into(),
        }
    }

    fn fetch(uid: u32) -> MailCmd {
        MailCmd::FetchBody {
            folder: "INBOX".into(),
            uid,
        }
    }

    fn mark(uid: u32) -> MailCmd {
        MailCmd::MarkRead {
            folder: "INBOX".into(),
            uid,
        }
    }

    fn list(offset: u32, limit: u32) -> MailCmd {
        MailCmd::ListMessages {
            folder: "INBOX".into(),
            offset,
            limit,
        }
    }

    fn uids_of(cmds: &[MailCmd]) -> Vec<(u32, &'static str)> {
        cmds.iter()
            .filter_map(|c| match c {
                MailCmd::Move { uid, .. } => Some((*uid, "move")),
                MailCmd::FetchBody { uid, .. } => Some((*uid, "fetch")),
                MailCmd::MarkRead { uid, .. } => Some((*uid, "mark")),
                MailCmd::ListFolders => Some((0, "folders")),
                MailCmd::ListMessages { offset, .. } => Some((*offset, "list")),
                MailCmd::Shutdown => Some((0, "shutdown")),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn drops_fetch_and_mark_for_moved_uids() {
        let out = compact_cmds(vec![
            move_cmd(10),
            fetch(11),
            mark(11),
            move_cmd(11),
            fetch(12),
            mark(12),
        ]);
        assert_eq!(
            uids_of(&out),
            vec![(10, "move"), (11, "move"), (12, "fetch"), (12, "mark")]
        );
    }

    #[test]
    fn keeps_only_last_fetch_body() {
        let out = compact_cmds(vec![fetch(1), fetch(2), fetch(3), mark(1), mark(3)]);
        assert_eq!(uids_of(&out), vec![(3, "fetch"), (1, "mark"), (3, "mark")]);
    }

    #[test]
    fn coalesces_folder_list_and_first_page() {
        let out = compact_cmds(vec![
            MailCmd::ListFolders,
            list(0, 50),
            MailCmd::ListFolders,
            list(0, 150),
            list(150, 50),
        ]);
        assert_eq!(
            uids_of(&out),
            vec![(0, "folders"), (0, "list"), (150, "list")]
        );
        match &out[1] {
            MailCmd::ListMessages { limit, .. } => assert_eq!(*limit, 150),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn shutdown_stays_last_after_pending_moves() {
        let out = compact_cmds(vec![move_cmd(1), fetch(2), MailCmd::Shutdown, fetch(3)]);
        assert_eq!(
            uids_of(&out),
            vec![(1, "move"), (2, "fetch"), (0, "shutdown")]
        );
    }
}
