# Omarchy — ideas worth considering for Sola

**Status:** idea (parked 2026-08-22). Super+K cheatsheet **promoted** 2026-08-31
([freeze](../specs/2026-08-31-window-menu-and-shortcuts-design.md)). Do not
implement the rest from this file. Promote a slice into a freeze + plan +
`CURRENT.md` **Now** if work starts.  
**Sources:** [omarchy.org](https://omarchy.org/), [Quattro manual](https://omarchy.org/manual/),
[`basecamp/omarchy` `quattro`](https://github.com/basecamp/omarchy) (v4.0.0, 2026-08-14).  
**Sola locks this must not reopen:** Iced + sola-kit; River; bus + call plane;
graphite / macOS-dark craft; default-float + zoning; Grok-first Workspaces;
CEF in `sola-browser`. See root [`CURRENT.md`](../../CURRENT.md) **Locked models**.

Omarchy and Sola share an instinct — a daily-driver Linux desk that is
opinionated, beautiful, and ready for real work — but they are not the same
kind of product. Steal **coverage and operator loops**. Do not steal tiling,
Hyprland, QML, or the ricing catalog.

---

## What Omarchy actually is

Omarchy (DHH / 37signals, now the Omacom Foundation) is an **omakase Arch
distro**. Quattro rewrote the chrome in [Quickshell](https://quickshell.org/):
bar, launcher, menus, notifications, OSD, lock, and polkit live in **one
long-running shell process** with plugins. Under that is still Hyprland + Arch
+ a large `omarchy-*` CLI. It ships Neovim, Chromium, Steam, frameless web-app
wrappers, and lazy-loaded coding-agent CLIs.

Quattro’s “drop Waybar / Walker / Mako / hyprlock, one shell process” is the
closest architectural rhyme. Sola already did that for chrome (`sola-shell` as
one iced daemon). Omarchy is still assembling a DE from distro pieces; Sola is
writing the DE.

| Surface | What it really is |
|---|---|
| **ISO** | Dedicated-drive install, LUKS by default, minutes to a usable desk |
| **Menu** | Super+Space is not just an app launcher — Install / Setup / Style / Update / Capture |
| **CLI** | `omarchy <group> <command>` is the same surface as the menu and the hotkeys; exists so an **agent** can operate the desk |
| **Theme pack** | One `colors.toml` generates bar, lock, terminal, neovim, Chromium, OSD, wallpaper, unlock art |
| **Clipboard** | Super+C/X/V everywhere including the terminal; Super+Ctrl+V history (text + images) |
| **Capture** | Print Screen family: freeze-picker, screenshot, record, OCR, QR, color, transcode |
| **Session** | Shell-owned idle JSON; lock; DND; night light; stay-awake; screensaver; bar glyphs only when a mode is on |
| **Agents** | Default-agent picker; lazy mise stubs; bar usage panel; crash → agent + diagnose skill; Herdr multiplexer that knows blocked vs working |
| **Plugins** | Bar widgets / panels / overlays as Quickshell plugins; clone a built-in to `~/.config/omarchy/plugins/` |
| **Updates** | Snapshot → migrate → packages; bar badge; channels stable / RC / edge / dev |

---

## Same instinct, different product

| | Omarchy | Sola (as-built) |
|---|---|---|
| Kind | Opinionated Arch + Hyprland + Quickshell | From-scratch Wayland DE |
| Chrome | One Quickshell process, QML plugins | One iced `sola-shell` daemon |
| Windowing | Tiling-first (dwindle / scrolling) | Default-float + opt-in zoning |
| Look | 22 skin themes as identity | One graphite system, bus tokens |
| Apps | Distro packages + web-app wrappers | First-party Iced (browser, mail, terminal, Workspaces, arcade, …) |
| Agents | Agent-agnostic CLI zoo + Herdr | `sola-workspaces`, Grok-first, call plane |
| IPC | Scripts + files + Hyprland | Bus + sola-call + Wayland |
| Dist | Pacman overlay, ISO, LUKS mandatory | NixOS Shape 1 + ISO scaffold (e2e pending) |

Sola is already ahead on process model, restartability, typed IPC, first-party
apps, and Workspaces as a product. Omarchy is ahead on **coverage**: lock,
clipboard, capture, update, onboarding, theme-everywhere, agent CLI.

---

## Leave on the floor

These are load-bearing for Omarchy and would fight Sola:

| Omarchy | Why it does not travel |
|---|---|
| Tiling-first Hyprland (dwindle / scrolling, mouse-hostile first boot) | Default-float + zoning; macOS density is the craft reference |
| 22 skin themes as identity (Tokyo Night, Gruvbox, …) | Graphite is the system; not a ricing catalog |
| QML plugin shell / clone-a-widget-to-hack-it | Kit + bus theme is the extension story; no second UI runtime |
| Agent-agnostic (`claude` / `codex` / `opencode` / … as equals) | Workspaces is Grok-first until Grok status is trustworthy |
| Bash/pacman overlay, ad-hoc migrations, AUR as App Store | NixOS module + `cargo make` is the stronger packaging story |
| Web-app zoo as the app set (HEY, ChatGPT, WhatsApp, Zoom wrappers) | Real first-party mail, browser, terminal, Workspaces, arcade |
| Super-heavy chord soup (`Super+Ctrl+Alt+…`) | Super-latch bug is a reminder: fewer, Mac-shaped chords |

Critics of Omarchy (packaging, security of script-as-distro) are real. Sola
should keep winning on integrity, not on “more scripts in `bin/`”.

Also refuse, unless a later freeze says otherwise:

- Replacing River with Hyprland
- Growing a plugin marketplace for shell chrome
- Launching agents in don’t-ask mode from `$HOME` into `~/Work` (Workspaces already has a project model)
- Usage-dashboard chrome on the menubar as v1

---

## Steal: coverage that fits Sola as it is

Ranked by daily-driver pain removed without reopening locks. Each item is a
**candidate slice**, not a commitment.

### 1. Three faces of one control plane

Omarchy’s rule: **menu = hotkey = `omarchy` CLI**. Theme, screenshot, bar,
plugins, update, even `omarchy menu summon style.theme`. The CLI exists so an
agent can operate the desk.

Sola already started this: `solactl` + sola-call + Workspaces verbs in lockstep
with the app ([CLI freeze](../specs/2026-08-18-workspaces-cli-design.md)).

**Sola version:**

- `solactl theme set`, font set, lock, idle, capture, debug dump
- Shell surfaces summonable by path (`solactl shell menu theme`)
- One `solactl debug` that packages logs, versions, bus/call health

Highest-leverage steal. Makes Grok in Workspaces a **desk operator**, not just
a code operator. Extends the call-plane law already in force. Confirm gates
remain **D3** — do not invent.

### 2. Unified Super+C / X / V, including the terminal

Omarchy’s best Mac-refugee feature: copy/paste is Super everywhere, including
the terminal. No `Ctrl+Shift+C` vs `Ctrl+C` kills-the-process split. History is
Super+Ctrl+V (text + images, searchable).

Kit apps already lean Super/Cmd. Terminal and Workspaces PTYs still live in
Linux-clipboard land. Paint/preview still have **no image clipboard**. Omarchy
puts every screenshot on disk *and* the clipboard, then the notification
thumbnail opens the annotator.

**Sola version:** Super+C/V in sola-terminal + Workspaces PTYs; clipboard
history overlay in the shell; screenshot/image paste into paint.

### 3. Super+K as the only hotkey you memorize — **promoted 2026-08-31**

Live cheatsheet overlay. Coming-from-Mac chapter: “when you blank, hit Super+K.”

**Sola version (installed `kit`+`shell` debug 2026-08-31):**
[freeze](../specs/2026-08-31-window-menu-and-shortcuts-design.md). Shell overlay
lists built-in chords + the focused app’s menus; click/Enter runs the action.

### 4. Capture as a product, not a PNG dump

Omarchy Print Screen family:

- Freeze the screen, then region / window / monitor (keyboard-driven picker)
- Shot → disk **and** clipboard; notification thumbnail → annotator
- Screen record with audio menu + optional webcam pip
- OCR region → clipboard
- QR decode marked **sensitive** (not stored in clipboard history — 2FA URIs)
- Color picker
- Transcode before share

Sola has `compositor.screenshot` → sola-preview, Super+Shift+3/4, single-output
([screenshot plan](../specs/2026-07-20-screenshot-capture-plan.md), request path
on sola-call).

**Sola version:** freeze-picker in river/shell; clipboard + preview; annotation
in **paint** (already the MIME dest); record later. OCR / QR / color once the
picker exists.

### 5. Theme as a complete package, not only a palette editor

Omarchy: one `colors.toml` generates bar, lock, terminal, neovim, Chromium,
OSD. Wallpaper set per theme. Unlock/Plymouth art per theme. Extra themes
install from a git URL.

Sola already has `Topic::Theme` + font roles + shell tokens — architecturally
better than templated dotfiles. Product gap: a theme change restyles **terminal
grid, browser chrome, lock (when it exists), wallpaper**, not only kit atoms.
A named theme is a folder you can export/import.

**Sola version:** keep graphite as default; allow a **pack** format. Skip the
22-skin marketplace.

### 6. Session hygiene: lock, idle, DND, night light, stay-awake

Quattro’s shell owns idle. One JSON: screensaver at 150s, lock at 300s, both
from last activity. Toggles plus bar glyphs that only appear when a mode is on.
DND still writes to notification history; confirmation toasts still get
through.

Sola has no lock screen, no idle policy, no DND, no night light.

**Sola version:** shell-owned lock + idle timings; DND; stay-awake. Night light
and ASCII screensaver are optional later. Do not bolt hyprlock.

### 7. Coming-from-Mac as a real chapter

Omarchy’s translation table is product work: Spotlight → Super+Space,
Cmd+Shift+4 → Print Screen, Time Machine → snapshots, App Store → Install
menu, **closing a window actually quits**.

Sola is *more* Mac-like than Omarchy. `docs/manual/` is the right home.

**Sola version:** one page “from macOS” when ISO e2e lands. Not a settings page.

### 8. ISO that is a computer, not an engineering harness

Omarchy’s pitch: minutes to a dedicated encrypted disk, Super+Return, you work.
Updates: snapshot → migrate → packages; bar badge; four channels.

Sola’s dist path is the same ambition (flower splash, kit wizard, loginless
Sola in QEMU) with **ISO e2e still pending**
([distribution freeze](../specs/2026-08-05-distribution-image-design.md)).

**Sola version:** steal the *feel*, not Arch.

- Install finishes as a **usable desk** (browser, terminal, Workspaces, mail, theme)
- One update verb (even if it is `nixos-rebuild` / tarball) with a menubar affordance
- `solactl debug` for “paste this in Discord”
- LUKS-by-default is an **ask-human** for the ISO (Omarchy made it mandatory)

### 9. Web apps / site-specific browser — via sola-browser

Omarchy wraps Chromium frameless for Grok.com, ChatGPT, YouTube, Zoom.

Sola already has CEF chrome with profiles and tab groups.

**Sola version:** “Open as app” / pinned profile + no omnibox. Not a second
browser. Grok.com as a dedicated window is the obvious first one; it does not
compete with Workspaces (chat site vs CLI agent).

### 10. Agent as a desk citizen (finish the loop)

Omarchy Quattro: default agent picker, Super+Shift+Ctrl+A, `a` alias, bar usage
panel, crash → agent with a diagnose skill, shipped skill in every harness’s
skills dir. Herdr is a multiplexer that **knows if the agent is blocked**.

`sola-workspaces` is the better product: project rail, spawn sibling, presence
marks, `solactl workspaces`, Grok hooks.

What Omarchy still has that Sola does not:

- Crash/coredump → “diagnose this with the desk skill”
- A **Sola skill** auto-installed for Grok on a Sola machine (in-repo
  `.grok/skills/` is not the ISO)
- `solactl` as the skill’s only door (already law for Workspaces)

Do **not** copy agent-agnostic launchers or a menubar usage dashboard as v1.

---

## Medium / later

Good ideas, not first promotions:

- **Scratchpad / popped sticky float** — Quake console and “this video follows
  me across spaces.” River tags + float; after lock/idle.
- **Notification history + invoke last** — Omarchy’s Super+Alt+, opens the last
  screenshot in the editor. Sola has toasts; history is a shell feature.
- **Share (LocalSend)** — AirDrop for mixed-OS houses. After capture.
- **File manager in the focused terminal’s cwd** — needs a first-party files
  app or a disciplined `xdg-open` of cwd.
- **Fingerprint / FIDO2 for lock and sudo** — after a lock screen exists.
- **Calendar / audio mixer / bluetooth panels** as shell popovers. Do **not**
  clone Omarchy’s rearrangeable Waybar-like bar; Sola’s menubar is Mac-shaped.
- **Windows VM for the one app you must have** — only if dogfood needs it.
- **Reminders / weather notices** — easy to become toy. Menubar already has
  clock/stats.

---

## Where Sola is already ahead

- Typed bus + call plane vs a pile of bash
- Independently restartable processes
- First-party Iced apps (browser with vault/passkeys, mail, paint, Workspaces)
  vs wrapping the web
- Workspaces as a **product** vs tmux layouts + Herdr
- Graphite design law vs theme-of-the-week
- NixOS Shape 1 for colleagues vs “curl | bash on Arch”

---

## Suggested promotion order (not CURRENT)

If any of this becomes work, natural order:

1. **solactl parity** — theme / capture / debug / shell summon
2. **Unified clipboard + history** (Mac muscle memory; unblocks paint image paste)
3. **Capture picker** (clipboard + annotate in paint; record later)
4. **Lock / idle / DND** (session hygiene)
5. **Hotkey overlay**
6. **Theme pack** that restyles terminal + browser + wallpaper

Do not change `CURRENT.md` **Now** until one of these is promoted.

---

## Ask-human (do not invent)

If a freeze needs a policy call, stop and ask. Candidates:

- LUKS-mandatory on the product ISO
- Night light as v1 vs later
- Web-apps-as-apps vs “just use the browser”
- Notification-history chrome
- Agent crash-reporter (privacy: coredumps off the box)
- Clipboard history retention / sensitive-item rules (QR / passwords)

Existing Decision points that already cover related ground: **D3** (call-plane
confirm) for any new `solactl` verbs that mutate the desk.

---

## Links

- https://omarchy.org/
- https://omarchy.org/manual/
- https://github.com/basecamp/omarchy (branch `quattro`)
- https://world.hey.com/dhh/omarchy-is-out-4666dd31
- Sola: [`CURRENT.md`](../../CURRENT.md), [`capabilities.md`](../capabilities.md),
  [`open-questions.md`](../open-questions.md)
