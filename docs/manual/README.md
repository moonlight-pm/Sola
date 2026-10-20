# Sola manual

The manual for Sola as it works today. The site renders these chapters, in
this order, as one page at
[sola.moonlight.pm/manual](https://sola.moonlight.pm/manual) (`names` in
[`site/app/pages/manual.thx`](../../site/app/pages/manual.thx) — keep the
two lists the same).

| Chapter | Contents |
|---------|----------|
| [Introduction](introduction.md) | What Sola is, requirements, the NixOS module, building, starting a session, paths |
| [Fonts](distribution.md) | The font families Sola needs installed system-wide |
| [Shell](sola-shell.md) | Menubar, launcher, switcher, zones, keyboard shortcuts |
| [Browser](sola-browser.md) | sola-browser: tabs, profiles, the Bitwarden vault, downloads |
| [Mail](sola-mail.md) | sola-mail |
| [Calendar](sola-calendar.md) | sola-calendar and its accounts |
| [Spotify](sola-spotify.md) | sola-spotify |
| [Arcade](sola-arcade.md) | The Steam library and nested games |
| [Paint](sola-paint.md) | The default image viewer and editor |
| [Preview](sola-preview.md) | The quick image viewer |
| [Website wrappers](sola-wrapper.md) | A website as its own application |
| [Design](sola-design.md) | HTML artboards on a canvas, for agents |
| [Monitor](sola-monitor.md) | The bus and call-plane inspector |
| [Scope](sola-scope.md) | The pixel loupe |
| [KVM](sola-kvm-operator.md) | One keyboard and mouse across two machines |
| [solactl](solactl.md) | The command line: every verb |

Not chapters yet: Terminal, Workspaces, Settings.

Kept here as developer notes, not on the site:
[design-language.md](design-language.md) (the visual law for kit and
shell work), [cursor-theme-loading.md](cursor-theme-loading.md), and
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
