//! Pane header (log column titles / inspector).

use crate::dom::Elem;
use crate::markup;

pub fn head(next: &mut u32, id: &str, bind: &str, fallback: &str) -> Elem {
    let mut el = markup::node(next, &["pane-head", "inspect-head"], None, Some(id), "");
    let mut cap = markup::node(next, &["t-caption"], None, None, fallback);
    cap.data_bind = Some(bind.into());
    el.children.push(cap);
    el
}

/// Cells only — the app HTML owns the `.log-head` row so it cannot collapse.
pub fn column_labels(next: &mut u32, cols: &[(&str, &str)]) -> Vec<Elem> {
    cols.iter()
        .map(|(class, label)| markup::node(next, &["t-caption", class], None, None, label))
        .collect()
}

/// Log column titles (Time / Topic / Source / Payload).
pub fn columns(next: &mut u32, id: &str, cols: &[(&str, &str)]) -> Elem {
    let mut el = markup::node(next, &["pane-head", "log-head"], None, Some(id), "");
    el.children = column_labels(next, cols);
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_emit_log_head() {
        let mut n = 1u32;
        let el = columns(
            &mut n,
            "log-head",
            &[
                ("col-time", "Time"),
                ("col-topic", "Topic"),
                ("col-source", "Source"),
                ("col-payload", "Payload"),
            ],
        );
        assert!(el.has_class("log-head"));
        assert_eq!(el.data_id.as_deref(), Some("log-head"));
        let labels: Vec<_> = el.children.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(labels, ["Time", "Topic", "Source", "Payload"]);
        let cells = column_labels(
            &mut n,
            &[
                ("col-time", "Time"),
                ("col-topic", "Topic"),
                ("col-source", "Source"),
                ("col-payload", "Payload"),
            ],
        );
        assert_eq!(cells.len(), 4);
        assert!(cells[0].has_class("col-time"));
    }
}
