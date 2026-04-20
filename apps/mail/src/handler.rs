use std::sync::Arc;

use serde_json::{json, Value};

use crate::state::MailState;

pub struct MailHandler {
    pub state: Arc<MailState>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for MailHandler {
    async fn dispatch(&self, cmd: &str, _args: &Value) -> Value {
        match cmd {
            "mail_connect"
            | "mail_test_connection"
            | "mail_list_folders"
            | "mail_list_messages"
            | "mail_search"
            | "mail_fetch_body"
            | "mail_send"
            | "mail_move"
            | "mail_mark_read"
            | "mail_empty_folder"
            | "apply_rules"
            | "open_url" => json!({ "ok": true, "todo": cmd }),
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}
