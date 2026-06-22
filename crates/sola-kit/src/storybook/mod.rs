//! Storybook app — global theme header + sidebar nav + showcase pane.
//!
//! Each kit-shipped component (badge, button, card, divider, field,
//! icon, number_input, popover, readable, sidebar, split, text, theme
//! atoms, toolbar) gets a page in [`pages`]; `swatch` and `text_input`
//! are folded into the Theme and Field pages respectively. The shell
//! holds the selected page in state and re-renders on `Select(Page)`.
//!
//! The storybook owns a list of [`ThemePreset`]s — index 0 is always
//! the immutable "Default" preset whose values come from
//! `Atoms::default()` / `FontSelection::default()`. Subsequent entries
//! are user-created copies (via "New Theme"). Edits to atoms or fonts
//! mutate the currently active preset; the Default is read-only and
//! atom edits / `SetFont` are no-ops while it's selected. Every edit,
//! preset switch, and delete re-emits `Topic::Theme` so other kit apps
//! pick up the change on their next render.

use std::sync::Arc;

use iced::widget::{button, column, container, pick_list, row, scrollable, text};
use iced::{Element, Length, Padding, Subscription};

use sola_bus::topics::{MenuActionPayload, Topic};
use sola_kit::components::{
    ColorPicker, SidebarItem, SidebarSection, button as kit_button, sidebar,
    text_input as kit_text_input,
};
use sola_kit::theme;

pub mod pages;

/// Which showcase page is currently rendered. Lives in the storybook's
/// state so navigation is parent-controlled (the sidebar component
/// doesn't track its own selection — see its module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Theme,
    Shell,
    Text,
    Button,
    Badge,
    Card,
    Field,
    Icon,
    NumberInput,
    Readable,
    ColorPicker,
    Divider,
    Popover,
    Sidebar,
    Split,
    Toolbar,
}

impl Page {
    /// Order rendered in the sidebar. Grouped by [`Page::section`]:
    /// Theme → Layout → Components.
    pub const ALL: &'static [Page] = &[
        Page::Theme,
        Page::Shell,
        Page::Divider,
        Page::Split,
        Page::Toolbar,
        Page::Text,
        Page::Button,
        Page::Badge,
        Page::Card,
        Page::Field,
        Page::Icon,
        Page::NumberInput,
        Page::Readable,
        Page::ColorPicker,
        Page::Popover,
        Page::Sidebar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Theme => "Theme",
            Page::Shell => "Shell",
            Page::Text => "Text",
            Page::Button => "Button",
            Page::Badge => "Badge",
            Page::Card => "Card",
            Page::Field => "Field",
            Page::Icon => "Icon",
            Page::NumberInput => "NumberInput",
            Page::Readable => "Readable",
            Page::ColorPicker => "ColorPicker",
            Page::Divider => "Divider",
            Page::Popover => "Popover",
            Page::Sidebar => "Sidebar",
            Page::Split => "Split",
            Page::Toolbar => "Toolbar",
        }
    }

    /// Section bucket for the sidebar. Mirrors the original kit's
    /// Theme / Layout / Components grouping.
    pub fn section(self) -> Option<&'static str> {
        match self {
            Page::Theme | Page::Shell => Some("Theme"),
            Page::Divider | Page::Split | Page::Toolbar => Some("Layout"),
            Page::Text
            | Page::Button
            | Page::Badge
            | Page::Card
            | Page::Field
            | Page::Icon
            | Page::NumberInput
            | Page::Readable
            | Page::ColorPicker
            | Page::Popover
            | Page::Sidebar => Some("Components"),
        }
    }

    /// The palette atoms this page's component visibly uses, surfaced as
    /// an inline editor panel below its demo (see `page_view`). Curated,
    /// best-effort — tune freely. Empty for pages that carry their own
    /// editors (Theme's full grid, Shell's token editor). Authored from
    /// each component's actual `extended_palette()` slot usage.
    pub fn atoms(self) -> &'static [AtomField] {
        use AtomField::{
            Accent, Bg, BgHover, BgRaised, Border, Danger, Fg, FgMuted, Selection, Success,
            Warning,
        };
        match self {
            Page::Theme | Page::Shell => &[],
            Page::Divider => &[Border, Bg],
            Page::Split => &[Bg, BgRaised, Border],
            Page::Toolbar => &[Bg, BgRaised, BgHover, Border, Fg, FgMuted],
            Page::Text => &[Fg, FgMuted, Accent, Success, Warning, Danger],
            Page::Button => &[Accent, Danger, Bg, BgHover, Border, Fg],
            Page::Badge => &[Accent, Success, Warning, Danger, Border, FgMuted],
            Page::Card => &[Bg, BgRaised, Border, Fg, Accent],
            Page::Field => &[BgRaised, Border, Fg, FgMuted],
            Page::Icon => &[Fg, FgMuted, Accent],
            Page::NumberInput => &[Bg, Border, Fg, FgMuted, Accent],
            Page::Readable => &[Bg, BgRaised, Fg, FgMuted],
            Page::ColorPicker => &[Bg, BgRaised, Border, Fg, Accent],
            Page::Popover => &[BgRaised, Border, Fg, FgMuted],
            Page::Sidebar => &[Bg, BgHover, Selection, Fg, FgMuted, Accent],
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Select(Page),
    Toolbar(pages::toolbar::Msg),
    Field(pages::field::Msg),
    NumberInput(pages::number_input::Msg),
    ColorPicker(pages::color_picker::Msg),
    Sidebar(pages::sidebar::Msg),
    /// Bus message arriving via [`sola_kit::app::bus_subscription`].
    Bus(Arc<sola_bus::Message>),
    /// Open the inline color picker for one palette atom. No-op when
    /// Default is active (its atoms are read-only).
    EditAtom(AtomField),
    /// Open the inline color picker for one shell color knob. No-op when
    /// Default is active (read-only).
    EditShellColor(ShellColorField),
    /// Set one shell spacing knob from the Shell page's number inputs.
    /// No-op when Default is active.
    SetShellSpace(ShellSpaceField, f32),
    /// A message from the open atom color picker.
    Picker(sola_kit::components::color_picker::Message),
    /// Close the open atom picker (a click outside its popover).
    ClosePicker,
    /// Swap one font role on the active preset. No-op when Default is
    /// active.
    SetFont(FontRole, String),
    /// Switch the active theme to the preset with this name.
    SelectTheme(String),
    /// User clicked "New Theme" — show the inline name input with a
    /// suggested name.
    NewThemeStart,
    /// Inline name input changed.
    NewThemeInput(String),
    /// Commit the inline name → fork the active preset under that name
    /// and switch to it. Silently re-shows the input if the name is
    /// empty or already used.
    NewThemeCommit,
    /// Dismiss the inline name input without forking.
    NewThemeCancel,
    /// First click of the two-stage delete — arm the confirm. No-op
    /// when Default is active (it can't be deleted).
    ArmDelete,
    /// Auto-disarm the two-stage delete — fired ~2s after arming by the
    /// timeout subscription, so a forgotten armed button reverts itself.
    DisarmDelete,
    /// Remove the active preset (only allowed when it's not Default)
    /// and switch back to Default. Second click of the two-stage delete.
    DeleteActiveTheme,
    /// Reset one palette atom to its compile-time default. A live edit
    /// like any other (broadcast, marks dirty); no persist. No-op when
    /// Default is active.
    ResetAtom(AtomField),
    /// Checkpoint the active preset: persist it and re-baseline the
    /// `Revert` target. Clears the dirty flag.
    SaveTheme,
    /// Discard the working set — restore the active preset from its last
    /// checkpoint, rebroadcast, and clear dirty.
    RevertTheme,
    /// Demo placeholder for showcases whose components require an
    /// `on_press` (or similar callback) message but don't model
    /// interaction in the storybook.
    Noop,
}

