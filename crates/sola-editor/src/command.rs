//! Everything the editor can be asked to do. Keys, menus and the command
//! palette all come down to a [`Command`].

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Forward,
    Backward,
    Down,
    Up,
    LineStart,
    LineEnd,
    IndentStart,
    WordForward,
    WordBackward,
    ParagraphForward,
    ParagraphBackward,
    BufferStart,
    BufferEnd,
    PageDown,
    PageUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Move the point. With the mark set (C-space), this extends the region.
    Move(Motion),
    /// Move the point, extending the region (Shift).
    Select(Motion),
    SetMark,
    Cancel,
    SwapPointAndMark,
    SelectAll,
    Center,

    Newline,
    Tab,
    Dedent,
    DeleteBackward,
    DeleteForward,
    KillWord,
    BackwardKillWord,
    KillLine,
    Cut,
    Copy,
    Yank,
    Transpose,
    ToggleComment,
    Undo,
    Redo,

    Find,
    GotoLine,
    Palette,

    OpenFile,
    Save,
    SaveAs,
    CloseFile,
    Quit,
}

impl Command {
    /// The commands the palette lists, with their names.
    pub const NAMED: &[(Command, &str)] = &[
        (Command::Find, "Find"),
        (Command::GotoLine, "Go to Line"),
        (Command::OpenFile, "Open File"),
        (Command::Save, "Save"),
        (Command::SaveAs, "Save As"),
        (Command::CloseFile, "Close File"),
        (Command::Quit, "Quit Editor"),
        (Command::Undo, "Undo"),
        (Command::Redo, "Redo"),
        (Command::Cut, "Cut"),
        (Command::Copy, "Copy"),
        (Command::Yank, "Paste"),
        (Command::SelectAll, "Select All"),
        (Command::SetMark, "Set Mark"),
        (Command::SwapPointAndMark, "Swap Point and Mark"),
        (Command::KillLine, "Kill Line"),
        (Command::KillWord, "Kill Word"),
        (Command::BackwardKillWord, "Backward Kill Word"),
        (Command::Transpose, "Transpose Characters"),
        (Command::ToggleComment, "Toggle Comment"),
        (Command::Dedent, "Dedent"),
        (Command::Center, "Center Cursor"),
        (Command::Move(Motion::BufferStart), "Go to Beginning of File"),
        (Command::Move(Motion::BufferEnd), "Go to End of File"),
    ];

    /// Whether this changes the text (refused in a read-only file).
    pub fn edits(self) -> bool {
        matches!(
            self,
            Command::Newline
                | Command::Tab
                | Command::Dedent
                | Command::DeleteBackward
                | Command::DeleteForward
                | Command::KillWord
                | Command::BackwardKillWord
                | Command::KillLine
                | Command::Cut
                | Command::Yank
                | Command::Transpose
                | Command::ToggleComment
                | Command::Undo
                | Command::Redo
        )
    }
}
