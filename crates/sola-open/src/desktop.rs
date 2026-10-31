//! Backward compatibility: `.desktop` handlers for the schemes no Sola app
//! claims (Claude Code's `claude-cli://`, Steam's `steam://`), the way
//! xdg-open finds them — `~/.config/mimeapps.list` first, then any `.desktop`
//! file whose `MimeType` lists `x-scheme-handler/<scheme>`.

use std::path::{Path, PathBuf};

/// A `.desktop` handler: its file name, its name, and the command it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub exec: String,
    mime: Vec<String>,
}

impl Entry {
    #[cfg(test)]
    pub(crate) fn new(id: &str, exec: &str, schemes: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: id.trim_end_matches(".desktop").into(),
            exec: exec.into(),
            mime: schemes.iter().map(|s| format!("x-scheme-handler/{s}")).collect(),
        }
    }

    /// The schemes it says it opens.
    pub fn schemes(&self) -> impl Iterator<Item = &str> {
        self.mime.iter().filter_map(|m| m.strip_prefix("x-scheme-handler/"))
    }

    pub fn opens_scheme(&self, scheme: &str) -> bool {
        self.schemes().any(|s| s.eq_ignore_ascii_case(scheme))
    }
}

/// The handler for `scheme` among `dirs` (each an `applications` folder),
/// `mimeapps` (a `mimeapps.list`) naming it first.
pub fn handler_for_scheme(scheme: &str, dirs: &[PathBuf], mimeapps: &Path) -> Option<Entry> {
    handler_for_scheme_in(&entries(dirs), scheme, mimeapps)
}

/// [`handler_for_scheme`] among `entries` already read.
pub fn handler_for_scheme_in(entries: &[Entry], scheme: &str, mimeapps: &Path) -> Option<Entry> {
    let mime = format!("x-scheme-handler/{scheme}");
    let chosen = default_ids(mimeapps, &mime);
    chosen
        .iter()
        .find_map(|id| entries.iter().find(|e| &e.id == id))
        .or_else(|| entries.iter().find(|e| e.opens_scheme(scheme)))
        .cloned()
}

/// `$XDG_DATA_HOME/applications`, then each `$XDG_DATA_DIRS` entry's.
pub fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = absolute_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::iter::once(data_home)
        .chain(data_dirs.split(':').filter(|d| d.starts_with('/')).map(PathBuf::from))
        .map(|d| d.join("applications"))
        .collect()
}

/// `$XDG_CONFIG_HOME/mimeapps.list` (`~/.config/mimeapps.list`).
pub fn mimeapps_list() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    absolute_env("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home.join(".config"))
        .join("mimeapps.list")
}

/// The argv for `exec` opening `target`: field codes filled in (`%u` `%U`
/// `%f` `%F` the target; `%i` `%c` `%k` dropped; `%%` a `%`), double quotes
/// grouping. With no field code the target goes last.
pub fn argv(exec: &str, target: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut placed = false;
    for word in words(exec) {
        match word.as_str() {
            "%u" | "%U" | "%f" | "%F" => {
                out.push(target.to_string());
                placed = true;
            }
            "%i" | "%c" | "%k" => {}
            _ => {
                let mut w = String::new();
                let mut chars = word.chars().peekable();
                while let Some(c) = chars.next() {
                    if c != '%' {
                        w.push(c);
                        continue;
                    }
                    match chars.next() {
                        Some('u' | 'U' | 'f' | 'F') => {
                            w.push_str(target);
                            placed = true;
                        }
                        Some('%') => w.push('%'),
                        Some(_) | None => {}
                    }
                }
                out.push(w);
            }
        }
    }
    if !placed {
        out.push(target.to_string());
    }
    out
}

fn words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Every application `.desktop` file in `dirs`, a file name once (the first
/// folder wins), by file name within a folder.
pub fn entries(dirs: &[PathBuf]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for dir in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
            .collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            if out.iter().any(|e| e.id == id) {
                continue;
            }
            if let Some(entry) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&id, &t)) {
                out.push(entry);
            }
        }
    }
    out
}

/// The `[Desktop Entry]` group of a file: an application with an `Exec`,
/// not hidden.
fn parse(id: &str, text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let (mut exec, mut mime, mut app, mut hidden) = (None, Vec::new(), false, false);
    let mut name = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Exec" => exec = Some(v.trim().to_string()),
            "Name" => name = Some(v.trim().to_string()),
            "MimeType" => mime = v.split(';').map(str::trim).filter(|m| !m.is_empty()).map(String::from).collect(),
            "Type" => app = v.trim() == "Application",
            "Hidden" => hidden = v.trim() == "true",
            _ => {}
        }
    }
    (app && !hidden).then_some(())?;
    let name = name.unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());
    Some(Entry { id: id.into(), name, exec: exec?, mime })
}

/// The `.desktop` ids `mimeapps.list` names for `mime` under
/// `[Default Applications]`.
fn default_ids(mimeapps: &Path, mime: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(mimeapps) else {
        return Vec::new();
    };
    let mut in_defaults = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
            continue;
        }
        if let Some((k, v)) = line.split_once('=').filter(|_| in_defaults) {
            if k.trim() == mime {
                return v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            }
        }
    }
    Vec::new()
}

fn absolute_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = "[Desktop Entry]\nName=Claude Code URL Handler\nExec=\"/oath/store/pkg/claude-code/x/claude\" --handle-uri %u\nType=Application\nNoDisplay=true\nMimeType=x-scheme-handler/claude-cli;\n";

    #[test]
    fn exec_fills_its_field_codes() {
        let entry = parse("c.desktop", CLAUDE).unwrap();
        assert_eq!(
            argv(&entry.exec, "claude-cli://open"),
            ["/oath/store/pkg/claude-code/x/claude", "--handle-uri", "claude-cli://open"]
        );
        assert_eq!(argv("steam %U %i", "steam://run/1"), ["steam", "steam://run/1"]);
        assert_eq!(argv("viewer --url=%u", "x:y"), ["viewer", "--url=x:y"]);
        assert_eq!(argv("plain", "t"), ["plain", "t"]);
        assert_eq!(argv("\"/a b/c\" \"\"", "t"), ["/a b/c", "", "t"]);
    }

    #[test]
    fn a_scheme_finds_its_handler_and_mimeapps_wins() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("claude.desktop"), CLAUDE).unwrap();
        std::fs::write(
            apps.join("other.desktop"),
            "[Desktop Entry]\nType=Application\nExec=other %u\nMimeType=x-scheme-handler/claude-cli;\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nHidden=true\nExec=h %u\nMimeType=x-scheme-handler/steam;\n",
        )
        .unwrap();
        let none = dir.path().join("none.list");
        let dirs = [apps.clone()];
        assert_eq!(handler_for_scheme("claude-cli", &dirs, &none).unwrap().id, "claude.desktop");
        assert!(handler_for_scheme("steam", &dirs, &none).is_none());
        let list = dir.path().join("mimeapps.list");
        std::fs::write(&list, "[Default Applications]\nx-scheme-handler/claude-cli=other.desktop;\n").unwrap();
        assert_eq!(handler_for_scheme("claude-cli", &dirs, &list).unwrap().id, "other.desktop");
    }
}
