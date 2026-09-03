//! MIME attachments: parse received parts, guess send types, save/open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mail_parser::{Message, MessagePart, MimeHeaders};

use super::types::MailAttachment;

/// User-facing files from a parsed RFC822 message.
///
/// Nested `message/rfc822` parts flatten to their files; a wrapper with no
/// nested files is kept as an `.eml`.
pub fn collect_attachments(parsed: &Message<'_>) -> Vec<MailAttachment> {
    let mut out = Vec::new();
    collect_into(parsed, &mut out);
    out
}

fn collect_into(parsed: &Message<'_>, out: &mut Vec<MailAttachment>) {
    for part in parsed.attachments() {
        if part.is_multipart() {
            continue;
        }
        if part.is_message() {
            if let Some(nested) = part.message() {
                let before = out.len();
                collect_into(nested, out);
                if out.len() > before {
                    continue;
                }
            }
        }
        push_part(part, out);
    }
}

fn push_part(part: &MessagePart<'_>, out: &mut Vec<MailAttachment>) {
    let mime = part_mime(part);
    let filename = part
        .attachment_name()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback_name(&mime, out.len()));
    let bytes: Arc<[u8]> = Arc::from(part.contents().to_vec());
    let size = bytes.len() as u64;
    out.push(MailAttachment {
        filename: sanitize_filename(&filename),
        mime,
        size,
        bytes,
    });
}

fn part_mime(part: &MessagePart<'_>) -> String {
    match part.content_type() {
        Some(ct) => match &ct.c_subtype {
            Some(sub) => format!("{}/{}", ct.c_type, sub),
            None => ct.c_type.to_string(),
        },
        None => "application/octet-stream".into(),
    }
}

fn fallback_name(mime: &str, index: usize) -> String {
    format!("attachment-{}{}", index + 1, ext_from_mime(mime))
}

fn ext_from_mime(mime: &str) -> &'static str {
    let mime = mime.to_ascii_lowercase();
    match mime.as_str() {
        "application/pdf" => ".pdf",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "text/plain" => ".txt",
        "text/html" => ".html",
        "text/calendar" => ".ics",
        "message/rfc822" => ".eml",
        "application/zip" => ".zip",
        _ => "",
    }
}

/// Safe leaf name for save / temp (no path, no `..`).
pub fn sanitize_filename(name: &str) -> String {
    let leaf = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    let leaf = leaf.trim().trim_start_matches('.');
    if leaf.is_empty() || leaf == ".." {
        "attachment".into()
    } else {
        leaf.to_string()
    }
}

/// Content-Type for an outgoing file name.
pub fn mime_from_filename(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "txt" | "rs" | "toml" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "ics" => "text/calendar",
        "eml" => "message/rfc822",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
    .into()
}

/// 400 B / 2 KB / 1.5 MB — same steps as the kit file picker.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = bytes as f64;
    if n < KB {
        format!("{bytes} B")
    } else if n < MB {
        format!("{:.0} KB", n / KB)
    } else if n < GB {
        format!("{:.1} MB", n / MB)
    } else {
        format!("{:.1} GB", n / GB)
    }
}

/// Default save location for received files.
pub fn downloads_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Write `bytes` to a unique temp path so Open can hand off to paint/browser.
pub fn write_open_temp(filename: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("sola-mail");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create temp dir: {e}"))?;
    let name = sanitize_filename(filename);
    let unique = format!(
        "{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let path = dir.join(unique);
    std::fs::write(&path, bytes).map_err(|e| format!("Can't write {}: {e}", path.display()))?;
    Ok(path)
}

/// Open a local file the same way `solactl open` does (paint for images, browser else).
pub fn open_path(path: &Path) {
    let s = path.to_string_lossy();
    if sola_core::open_image::looks_like_image(&s) {
        sola_core::open_image_logged(&s);
    } else {
        sola_core::open_url_logged(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::MessageParser;

    fn parse(raw: &[u8]) -> mail_parser::Message<'static> {
        MessageParser::default()
            .parse(raw)
            .expect("parse")
            .into_owned()
    }

    #[test]
    fn mixed_pdf_is_collected() {
        let raw = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: invoice\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
Hello\r\n\
--b\r\n\
Content-Type: application/pdf; name=\"invoice.pdf\"\r\n\
Content-Disposition: attachment; filename=\"invoice.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
AQIDBA==\r\n\
--b--\r\n";
        let parsed = parse(raw);
        let atts = collect_attachments(&parsed);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].filename, "invoice.pdf");
        assert_eq!(atts[0].mime, "application/pdf");
        assert_eq!(&*atts[0].bytes, &[1, 2, 3, 4]);
    }

    #[test]
    fn nested_rfc822_flattens_inner_file() {
        let raw = br#"From: Art <art@example.com>
To: jane@example.com
Subject: mixed
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary="festivus";

--festivus
Content-Type: text/plain

hello
--festivus
Content-Type: message/rfc822
Content-Disposition: inline; filename="note.eml"

From: Cosmo <kramer@example.com>
Subject: inner
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary="giddyup";

--giddyup
Content-Type: text/plain

inner body
--giddyup
Content-Type: image/gif; name="tables.gif"
Content-Disposition: attachment; filename="tables.gif"
Content-Transfer-Encoding: base64

R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7
--giddyup--
--festivus--
"#;
        let parsed = parse(raw);
        let atts = collect_attachments(&parsed);
        assert_eq!(atts.len(), 1, "{atts:?}");
        assert_eq!(atts[0].filename, "tables.gif");
        assert_eq!(atts[0].mime, "image/gif");
    }

    #[test]
    fn sanitize_strips_paths_and_dots() {
        assert_eq!(sanitize_filename("/tmp/../secret.pdf"), "secret.pdf");
        assert_eq!(sanitize_filename(".."), "attachment");
        assert_eq!(sanitize_filename("a/b\\c.txt"), "c.txt");
        assert_eq!(sanitize_filename(""), "attachment");
    }

    #[test]
    fn mime_from_common_names() {
        assert_eq!(mime_from_filename("a.PDF"), "application/pdf");
        assert_eq!(mime_from_filename("x.png"), "image/png");
        assert_eq!(mime_from_filename("noext"), "application/octet-stream");
    }

    #[test]
    fn human_size_steps() {
        assert_eq!(human_size(400), "400 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(1_572_864), "1.5 MB");
    }
}
