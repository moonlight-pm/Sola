//! SMTP submit on its own thread (a slow server never stalls reading),
//! then the Sent copy and the replaced draft.

use std::sync::mpsc::Sender;

use super::cmds::{MailEvent, Op};
use super::write::WriteCmd;
use crate::bridge;
use crate::protocol::sender::{self, Outgoing};
use crate::protocol::{Account, MailId};

pub struct SendPlan {
    pub smtp: Account,
    /// Write session that keeps the Sent copy (`None`: the SMTP server
    /// files it itself, or no account has a Sent box).
    pub sent: Option<Sender<WriteCmd>>,
    /// Draft to delete once the message is out.
    pub replaced: Option<(Sender<WriteCmd>, MailId)>,
}

pub fn spawn(plan: SendPlan, msg: Outgoing) {
    let started = std::thread::Builder::new()
        .name("sola-mail-send".into())
        .spawn(move || match sender::send_mail(&plan.smtp, &msg) {
            Ok(raw) => {
                if let Some(w) = &plan.sent {
                    let _ = w.send(WriteCmd::Append {
                        folder: "Sent".into(),
                        raw,
                        draft: false,
                        replaces: None,
                    });
                }
                if let Some((w, id)) = plan.replaced {
                    let _ = w.send(WriteCmd::Delete(id));
                }
                bridge::emit(MailEvent::Sent);
            }
            Err(e) => {
                tracing::warn!(
                    from = msg.from.as_str(),
                    smtp = plan.smtp.smtp_host.as_str(),
                    "send failed: {e}"
                );
                bridge::emit(MailEvent::Failed {
                    op: Op::Send,
                    message: e.to_string(),
                });
            }
        });
    if let Err(e) = started {
        bridge::emit(MailEvent::Failed {
            op: Op::Send,
            message: format!("couldn't start sending: {e}"),
        });
    }
}