/// Identifies which slot in [`theme::FontSelection`] a `SetFont` edit
/// targets. Kept here (not in `theme.rs`) because it's UI-shaped — the
/// pick_list per role wraps each `Family` into a different `SetFont(role, ..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRole {
    Ui,
    UiMedium,
    Display,
    Chrome,
    Mono,
}

/// Identifies which editable colour atom an `EditAtom`
/// targets. UI-shaped (the theme page's swatch grid and color picker
/// carry it), so it lives here rather than in `theme.rs`. The
/// get/set pair is the storybook's view onto `theme::Atoms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomField {
    Bg,
    BgRaised,
    BgHover,
    Border,
    Fg,
    FgMuted,
    Accent,
    Success,
    Warning,
    Danger,
    Selection,
}

impl AtomField {
    /// Read this atom out of an `Atoms`.
    pub fn get(self, a: &theme::Atoms) -> iced::Color {
        match self {
            AtomField::Bg => a.bg,
            AtomField::BgRaised => a.bg_raised,
            AtomField::BgHover => a.bg_hover,
            AtomField::Border => a.border,
            AtomField::Fg => a.fg,
            AtomField::FgMuted => a.fg_muted,
            AtomField::Accent => a.accent,
            AtomField::Success => a.success,
            AtomField::Warning => a.warning,
            AtomField::Danger => a.danger,
            AtomField::Selection => a.selection,
        }
    }

    /// Write this atom into an `Atoms`.
    pub fn set(self, a: &mut theme::Atoms, c: iced::Color) {
        match self {
            AtomField::Bg => a.bg = c,
            AtomField::BgRaised => a.bg_raised = c,
            AtomField::BgHover => a.bg_hover = c,
            AtomField::Border => a.border = c,
            AtomField::Fg => a.fg = c,
            AtomField::FgMuted => a.fg_muted = c,
            AtomField::Accent => a.accent = c,
            AtomField::Success => a.success = c,
            AtomField::Warning => a.warning = c,
            AtomField::Danger => a.danger = c,
            AtomField::Selection => a.selection = c,
        }
    }

    /// This atom's compile-time default colour (its `hex::*` constant),
    /// read off `Atoms::default()`. Drives the per-atom "reset to
    /// default" affordance and its visibility (shown only when the live
    /// value differs from this).
    pub fn default_color(self) -> iced::Color {
        self.get(&theme::Atoms::default())
    }
}

/// Identifies which shell color knob an `EditShellColor` targets.
/// UI-shaped (the Shell page's swatches and color picker carry it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellColorField {
    MenubarBg,
    BackdropDim,
    SwitcherBg,
    SwitcherBorder,
    SwitcherIconBg,
    SwitcherIconFg,
    SwitcherIconFgSel,
}

impl ShellColorField {
    pub fn get(self, s: &theme::ShellStyle) -> iced::Color {
        match self {
            Self::MenubarBg => s.menubar_bg,
            Self::BackdropDim => s.backdrop_dim,
            Self::SwitcherBg => s.switcher_bg,
            Self::SwitcherBorder => s.switcher_border,
            Self::SwitcherIconBg => s.switcher_icon_bg,
            Self::SwitcherIconFg => s.switcher_icon_fg,
            Self::SwitcherIconFgSel => s.switcher_icon_fg_sel,
        }
    }
    pub fn set(self, s: &mut theme::ShellStyle, c: iced::Color) {
        match self {
            Self::MenubarBg => s.menubar_bg = c,
            Self::BackdropDim => s.backdrop_dim = c,
            Self::SwitcherBg => s.switcher_bg = c,
            Self::SwitcherBorder => s.switcher_border = c,
            Self::SwitcherIconBg => s.switcher_icon_bg = c,
            Self::SwitcherIconFg => s.switcher_icon_fg = c,
            Self::SwitcherIconFgSel => s.switcher_icon_fg_sel = c,
        }
    }
}

/// Identifies which shell spacing knob a `SetShellSpace` targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSpaceField {
    SwitcherPad,
    SwitcherTilePad,
    LauncherWidth,
    LauncherPad,
}

impl ShellSpaceField {
    pub fn set(self, s: &mut theme::ShellStyle, v: f32) {
        match self {
            Self::SwitcherPad => s.switcher_pad = v,
            Self::SwitcherTilePad => s.switcher_tile_pad = v,
            Self::LauncherWidth => s.launcher_width = v,
            Self::LauncherPad => s.launcher_pad = v,
        }
    }
}

/// A named bundle of atoms + font selection + shell style. The
/// storybook keeps a `Vec<ThemePreset>` with index 0 always being the
/// immutable "Default" preset whose values match the Rust constants in
/// `sola_kit::theme` and `sola_kit::fonts`.
#[derive(Clone, Debug)]
pub struct ThemePreset {
    pub name: String,
    pub atoms: theme::Atoms,
    pub fonts: theme::FontSelection,
    pub shell: theme::ShellStyle,
}

