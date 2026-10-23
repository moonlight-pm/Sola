# Applications

Sola's own applications are Iced programs on sola-kit. Each is its own
process and its own Wayland window, follows the theme, and is launched by
the [session manager](session.md): from the launcher (Super+Space), from
the shell, or with `solactl session launch <app_id>`. Binaries live in
`/opt/sola/bin`; each restarts itself when its binary there is replaced.

The shell is stable in daily use; the applications are early, and each is
incomplete. [docs/capabilities.md](../capabilities.md) lists what each can
do and what it lacks. Applications that offer methods on the
[call plane](call.md) are driven with [solactl](solactl.md).

| Application | Binary | What it is | Notes |
|-------------|--------|------------|-------|
| Terminal | `sola-terminal` | A terminal on Alacritty's grid, with tabs | — |
| Projects | `sola-projects` | Each project is its own window with its tabs along the top, one per checkout (the root or a git worktree), each with terminal panes; shows whether the coding agent in a pane (Claude Code, Codex, Grok) is working, waiting, or done, and the project's row in the shell's tab bar shows when one is working or waiting on a decision. Call owner `projects`. | [solactl](solactl.md#projects-solactl-projects) |
| Browser | `sola-browser` | Chromium's engine (CEF) under Sola's own chrome: a window per page, profiles, a Bitwarden vault, downloads, developer tools. The default for web links. Call owner `browser`. | [notes](sola-browser.md) |
| Mail | `sola-mail` | IMAP and SMTP for several accounts, with drafts and attachments | [notes](sola-mail.md) |
| Calendar | `sola-calendar` | Month, week, and day views of local, Google, iCloud, CalDAV, and ICS calendars, with reminders | [notes](sola-calendar.md) |
| Settings | `sola-settings` | Applications in the launcher (website wrappers included), and mail and calendar accounts | — |
| Arcade | `sola-arcade` | The Steam library in a window; each game runs in a nested gamescope session sized to it | [notes](sola-arcade.md) |
| Spotify | `sola-spotify` | Playback through librespot, playlists, the library, and Spotify Connect | [notes](sola-spotify.md) |
| Paint | `sola-paint` | The default image viewer and editor | [notes](sola-paint.md) |
| Preview | `sola-preview` | A quick viewer for screenshots and images | [notes](sola-preview.md) |
| Design | `sola-design` | HTML artboards on a canvas, written and edited by AI agents. Call owner `design`. | [notes](sola-design.md) |
| Website wrappers | `sola-wrapper <id>` | A website as an application of its own, with a launcher entry and desktop notifications | [notes](sola-wrapper.md) |
| Monitor | `sola-monitor` | A live view of bus traffic and of the call plane's methods and calls | [notes](sola-monitor.md) |
| Scope | `sola-scope` | A pixel loupe: the screen under the pointer, magnified | [notes](sola-scope.md) |
| KVM | `sola-kvm` | One keyboard and mouse across two machines, with a shared clipboard | [notes](sola-kvm-operator.md) |
| Bots | `sola-bots`, `sola-botsd` | Named assistant sessions, each with its own home directory; the daemon also serves a phone app over HTTP. Call owner `bots`. | [solactl](solactl.md#bots-solactl-bots) |

The notes are kept in the repository beside this manual and are not part of
it: they describe each application's keys, menus, and files in detail, and
change as often as the applications do.

`sola-kit` is also a binary, **Kit**: the storybook, which shows every
toolkit component, and the theme editor (see [Theme and toolkit](theme.md)).
