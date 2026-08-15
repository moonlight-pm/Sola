//! Shell-initiated capture via `compositor.screenshot` (not the bus).

use std::path::PathBuf;
use std::time::Duration;

use sola_call::methods::OWNER_COMPOSITOR;

use crate::app::Msg;

pub fn full() -> iced::Task<Msg> {
    invoke(serde_json::json!({}))
}

pub fn window(app_id: String, title: Option<String>) -> iced::Task<Msg> {
    let mut params = serde_json::json!({ "app": app_id });
    if let Some(t) = title {
        params["window"] = serde_json::Value::String(t);
    }
    invoke(params)
}

pub fn region(x: i32, y: i32, width: i32, height: i32) -> iced::Task<Msg> {
    invoke(serde_json::json!({
        "x": x,
        "y": y,
        "width": width,
        "height": height,
    }))
}

fn invoke(params: serde_json::Value) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let r = sola_call::invoke(
                    OWNER_COMPOSITOR,
                    "screenshot",
                    params,
                    Duration::from_secs(20),
                );
                let _ = tx.send(r);
            });
            match rx.await {
                Ok(Ok(v)) => v
                    .get("path")
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| "screenshot: no path in reply".into()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err("screenshot: worker dropped".into()),
            }
        },
        Msg::ScreenshotDone,
    )
}
