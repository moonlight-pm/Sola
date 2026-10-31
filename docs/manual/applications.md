# Applications

Sola's own applications are Iced programs on sola-kit. Each is its own
process, follows the theme, and is started by the
[session manager](session.md): from the launcher (Super+Space), from the
shell, or with `solactl session launch <app_id>`. On Oath their programs
are in `/bin`.

Each application window has a titlebar with its application's menus: the
first under the application's name, then its others, then the Window
menu, and the window's title at the right. Menus show shortcuts with ⌘ for the
Super key; the shell takes those chords and hands them to the focused
application. Several applications open a window per document, each a row
in the shell's bar: Browser a window per page, Terminal a window per tab,
Paint a window per image, and Projects and Design a window per project.

The shell is stable in daily use; the applications are early, and each is
incomplete. Applications that offer methods on the
[call plane](call.md) are driven with [solactl](solactl.md).

| Application | Program | Package | What it is | Notes |
|-------------|---------|---------|------------|-------|
| Terminal | `sola-terminal` | `pkg:sola-terminal` | A terminal on Alacritty's grid: a window per tab, split panes, each running in tmux | — |
| Projects | `sola-projects` | `pkg:sola-projects` | The folders of `~/Projects`, here or on another computer over ssh: a window per session (a terminal in tmux, on the computer it runs on), with a bar for its agent, model, turns, compactions and context. The launcher's Projects opens a window to pick a project in. Shows whether the coding agent in a session (Claude Code, Codex, Grok) is working, waiting, or done; its row in the bar marks one working or waiting on a decision, and one that asks or finishes gets an island in the top row. Call owner `projects`. | [solactl](solactl.md#projects-solactl-projects) |
| Browser | `sola-browser` | `pkg:sola-browser` | Chromium's engine (CEF 156) under Sola's own chrome: a window per page, a profile per desktop workspace, a Bitwarden vault, downloads, developer tools. Every Sola application opens web links in it. Call owner `browser`. | [notes](sola-browser.md) |
| Web apps | `sola-browser --app <id>` | `pkg:sola-browser` | A website that Browser runs as an application of its own, made from a Browser page: a launcher entry, its own windows, name, and menus | [notes](web-apps.md) |
| Mail | `sola-mail` | `pkg:sola-mail` | IMAP and SMTP for several accounts in one combined set of mailboxes, with rules, drafts, attachments, and safe HTML letters | [notes](sola-mail.md) |
| Calendar | `sola-calendar` | `pkg:sola-calendar` | Month, week, and day views of local, Google, iCloud, CalDAV, and feed calendars, with reminders, `.ics` invites, and today's events in the top row's clock | [notes](sola-calendar.md) |
| Paint | `sola-paint` | `pkg:sola-paint` | The default image viewer and editor, a window per image; sets the desktop's background picture | [notes](sola-paint.md) |
| Preview | `sola-preview` | `pkg:sola-preview` | A quick viewer for screenshots and images | [notes](sola-preview.md) |
| Design | `sola-design` | `pkg:sola-design` | HTML artboards on a canvas, written and edited by AI agents: a window per project, its canvases along the top. Call owner `design`. | [notes](sola-design.md) |
| Settings | `sola-settings` | `pkg:sola-settings` | Applications in the launcher, and web apps (a name, where each starts, removing one); Appearance (light or dark for web pages, the theme's colors, from the background picture or your own, the background picture and every picture set before it, each with the look it was worn with, the desk's opacity, the idle fade and lock); Login (the password and auto-login, [Security](security.md)); Mail accounts and rules; Calendar accounts. Also **Settings…** in the flower menu. | — |
| Bots | `sola-bots`, `sola-botsd` | `pkg:sola-bots` | Named assistant sessions, each a directory in `~/Bots` on the AI it was made with (Grok, Claude, or Gab AI); the daemon `sola-botsd` serves them on a Unix socket. On Oath nothing starts the daemon, so the Bots window has nothing to talk to there. Call owner `bots`. | [solactl](solactl.md#bots-solactl-bots) |
| Oath | `sola-oath` | `pkg:sola` | Oath's packages: turn one on or off and **Apply** (`oath set`, `oath apply`), or **Undo** the last change. Listed only on Oath. | — |
| Monitor | `sola-monitor` | `pkg:sola-monitor` | A live view of bus traffic and of the call plane's methods and calls | [notes](sola-monitor.md) |
| Scope | `sola-scope` | `pkg:sola-scope` | A pixel loupe: the screen under the pointer, magnified | [notes](sola-scope.md) |
| KVM | `sola-kvm` | `pkg:sola` | One keyboard and mouse across two machines, with a shared clipboard; once paired, each machine drops input it did not seal ([Security](security.md#pairing-the-kvm)). A service, not in the launcher. | [notes](sola-kvm-operator.md) |

There is no separate Wrapper application: a wrapped website is a
[web app](web-apps.md) that Browser hosts.

The notes are kept in the repository beside this manual and are not part
of the page: they describe each application's keys, menus, and files in
detail, and change as often as the applications do.
