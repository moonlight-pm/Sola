# Sola manual

The manual for Sola as it works today. The site renders these chapters, in
this order, as one page at
[sola.moonlight.pm/manual](https://sola.moonlight.pm/manual) (`names` in
[`site/app/pages/manual.thx`](../../site/app/pages/manual.thx) — keep the
two lists the same).

| Chapter | Contents |
|---------|----------|
| [Introduction](introduction.md) | What Sola is, requirements, the NixOS module, fonts, building, starting a session, paths |
| [Architecture](architecture.md) | How a session is put together: the processes, the three planes, sockets, start-up, restarts |
| [Process manager](process-manager.md) | `sola`: what it starts, in what order, restart policy, binary watch, ending the session |
| [The bus](bus.md) | `sola-bus`: socket, wire format, every topic, sticky and persisted state, reconnects |
| [The call plane](call.md) | `sola-call`: owners and methods, the wire format, timeouts, observers, MCP |
| [Compositor](compositor.md) | River and `sola-river`: window management, focus, zones, outputs, screenshots, input |
| [Session manager](session.md) | `sola-session`: launching, tracking, and closing applications; opening URLs and files |
| [Shell](sola-shell.md) | `sola-shell`: menubar, tab bar, launcher, zones, keyboard shortcuts |
| [Security](security.md) | The password, auto-login, the lock screen, Exit Sola's console, KVM pairing: each way in, and how to close it |
| [Theme and toolkit](theme.md) | `sola-kit` and the theme protocol: palette tokens, font roles, live updates |
| [solactl](solactl.md) | The command line: every command, method, and flag |
| [Applications](applications.md) | The applications, one line each, with links to their notes |

Application notes — kept here, linked from the Applications chapter, not on
the page: [Browser](sola-browser.md), [Mail](sola-mail.md),
[Calendar](sola-calendar.md), [Arcade](sola-arcade.md), [Paint](sola-paint.md),
[Preview](sola-preview.md), [Web apps](web-apps.md), [Design](sola-design.md),
[Monitor](sola-monitor.md), [Scope](sola-scope.md),
[KVM](sola-kvm-operator.md), [Fonts](distribution.md). They follow the same
rules.

Developer notes, not on the site: [design-language.md](design-language.md)
(the visual law for kit and shell work),
[cursor-theme-loading.md](cursor-theme-loading.md), and
[sola-river-layer-shell.md](sola-river-layer-shell.md).

Engineering status lives elsewhere: [`../capabilities.md`](../capabilities.md)
(what exists, with gaps and dogfood state), [`../roadmap.md`](../roadmap.md),
and [`../../CURRENT.md`](../../CURRENT.md).

## Rules for this manual

- Document working behaviour only ([progress model](../progress-model.md)).
  A partial feature is described with its limits, or left out. Never
  describe plans as facts.
- No dogfood state: install dates, build modes of an install, "smoked" /
  "unsmoked", and session history go in the Dogfood column of
  [`../capabilities.md`](../capabilities.md), not here.
- Write for people using Sola: plain, exact, upfront about limits, no
  promotional language. Name applications as the launcher does (Browser,
  Mail); crate and binary names (`sola-browser`) are for commands and paths.
- No host names of particular machines; use placeholders (`<server>`).
- The site's renderer supports a Markdown subset; stay inside it:
  - one `#` heading per file (the chapter title, unnumbered); `##` and `###`
    for sections, `####` sparingly;
  - paragraphs, `- ` and `1. ` lists (continuation lines indented to the
    item's text), fenced code marked `bash`, `nix`, `toml`, or `text`;
  - pipe tables with a header row and a `|---|` separator; `\|` for a
    literal pipe inside a cell;
  - inline `code`, **bold**, *italic* (asterisks), and `[text](target)`
    links;
  - `> ` blockquotes;
  - no raw HTML, images, footnotes, reference links, setext headings,
    horizontal rules, or trailing-space line breaks.
- Link another chapter as `sola-shell.md#section-slug`, the same chapter as
  `#slug`, and anything outside the manual by a relative path
  (`../specs/…`); the site turns those into links to the repository.
- A new chapter goes in the table above and in `names` in the site, in the
  same commit.
