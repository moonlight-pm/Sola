//! What a file's type means for editing: its comment and its indent.

use std::path::Path;

fn name_of(path: &Path) -> String {
    let name = path.extension().or_else(|| path.file_name());
    name.map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The line comment of the file at `path`.
pub fn comment(path: Option<&Path>) -> Option<&'static str> {
    Some(match name_of(path?).as_str() {
        "rs" | "c" | "h" | "cc" | "cpp" | "hpp" | "js" | "jsx" | "ts" | "tsx" | "go" | "java"
        | "kt" | "swift" | "zig" | "scss" | "dart" | "cs" | "thx" => "//",
        "py" | "sh" | "bash" | "zsh" | "fish" | "toml" | "yaml" | "yml" | "nix" | "rb" | "pl"
        | "conf" | "cfg" | "ini" | "mk" | "makefile" | "dockerfile" | "tf" | "r" | "env" => "#",
        "sql" | "lua" | "hs" | "elm" | "ada" => "--",
        "el" | "lisp" | "scm" | "clj" | "asm" => ";",
        "tex" | "erl" | "m" => "%",
        "vim" => "\"",
        _ => return None,
    })
}

/// Spaces per indent where the text doesn't say: 2, as Zed had it, but for
/// the languages whose own style is 4 (Thoxa among them, as in Joshua's Zed).
pub fn tab_size(path: Option<&Path>) -> usize {
    match path.map(name_of).as_deref() {
        Some("rs" | "py" | "thx" | "java" | "kt" | "swift" | "cs" | "php" | "go" | "c" | "h" | "cpp") => 4,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_indents_come_from_the_file_type() {
        assert_eq!(comment(Some(Path::new("a/b.rs"))), Some("//"));
        assert_eq!(comment(Some(Path::new("Cargo.toml"))), Some("#"));
        assert_eq!(comment(Some(Path::new("Makefile"))), Some("#"));
        assert_eq!(comment(Some(Path::new("notes.md"))), None);
        assert_eq!(comment(None), None);
        assert_eq!(tab_size(Some(Path::new("a.rs"))), 4);
        assert_eq!(tab_size(Some(Path::new("a.toml"))), 2);
    }
}
