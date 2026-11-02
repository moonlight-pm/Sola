# Editor

Editor (`sola-editor`) is Sola's text editor: Emacs keys, syntax colors from
the Sola palette, and a window per file. It is deliberately small. It has no
language server, file tree, splits, soft wrap or multiple cursors yet. On Oath
it ships in `pkg:sola-editor`.

## Open

- `edit a.rs b.rs` from any terminal, here or in Projects. It hands the files
  to the running editor (starting it if needed) and returns at once.
- `edit new.txt`: a file that doesn't exist opens empty and is made when saved.
- `edit` alone brings the editor up.
- Launcher → **Editor**
- `solactl open notes.md`, for the file types Editor claims: source and config
  files such as `rs`, `toml`, `md`, `py`, `sh`, `ts`, `yaml`, `nix`. `txt`,
  `json` and `xml` stay with the browser, which claimed them first.
- **File → Open…** (`C-x C-f`, Super+O): Sola's file panel.

Each file is its own window, a row in the bar named for the file. Opening a
file already open brings its window up.

## Keys

Emacs, as set up in Zed. `C-` is Ctrl, `M-` is Alt, `s-` is Super.

| Move | |
|---|---|
| `C-f` `C-b` `C-n` `C-p`, arrows | Character, line |
| `C-a` `C-e`, Home End | Line start, end |
| `M-m` | First non-blank |
| `M-f` `M-b`, `M-→` `M-←` | Word |
| `M-{` `M-}`, `C-↑` `C-↓` | Paragraph |
| `C-v`, Page Down | Page down, cursor centered |
| `s-v` `M-v`, Page Up | Page up, cursor centered |
| `M-<` `M->`, `C-Home` `C-End`, `C-x [` `C-x ]` | Start, end of file |
| `C-l` | Center the cursor |
| `M-g g` | Go to line (`12` or `12:5`) |

| Mark and region | |
|---|---|
| `C-space` | Set the mark: moves extend the region until `C-g` |
| `C-g`, Escape | Drop the mark, or leave a chord, find or palette |
| `C-x C-x` | Swap point and mark |
| `s-a`, `C-x h` | Select all |
| Shift + a move, drag | Select |

| Edit | |
|---|---|
| `C-d`, Delete; Backspace | Delete a character (in an indent, Backspace goes back a tab stop) |
| `M-d`; `M-Backspace` | Delete a word, forward; back |
| `C-k` | Kill to the line's end, then the newline. Runs of `C-k` add up on the clipboard |
| `C-w` | Cut the region, or the whole line |
| `M-w` `s-w` | Copy the region, or the whole line, and drop the mark |
| `C-y` | Paste the clipboard |
| `C-t` | Transpose characters |
| `s-/` `M-;` | Toggle comment |
| Tab, Shift+Tab | Indent; dedent the line or region |
| `C-/` `C-_` `C-x u` | Undo |
| Super+Shift+Z | Redo |

| Files and commands | |
|---|---|
| `C-s` `C-r` | Find. In it, `C-s` next, `C-r` previous, `C-g` close. Lower case matches either case; a capital makes it exact |
| `C-x C-f` | Open |
| `C-x C-s` | Save |
| `C-x C-w` | Save as |
| `C-x k` | Close the file |
| `C-x C-c` | Quit |
| `s-S-p` `M-x` | Command palette |

Super keys on a menu (Super+S, Super+Z, Super+F and the like) work as in every
Sola app. Super+V and Super+W are the exceptions, kept for page up and copy, so
Editor has no key for Paste or Close Window on its menus.

The bindings are one table in `crates/sola-editor/src/keymap.rs`, and aren't yet
in Settings → Keybindings.

## Changes on disk

An open file follows the disk: if something else writes it, an agent say, the
window shows the new text, keeping your place. With unsaved edits it says so on
the status line instead, and **Save** asks whether to overwrite or reload.

## Unsaved work

Closing a file with unsaved changes asks first. Quitting doesn't: unsaved text
is kept in `~/.config/sola/editor/recover/` (written every 2 seconds while there
is some) and comes back, still unsaved, with the session. Open files, and where
you were in them, come back after a desk restart.

## Limits

- A file that isn't UTF-8 opens read-only. Past 256 MB it isn't opened.
- A line past 8 KB has no syntax colors.
- Colors are worked out from the top of the file, in the background, as far as
  you have gone. Text is plain until they arrive: a moment in an ordinary file,
  over half a minute at the far end of a 300,000-line one.
- Saving replaces the file by renaming a temporary over it (it keeps its mode;
  through a symlink, the target changes).