impl ThemePreset {
    /// The preset's complete bus form — atoms + fonts + shell tokens.
    /// The single source for broadcast, persist, retract, and resync
    /// matching, so the value-equality invariant can't drift between
    /// call sites.
    fn bus_theme(&self) -> sola_core::theme::Theme {
        theme::bus_theme_with_shell(theme::bus_theme_from(&self.atoms, &self.fonts), &self.shell)
    }
}

pub struct Storybook {
    page: Page,
    toolbar: pages::toolbar::State,
    field: pages::field::State,
    number_input: pages::number_input::State,
    color_picker: pages::color_picker::State,
    sidebar: pages::sidebar::State,
    /// All known themes. `themes[0]` is always Default and can't be
    /// edited or deleted. Subsequent entries are user copies.
    themes: Vec<ThemePreset>,
    /// Index into `themes` of the currently active preset.
    active_theme: usize,
    /// `Some(buffer)` while the inline "New Theme" name input is
    /// showing; `None` when the header is in its normal layout.
    naming: Option<String>,
    /// Two-stage delete: `false` shows the restrained "Delete" outline,
    /// `true` (after one click) shows the filled "Confirm?" affordance.
    /// Disarmed by any other header action so a stale armed state can't
    /// linger across navigation.
    delete_armed: bool,
    /// Which palette atom's inline color picker is open on the Theme
    /// page, if any. Only meaningful while an editable (non-Default)
    /// preset is active.
    editing_atom: Option<AtomField>,
    /// Which shell color knob's inline picker is open on the Shell page,
    /// if any. Mutually exclusive with `editing_atom` — both pair with
    /// the single `picker`.
    editing_shell: Option<ShellColorField>,
    /// The open atom's HSV/hex picker, paired with `editing_atom`. Holds
    /// its own HSV state so hue survives value→0; `None` when no atom is
    /// being edited.
    picker: Option<ColorPicker>,
    /// Cached pick_list options for the per-role font selectors. Built
    /// once in `default()` from `INSTALLED_FAMILIES` so each render of
    /// the Theme page doesn't reallocate five `Vec<String>`s, and
    /// pre-warmed on every frame via a transparent strip in `view()`
    /// so cosmic-text has already shaped the family names by the time
    /// the user opens a dropdown.
    family_options: Vec<String>,
    /// Live iced theme — derived from the active preset's atoms, also
    /// replaceable by a `Topic::Theme` delivery from another emitter.
    theme: iced::Theme,
    /// Most recent `Topic::Theme` payload, captured from sticky-replay
    /// on connect and from later edits. We use it to resync
    /// `active_theme` whenever a `CustomTheme` upsert lands — the
    /// replay order between `Topic::Theme` and `Topic::CustomTheme` is
    /// undefined, so we re-run the match every time either side moves.
    last_live_theme: Option<sola_bus::topics::Theme>,
    /// Last *saved* snapshot of the active preset — the baseline `Revert`
    /// restores to. Edits mutate `themes[active_theme]` live (broadcast,
    /// no persist); `Save` re-snapshots here and persists, `Revert`
    /// copies this back over the active slot. Re-baselined whenever a
    /// non-dirty bus delivery resolves the active preset (startup replay).
    checkpoint: ThemePreset,
    /// `true` once the active preset has been edited since the last Save
    /// or Revert. Drives the header's `Edited •` indicator and gates the
    /// Save/Revert buttons. A bool (not value-equality) so reverting an
    /// edit by hand still reads as dirty — "touched", not "differs".
    dirty: bool,
}

impl Storybook {
    /// Name of the immutable default preset that always occupies slot 0.
    pub const DEFAULT_THEME_NAME: &'static str = "Default";

    pub fn default() -> Self {
        let default_preset = ThemePreset {
            name: Self::DEFAULT_THEME_NAME.to_string(),
            atoms: theme::Atoms::default(),
            fonts: theme::FontSelection::default(),
            shell: theme::ShellStyle::default(),
        };
        let theme = theme::build_theme(&default_preset.atoms);
        // The checkpoint starts as the active preset's snapshot; startup
        // bus replay re-baselines it once a saved preset resolves.
        let checkpoint = default_preset.clone();
        Self {
            page: Page::Theme,
            toolbar: pages::toolbar::State::default(),
            field: pages::field::State::default(),
            number_input: pages::number_input::State::default(),
            color_picker: pages::color_picker::State::default(),
            sidebar: pages::sidebar::State::default(),
            themes: vec![default_preset],
            active_theme: 0,
            naming: None,
            delete_armed: false,
            editing_atom: None,
            editing_shell: None,
            picker: None,
            // Shipped families + everything fontdb finds installed, so
            // the per-role picker can offer any system font, not just
            // what we ship.
            family_options: sola_kit::fonts::pickable_families(),
            theme,
            last_live_theme: None,
            checkpoint,
            dirty: false,
        }
    }

    pub fn title(&self) -> String {
        format!("sola-kit · {}", self.page.label())
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        use iced::event::{self, Event};
        use iced::mouse;

        let bus = sola_kit::app::bus_subscription().map(Msg::Bus);
        let mut subs = vec![bus];

        // While the delete confirm is armed, run a one-shot-ish timer
        // that disarms it ~2s after arming (the subscription only exists
        // while `delete_armed`, so it starts on arm and is torn down on
        // confirm/disarm). `every` first fires ~2s after it starts.
        if self.delete_armed {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(2))
                    .map(|_| Msg::DisarmDelete),
            );
        }

        // Sidebar dogfood: while a resize or reorder gesture is in
        // progress, listen for global cursor moves + release. Two
        // listeners mirror the terminal's pattern — one feeds the x
        // (divider width), one feeds the y (reorder drop). The update
        // arms are independent (each guards on its own gesture state), so
        // the cross-fired release messages are harmless.
        if self.sidebar.needs_cursor_subscription() {
            subs.push(event::listen_with(|ev, _, _| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::Sidebar(pages::sidebar::Msg::DividerMove(position.x)))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::Sidebar(pages::sidebar::Msg::DividerRelease))
                }
                _ => None,
            }));
            subs.push(event::listen_with(|ev, _, _| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::Sidebar(pages::sidebar::Msg::ReorderMove(position.y)))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::Sidebar(pages::sidebar::Msg::ReorderEnd))
                }
                _ => None,
            }));
        }

        Subscription::batch(subs)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Select(page) => {
                self.page = page;
                // Don't carry a half-open atom/shell picker across pages.
                self.editing_atom = None;
                self.editing_shell = None;
                self.picker = None;
            }
            Msg::Toolbar(m) => self.toolbar.update(m),
            Msg::Field(m) => self.field.update(m),
            Msg::NumberInput(m) => self.number_input.update(m),
            Msg::ColorPicker(m) => self.color_picker.update(m),
            Msg::Sidebar(m) => self.sidebar.update(m),
            Msg::Bus(message) => {
                let Some(topic) = Topic::parse(&message) else { return };
                match topic {
                    Topic::Theme(bus_theme) => {
                        // External theme delivery (sticky-replay from
                        // sola-shell on first connect, or another editor's
                        // edit). Rebuild the iced theme (pure) and
                        // install the font role table explicitly — theme_from_bus
                        // no longer does it as a side effect.
                        self.theme = theme::theme_from_bus(&bus_theme);
                        crate::fonts::install(theme::fonts_from_bus_theme(&bus_theme));
                        theme::install_selection(
                            theme::atoms_from_bus_theme(&bus_theme).selection,
                        );
                        self.last_live_theme = Some(bus_theme);
                        self.resync_active_theme();
                        // A clean storybook re-baselines its Revert target to
                        // whatever the bus resolved (startup replay); a dirty
                        // one keeps its in-progress checkpoint.
                        if !self.dirty {
                            self.checkpoint = self.active().clone();
                        }
                    }
                    Topic::CustomTheme(named) => {
                        // Persistent-topic add/update vs retract is signalled
                        // by message.sticky (true on emit, false on retract).
                        if message.sticky {
                            self.upsert_custom_theme(named);
                        } else {
                            self.remove_custom_theme(&named.name);
                        }
                        self.resync_active_theme();
                        if !self.dirty {
                            self.checkpoint = self.active().clone();
                        }
                    }
                    Topic::MenuAction(MenuActionPayload { app_id, action_id })
                        if app_id == "sola-kit" && action_id == "quit" =>
                    {
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
            Msg::EditAtom(field) => {
                if self.is_default_active() {
                    tracing::debug!("ignoring EditAtom — Default theme is read-only");
                    return;
                }
                self.editing_shell = None;
                // Toggle: clicking the open atom's swatch again closes it.
                if self.editing_atom == Some(field) {
                    self.editing_atom = None;
                    self.picker = None;
                } else {
                    let color = field.get(&self.active().atoms);
                    self.editing_atom = Some(field);
                    self.picker = Some(ColorPicker::new(color));
                }
            }
            Msg::EditShellColor(field) => {
                if self.is_default_active() {
                    tracing::debug!("ignoring EditShellColor — Default theme is read-only");
                    return;
                }
                self.editing_atom = None;
                if self.editing_shell == Some(field) {
                    self.editing_shell = None;
                    self.picker = None;
                } else {
                    let color = field.get(&self.active().shell);
                    self.editing_shell = Some(field);
                    self.picker = Some(ColorPicker::new(color));
                }
            }
            Msg::SetShellSpace(field, value) => {
                if self.is_default_active() {
                    tracing::debug!("ignoring SetShellSpace — Default theme is read-only");
                    return;
                }
                field.set(&mut self.active_mut().shell, value);
                self.broadcast_theme();
                self.dirty = true;
            }
            Msg::ClosePicker => {
                self.editing_atom = None;
                self.editing_shell = None;
                self.picker = None;
            }
            Msg::Picker(m) => {
                let Some(picker) = self.picker.as_mut() else { return };
                picker.update(m);
                let color = picker.color();
                if let Some(field) = self.editing_atom {
                    self.apply_atom(field, color);
                } else if let Some(field) = self.editing_shell {
                    self.apply_shell_color(field, color);
                }
            }
            Msg::SetFont(role, family) => {
                if self.is_default_active() {
                    tracing::debug!("ignoring SetFont — Default theme is read-only");
                    return;
                }
                let active = self.active_mut();
                match role {
                    FontRole::Ui => active.fonts.ui = family,
                    FontRole::UiMedium => active.fonts.ui_medium = family,
                    FontRole::Display => active.fonts.display = family,
                    FontRole::Chrome => active.fonts.chrome = family,
                    FontRole::Mono => active.fonts.mono = family,
                }
                self.install_active_fonts();
                self.broadcast_theme();
                self.dirty = true;
            }
            Msg::SelectTheme(name) => {
                let Some(idx) = self.themes.iter().position(|t| t.name == name) else {
                    return;
                };
                // Manual-only commit: switching themes abandons the unsaved
                // working set. Restore the current preset from its
                // checkpoint before leaving so discarded edits don't linger
                // in `themes`.
                if self.dirty {
                    self.themes[self.active_theme] = self.checkpoint.clone();
                }
                self.active_theme = idx;
                self.checkpoint = self.active().clone();
                self.dirty = false;
                self.naming = None;
                self.delete_armed = false;
                self.editing_atom = None;
                self.editing_shell = None;
                self.picker = None;
                self.refresh_active_theme();
                self.install_active_fonts();
                self.broadcast_theme();
            }
            Msg::NewThemeStart => {
                self.delete_armed = false;
                let base = if self.is_default_active() {
                    "default-copy".to_string()
                } else {
                    format!("{}-copy", self.active().name)
                };
                let suggested = self.unique_name(&base);
                self.naming = Some(suggested);
            }
            Msg::NewThemeInput(buffer) => {
                if self.naming.is_some() {
                    self.naming = Some(buffer);
                }
            }
            Msg::NewThemeCommit => {
                let Some(buffer) = self.naming.take() else { return };
                let name = buffer.trim().to_string();
                if !sola_core::theme::is_valid_theme_name(&name)
                    || self.themes.iter().any(|t| t.name == name)
                {
                    // Keep the input open so the user can correct it.
                    self.naming = Some(buffer);
                    return;
                }
                let mut copy = self.active().clone();
                copy.name = name;
                // "Save as": the fork keeps the current working set and is
                // persisted; the source preset reverts to its checkpoint so
                // the edits move to the new theme rather than being left
                // unsaved on the original.
                if self.dirty {
                    self.themes[self.active_theme] = self.checkpoint.clone();
                }
                self.themes.push(copy);
                self.active_theme = self.themes.len() - 1;
                self.checkpoint = self.active().clone();
                self.dirty = false;
                self.refresh_active_theme();
                self.install_active_fonts();
                self.broadcast_theme();
                self.persist_active_theme();
            }
            Msg::NewThemeCancel => {
                self.naming = None;
            }
            Msg::ArmDelete => {
                if !self.is_default_active() {
                    self.delete_armed = true;
                }
            }
            Msg::DisarmDelete => {
                self.delete_armed = false;
            }
            Msg::DeleteActiveTheme => {
                self.delete_armed = false;
                self.editing_atom = None;
                self.editing_shell = None;
                self.picker = None;
                if self.is_default_active() {
                    return;
                }
                let removed = self.themes.remove(self.active_theme);
                self.retract_custom_theme(&removed);
                self.active_theme = 0;
                self.checkpoint = self.active().clone();
                self.dirty = false;
                self.refresh_active_theme();
                self.install_active_fonts();
                self.broadcast_theme();
            }
            Msg::ResetAtom(field) => {
                if self.is_default_active() {
                    tracing::debug!("ignoring ResetAtom — Default theme is read-only");
                    return;
                }
                // Surgical: reset just this one atom to its compile-time
                // default. A live edit like any other (apply_atom marks
                // dirty + broadcasts, no persist).
                self.apply_atom(field, field.default_color());
            }
            Msg::SaveTheme => {
                if self.is_default_active() {
                    return;
                }
                self.persist_active_theme();
                self.checkpoint = self.active().clone();
                self.dirty = false;
            }
            Msg::RevertTheme => {
                if !self.dirty {
                    return;
                }
                self.themes[self.active_theme] = self.checkpoint.clone();
                self.editing_atom = None;
                self.editing_shell = None;
                self.picker = None;
                self.refresh_active_theme();
                self.install_active_fonts();
                self.broadcast_theme();
                self.dirty = false;
            }
            Msg::Noop => {}
        }
    }

    /// Recompute the live iced theme from the active preset's atoms, and
    /// refresh the process-wide selection colour so the storybook's own
    /// sidebar preview reflects a `selection` edit on the next render.
    fn refresh_active_theme(&mut self) {
        self.theme = theme::build_theme(&self.active().atoms);
        theme::install_selection(self.active().atoms.selection);
    }

    /// Write one atom onto the active preset and propagate it live:
    /// rebuild the iced theme + broadcast `Topic::Theme`, then mark the
    /// working set dirty. **No persist** — that's deferred to `Save`
    /// (see `Msg::SaveTheme`). No-op on the read-only Default. Driven by
    /// `Picker` (the color picker) as the user drags or types.
    fn apply_atom(&mut self, field: AtomField, color: iced::Color) {
        if self.is_default_active() {
            tracing::debug!("ignoring atom edit — Default theme is read-only");
            return;
        }
        field.set(&mut self.active_mut().atoms, color);
        self.refresh_active_theme();
        self.broadcast_theme();
        self.dirty = true;
    }

    /// Write one shell color onto the active preset and propagate it live.
    /// Unlike `apply_atom` there's no `refresh_active_theme` — shell
    /// tokens don't feed the storybook's own iced theme, only the
    /// broadcast bus value. **No persist** — deferred to `Save`.
    fn apply_shell_color(&mut self, field: ShellColorField, color: iced::Color) {
        if self.is_default_active() {
            tracing::debug!("ignoring shell color edit — Default theme is read-only");
            return;
        }
        field.set(&mut self.active_mut().shell, color);
        self.broadcast_theme();
        self.dirty = true;
    }

    /// Push the active preset's font selection into the process-wide
    /// fonts table so the storybook's own widgets see the swap before
    /// the bus delivery loops back.
    fn install_active_fonts(&self) {
        let f = &self.active().fonts;
        sola_kit::fonts::install(sola_kit::fonts::fonts_from_families(
            &f.ui, &f.ui_medium, &f.display, &f.chrome, &f.mono,
        ));
    }

    /// Emit the active preset as a `Topic::Theme`. Centralised so all
    /// edit paths stay symmetric.
    fn broadcast_theme(&self) {
        let active = self.active();
        let bus_theme = active.bus_theme();
        match sola_kit::app::bus().lock() {
            Ok(mut bus) => {
                if let Err(err) = bus.emit(Topic::Theme(bus_theme)) {
                    tracing::warn!("failed to emit Topic::Theme: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("bus mutex poisoned, can't emit Topic::Theme: {err}");
            }
        }
    }


    /// Persist the active preset on the bus under `Topic::CustomTheme`.
    /// No-op for Default (its values are reconstituted from Rust
    /// constants on every boot).
    fn persist_active_theme(&self) {
        if self.is_default_active() {
            return;
        }
        let active = self.active();
        let named = sola_bus::topics::NamedTheme {
            name: active.name.clone(),
            theme: active.bus_theme(),
        };
        match sola_kit::app::bus().lock() {
            Ok(mut bus) => {
                if let Err(err) = bus.emit(Topic::CustomTheme(named)) {
                    tracing::warn!("failed to emit Topic::CustomTheme: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("bus mutex poisoned, can't emit Topic::CustomTheme: {err}");
            }
        }
    }

    /// Retract a custom theme from the bus so the bus host drops it
    /// from `state.toml`. Caller is responsible for having already
    /// removed it from `self.themes`.
    fn retract_custom_theme(&self, removed: &ThemePreset) {
        let named = sola_bus::topics::NamedTheme {
            name: removed.name.clone(),
            theme: removed.bus_theme(),
        };
        match sola_kit::app::bus().lock() {
            Ok(mut bus) => {
                if let Err(err) = bus.retract(Topic::CustomTheme(named)) {
                    tracing::warn!("failed to retract Topic::CustomTheme: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("bus mutex poisoned, can't retract Topic::CustomTheme: {err}");
            }
        }
    }

    /// Bus delivered a `Topic::CustomTheme` add/update — fold into the
    /// in-memory `themes` list. Skipped if it collides with the
    /// hardcoded "Default" name (which is owned by Rust constants).
    fn upsert_custom_theme(&mut self, named: sola_bus::topics::NamedTheme) {
        if named.name == Self::DEFAULT_THEME_NAME {
            tracing::warn!(
                "ignoring bus-delivered CustomTheme named \"{}\" — that slot is reserved",
                Self::DEFAULT_THEME_NAME
            );
            return;
        }
        let preset = ThemePreset {
            name: named.name.clone(),
            atoms: theme::atoms_from_bus_theme(&named.theme),
            fonts: theme::font_selection_from_bus_theme(&named.theme),
            shell: theme::shell_style_from_bus_theme(&named.theme),
        };
        match self.themes.iter().position(|t| t.name == named.name) {
            Some(idx) => self.themes[idx] = preset,
            None => self.themes.push(preset),
        }
    }

    /// Bus delivered a `Topic::CustomTheme` retract — drop the matching
    /// preset. If it was the active one, fall back to Default.
    fn remove_custom_theme(&mut self, name: &str) {
        let Some(idx) = self.themes.iter().position(|t| t.name == name) else {
            return;
        };
        self.themes.remove(idx);
        if self.active_theme == idx {
            self.active_theme = 0;
            self.refresh_active_theme();
            self.install_active_fonts();
        } else if self.active_theme > idx {
            self.active_theme -= 1;
        }
    }


    /// If a live `Topic::Theme` has landed, point `active_theme` at the
    /// preset whose value matches it. Called after every `Topic::Theme`
    /// or `Topic::CustomTheme` delivery so the picker and swatch grid
    /// stay in sync regardless of replay order at startup. No-op if no
    /// live theme has been seen yet, or if none of the loaded presets
    /// matches.
    ///
    /// Matching is by value, not name, because a live `Topic::Theme`
    /// arrives anonymous (no preset name on the wire). To keep that match
    /// robust across kit upgrades, both sides are compared in the kit's
    /// *canonical* form — see the in-body note.
    fn resync_active_theme(&mut self) {
        let Some(live) = self.last_live_theme.as_ref() else {
            return;
        };
        // Canonicalise the live theme through the kit's model before
        // matching. A live `Topic::Theme` arrives as a raw token map, and
        // the on-disk `theme/current.yaml` that seeds it at startup may
        // predate tokens the kit has since added (every `shell-*` addition
        // grows the set). Comparing raw wire forms would then spuriously
        // fail — the saved preset reconstructs the newer token at its
        // default while the older live map omits it, so the two byte
        // forms differ and the selection silently resets to Default on
        // reopen. Round-tripping the live theme through deconstruct →
        // reconstruct applies the same per-token fallbacks the presets
        // use, so a token missing from the live map resolves to the
        // identical default on both sides and the match holds.
        let canonical = ThemePreset {
            name: String::new(),
            atoms: theme::atoms_from_bus_theme(live),
            fonts: theme::font_selection_from_bus_theme(live),
            shell: theme::shell_style_from_bus_theme(live),
        }
        .bus_theme();
        // Trust the current selection if it already resolves to the live
        // theme. A fresh fork of Default is byte-identical to Default
        // until it's edited, so an unguarded search would snap a
        // just-selected fork back to whichever identical preset sorts
        // first (Default at slot 0). Guarding keeps an explicit selection
        // put.
        if self.active().bus_theme() == canonical {
            return;
        }
        let Some(idx) = self.themes.iter().position(|p| p.bus_theme() == canonical) else {
            return;
        };
        if idx != self.active_theme {
            self.active_theme = idx;
        }
    }

    fn active(&self) -> &ThemePreset {
        &self.themes[self.active_theme]
    }

    fn active_mut(&mut self) -> &mut ThemePreset {
        &mut self.themes[self.active_theme]
    }

    fn is_default_active(&self) -> bool {
        self.active_theme == 0
    }

    /// Pick the first kebab-safe name in the `base`, `base-b`, `base-c`,
    /// ... series that isn't already taken. Bails after `z` (25 copies).
    fn unique_name(&self, base: &str) -> String {
        if !self.themes.iter().any(|t| t.name == base) {
            return base.to_string();
        }
        for c in b'b'..=b'z' {
            let candidate = format!("{base}-{}", c as char);
            if !self.themes.iter().any(|t| t.name == candidate) {
                return candidate;
            }
        }
        unreachable!("more than 25 copies of {base:?} — back away slowly")
    }

    pub fn view(&self) -> Element<'_, Msg> {
        // Bucket pages into sections in `Page::ALL` order; the first
        // section keyed by a given header wins (matches legacy's
        // first-appearance ordering).
        let mut buckets: Vec<(Option<&'static str>, Vec<SidebarItem<Msg>>)> = Vec::new();
        for p in Page::ALL.iter().copied() {
            let item = SidebarItem::new(p.label(), Msg::Select(p)).active(p == self.page);
            let key = p.section();
            match buckets.iter_mut().find(|(k, _)| *k == key) {
                Some((_, items)) => items.push(item),
                None => buckets.push((key, vec![item])),
            }
        }
        let sections: Vec<SidebarSection<Msg>> = buckets
            .into_iter()
            .map(|(key, items)| match key {
                Some(label) => SidebarSection::new(label, items),
                None => SidebarSection::unlabeled(items),
            })
            .collect();

        let content = scrollable(
            container(self.page_view())
                .padding(Padding::from([20, 28]))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        let right = column![self.header(), content, self.font_prewarm()]
            .width(Length::Fill)
            .height(Length::Fill);

        // The atom colour picker is anchored to its swatch from inside
        // the Theme page (see `pages::theme` + `popover_anchored`), so
        // there's no window-level float to compose here.
        row![sidebar(sections), right]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Zero-height transparent strip that lays out every family name
    /// in the kit's default UI font on the first frame. cosmic-text
    /// shapes glyphs at layout time, so once this strip is in the tree
    /// the family-name strings are warm in the shaping cache — opening
    /// a font pick_list later doesn't pay the first-shape tax.
    fn font_prewarm(&self) -> Element<'_, Msg> {
        // Only the shipped families are worth pre-shaping; the full
        // `family_options` list now includes every installed system
        // font, which we don't want to lay out every frame.
        let mut r = iced::widget::Row::new().spacing(8);
        for family in sola_kit::fonts::INSTALLED_FAMILIES {
            r = r.push(
                text(*family)
                    .size(1)
                    .style(|_t: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color::TRANSPARENT),
                    }),
            );
        }
        container(r)
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .clip(true)
            .into()
    }

    /// Global theme management bar shown above the content panel.
    fn header(&self) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match &self.naming {
            Some(buffer) => {
                let trimmed = buffer.trim();
                let name_ok = sola_core::theme::is_valid_theme_name(trimmed)
                    && !self.themes.iter().any(|t| t.name == trimmed);
                let input = kit_text_input::text_input("theme-name", buffer)
                    .on_input(Msg::NewThemeInput)
                    .on_submit(Msg::NewThemeCommit)
                    .style(kit_text_input::style)
                    .width(Length::Fixed(240.0));
                let mut save = button(text("Save"))
                    .style(kit_button::primary)
                    .padding(Padding::from([6, 14]));
                if name_ok {
                    save = save.on_press(Msg::NewThemeCommit);
                }
                let cancel = button(text("Cancel"))
                    .style(kit_button::ghost)
                    .padding(Padding::from([6, 14]))
                    .on_press(Msg::NewThemeCancel);
                let hint = text(if trimmed.is_empty() {
                    "lowercase letters and hyphens (e.g. solar-flare)"
                } else if !sola_core::theme::is_valid_theme_name(trimmed) {
                    "invalid — lowercase letters and hyphens only"
                } else if self.themes.iter().any(|t| t.name == trimmed) {
                    "name already taken"
                } else {
                    ""
                })
                .size(11)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().background.strong.color),
                });
                iced::widget::column![
                    row![input, save, cancel]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    hint,
                ]
                .spacing(4)
                .into()
            }
            None => {
                let names: Vec<String> =
                    self.themes.iter().map(|t| t.name.clone()).collect();
                let picker = pick_list(names, Some(self.active().name.clone()), Msg::SelectTheme)
                    .width(Length::Fixed(240.0));
                let new_btn = button(text("New Theme"))
                    .style(kit_button::secondary)
                    .padding(Padding::from([6, 14]))
                    .on_press(Msg::NewThemeStart);
                // Two-stage delete: outline "Delete" arms the confirm,
                // a second click ("Confirm?") commits. Default is
                // undeletable, so it renders disabled (no on_press).
                let del_btn: Element<'_, Msg> = if self.is_default_active() {
                    button(text("Delete"))
                        .style(kit_button::danger_outline)
                        .padding(Padding::from([6, 14]))
                        .into()
                } else {
                    kit_button::confirm_button(
                        self.delete_armed,
                        "Delete",
                        "Confirm?",
                        Msg::ArmDelete,
                        Msg::DeleteActiveTheme,
                    )
                    .padding(Padding::from([6, 14]))
                    .into()
                };
                let mut controls = row![picker, new_btn, del_btn]
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                // Save/Revert surface only with an unsaved working set; on
                // the read-only Default editing is a no-op so this never
                // shows there.
                if self.dirty {
                    let indicator =
                        text("Edited •").size(11).style(|theme: &iced::Theme| {
                            iced::widget::text::Style {
                                color: Some(theme.extended_palette().warning.base.color),
                            }
                        });
                    let save = button(text("Save"))
                        .style(kit_button::primary)
                        .padding(Padding::from([6, 14]))
                        .on_press(Msg::SaveTheme);
                    let revert = button(text("Revert"))
                        .style(kit_button::ghost)
                        .padding(Padding::from([6, 14]))
                        .on_press(Msg::RevertTheme);
                    controls = controls.push(indicator).push(save).push(revert);
                }
                controls.into()
            }
        };

        container(body)
            .padding(Padding::from([10, 28]))
            .width(Length::Fill)
            .style(|theme: &iced::Theme| {
                let p = theme.extended_palette();
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(p.background.weaker.color)),
                    border: iced::Border {
                        color: p.background.strong.color,
                        width: 0.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    }

    fn page_view(&self) -> Element<'_, Msg> {
        let editable = !self.is_default_active();
        let content: Element<'_, Msg> = match self.page {
            Page::Theme => pages::theme::view(
                &self.active().atoms,
                &self.active().fonts,
                &self.family_options,
                editable,
                self.editing_atom,
                self.picker.as_ref().map(|p| p.view().map(Msg::Picker)),
            ),
            Page::Shell => pages::shell::view(
                &self.active().shell,
                editable,
                self.editing_shell,
                self.picker.as_ref().map(|p| p.view().map(Msg::Picker)),
            ),
            Page::Text => pages::text::view(),
            Page::Button => pages::button::view(),
            Page::Badge => pages::badge::view(),
            Page::Card => pages::card::view(),
            Page::Field => pages::field::view(&self.field).map(Msg::Field),
            Page::Icon => pages::icon::view(),
            Page::NumberInput => {
                pages::number_input::view(&self.number_input).map(Msg::NumberInput)
            }
            Page::Readable => pages::readable::view(),
            Page::ColorPicker => {
                pages::color_picker::view(&self.color_picker).map(Msg::ColorPicker)
            }
            Page::Divider => pages::divider::view(),
            Page::Popover => pages::popover::view(),
            Page::Sidebar => pages::sidebar::view(&self.sidebar).map(Msg::Sidebar),
            Page::Split => pages::split::view(),
            Page::Toolbar => pages::toolbar::view(&self.toolbar).map(Msg::Toolbar),
        };

        // Theme/Shell carry their own atom/token editors; every other page
        // gets a contextual atom panel below its demo. The open picker (if
        // any) is created here and routed into the panel — only this branch
        // runs per render, so the Theme/Shell arms above never see it.
        let fields = self.page.atoms();
        if fields.is_empty() {
            content
        } else {
            let picker = self.picker.as_ref().map(|p| p.view().map(Msg::Picker));
            column![
                content,
                pages::theme::atom_panel(
                    &self.active().atoms,
                    fields,
                    editable,
                    self.editing_atom,
                    picker,
                ),
            ]
            .spacing(28)
            .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // E4: a page that isn't in `Page::ALL` is silently dropped from the
    // sidebar with no error. This test makes that a build/test failure:
    // the wildcard-free match is a compile-time guard (a new `Page`
    // variant won't compile until it's listed), and the asserts ensure
    // `VARIANTS` and `Page::ALL` stay in lockstep.
    #[test]
    fn every_page_variant_is_listed_in_all() {
        const VARIANTS: &[Page] = &[
            Page::Theme,
            Page::Shell,
            Page::Text,
            Page::Button,
            Page::Badge,
            Page::Card,
            Page::Field,
            Page::Icon,
            Page::NumberInput,
            Page::Readable,
            Page::ColorPicker,
            Page::Divider,
            Page::Popover,
            Page::Sidebar,
            Page::Split,
            Page::Toolbar,
        ];

        // Compile-time exhaustiveness: adding a `Page` variant forces a
        // new arm here, prompting the author to also extend `VARIANTS`.
        fn _exhaustive(p: Page) {
            match p {
                Page::Theme
                | Page::Shell
                | Page::Text
                | Page::Button
                | Page::Badge
                | Page::Card
                | Page::Field
                | Page::Icon
                | Page::NumberInput
                | Page::Readable
                | Page::ColorPicker
                | Page::Divider
                | Page::Popover
                | Page::Sidebar
                | Page::Split
                | Page::Toolbar => {}
            }
        }

        for v in VARIANTS {
            assert!(Page::ALL.contains(v), "{v:?} is missing from Page::ALL");
        }
        assert_eq!(
            Page::ALL.len(),
            VARIANTS.len(),
            "Page::ALL has a duplicate or an entry not in VARIANTS"
        );
    }

    // A fresh fork of Default is byte-identical to Default until it's
    // edited. `Topic::Theme` is anonymous (carries values, not a name),
    // so resync re-derives the active preset by value — and a naive
    // by-value search returns Default (slot 0) for the identical fork,
    // snapping the user's just-made selection back to Default. resync
    // must keep the active selection when it already matches the live
    // theme.
    #[test]
    fn selecting_a_fresh_fork_of_default_stays_selected() {
        let mut sb = Storybook::default();
        let mut fork = sb.themes[0].clone();
        fork.name = "my-copy".to_string();
        sb.themes.push(fork);
        sb.active_theme = 1;
        // Echo of our own broadcast: the live theme equals the fork's
        // (== Default's) bus form.
        sb.last_live_theme = Some(sb.themes[1].bus_theme());

        sb.resync_active_theme();

        assert_eq!(
            sb.active_theme, 1,
            "a fresh fork of Default must stay selected, not snap back to Default"
        );
    }

    #[test]
    fn preset_bus_theme_roundtrips_shell_style() {
        let mut preset = ThemePreset {
            name: "test".into(),
            atoms: theme::Atoms::default(),
            fonts: theme::FontSelection::default(),
            shell: theme::ShellStyle::default(),
        };
        preset.shell.switcher_bg = theme::parse("#ffb80080");
        preset.shell.launcher_width = 720.0;
        let bus = preset.bus_theme();
        assert_eq!(theme::shell_style_from_bus_theme(&bus), preset.shell);
    }

    // An external `Topic::Theme` whose value no longer matches the
    // active preset must still move the selection to the matching
    // preset (resync's actual job).
    #[test]
    fn resync_follows_external_theme_to_matching_preset() {
        let mut sb = Storybook::default();
        let mut other = sb.themes[0].clone();
        other.name = "other".to_string();
        other.atoms.accent = iced::Color::from_rgb(0.9, 0.1, 0.1);
        sb.themes.push(other);
        // Active is Default, but the live theme matches `other`.
        sb.active_theme = 0;
        sb.last_live_theme = Some(sb.themes[1].bus_theme());

        sb.resync_active_theme();

        assert_eq!(
            sb.active_theme, 1,
            "resync must follow the live theme to the matching preset"
        );
    }

    // An older `theme/current.yaml` predates a kit token addition, so the
    // live theme it seeds at startup is missing that token. resync must
    // still resolve it to the saved preset — comparing raw wire forms
    // used to choke here (the preset reconstructs the newer token while
    // the stale live map lacks it), silently resetting the selection to
    // Default on every reopen until the user re-saved.
    #[test]
    fn resync_tolerates_token_drift() {
        let mut sb = Storybook::default();
        let mut other = sb.themes[0].clone();
        other.name = "drifted".to_string();
        // Edit an atom so the preset is distinguishable from Default; the
        // drifted token itself stays at its default on both sides (it
        // can't carry a custom value — it didn't exist when this preset
        // was saved).
        other.atoms.accent = iced::Color::from_rgb(0.2, 0.7, 0.4);
        sb.themes.push(other);
        sb.active_theme = 0;

        // Simulate the stale on-disk live theme: the preset's bus form
        // with one shell token stripped, as if persisted before that
        // token existed.
        let mut stale = sb.themes[1].bus_theme();
        let removed = stale.palette.tokens.remove("shell-switcher-icon-fg-sel");
        assert!(removed.is_some(), "token must exist to model its later addition");
        // Sanity: the raw forms differ, so the old value-match would miss.
        assert_ne!(stale, sb.themes[1].bus_theme());
        sb.last_live_theme = Some(stale);

        sb.resync_active_theme();

        assert_eq!(
            sb.active_theme, 1,
            "resync must tolerate a live theme missing a newer token"
        );
    }

    #[test]
    fn theme_and_shell_pages_have_no_atom_panel() {
        // Those two carry their own editors (full grid / token editor),
        // so they opt out of the per-component panel.
        assert!(Page::Theme.atoms().is_empty());
        assert!(Page::Shell.atoms().is_empty());
    }

    #[test]
    fn sidebar_page_exposes_the_selection_atom() {
        // The whole reason `selection` exists: a tunable knob for the
        // sidebar's selected-row highlight, surfaced on its own page.
        assert!(
            Page::Sidebar.atoms().contains(&AtomField::Selection),
            "the Sidebar page must expose the selection atom"
        );
    }

    #[test]
    fn fresh_storybook_is_clean_with_matching_checkpoint() {
        let sb = Storybook::default();
        assert!(!sb.dirty, "a freshly booted storybook has no unsaved work");
        assert_eq!(sb.checkpoint.name, sb.active().name);
        assert_eq!(
            sb.checkpoint.atoms,
            sb.active().atoms,
            "the Revert checkpoint must start equal to the active preset"
        );
    }

    #[test]
    fn atom_field_default_color_is_the_compile_time_default() {
        // Drives the per-atom reset: resetting must land exactly on the
        // default-theme value for that atom.
        let defaults = theme::Atoms::default();
        assert_eq!(AtomField::Selection.default_color(), defaults.selection);
        assert_eq!(AtomField::Accent.default_color(), defaults.accent);
    }
}
