# Opening links and files — handlers Sola keeps

**Date:** 2026-10-31  
**Status:** **decided** (Joshua, 2026-10-31: the answers below); slice 1 in progress.  
**Implementation (2026-10-31):** slice 1 built on branch `sola-open` (tests and `cargo make build`; not applied). The Browser opens a link it does not load only on a click. Slice 2 not started.  
**Branch:** `sola-open`  
**Related:** [D14](../open-questions.md#d14--what-pkgsola-ships-on-oath-p2) (answered: Sola owns `/bin/xdg-open`), decision log "Link and file handlers", [session manager](../manual/session.md#opening-links-and-files).

Joshua (2026-10-31): "currently on our oath system i do not believe opening
links between apps is working"; "Since i run almost only sola apps -- i wonder
if we couldn't do something custom, with perhaps backward compatibilty";
"sola owns it.."; "we put this in sola-kit -- any app can register itself for
any custom protocol -- we'll have to determine how they keep from stomping on
each other. I think this is what macos does in some way."

## Today

On novus there is no `xdg-open` at all, `$BROWSER` is unset, and `gio open`
says "Operation not supported". A link clicked in a Sola app works (Terminal,
Mail, Calendar and Projects call `sola_core::open_url`), but everything that
asks the system goes nowhere: CLI tools that open a browser (`gh auth login`,
`cf`, `claude /login`, `cargo doc --open`, Python's `webbrowser`), Steam's
links, `claude-cli://` deep links (Claude Code wrote a `.desktop` handler that
nothing reads), the Browser's Open on a download and the Oath app's home link
(both run `xdg-open`). The Browser does nothing with a link it cannot load
(`mailto:`, `claude-cli:`), and nothing in Sola knows which app opens what
besides three fixed openers (URL → Browser, image → Paint, `.ics` →
Calendar).

## Decided

| Question | Answer |
|---|---|
| Who owns `/bin/xdg-open` on Oath | Sola: `pkg:sola` ships Sola's own opener under that name (D14). Not xdg-utils, not `.desktop` files. |
| How apps say what they open | They **claim** link schemes and file types through the kit. |
| What a claim covers | **Link schemes and file types**, in one registry. |
| A second app claims something that already has a handler | The handler **stays**. The newcomer is an alternative, listed in Settings. |
| An app wants to be the handler | It may **ask**; the shell asks you, and only your yes changes it. |
| Who sets handlers | You, in Settings. |
| What nothing in Sola claims | Backward compatibility: `.desktop` handlers and `mimeapps.list`. |

## The model

### Claims

A claim is an app id with the schemes (`mailto`, `http`) and file types
(extensions: `png`, `ics`) it opens. Two sources:

- **Sola's own apps** ship theirs, in `sola-open` (`builtin.rs`), so a fresh
  machine opens images before Paint has ever run:

  | App | Schemes | Types |
  |---|---|---|
  | Browser | `http`, `https` | `html`, `htm`, `xhtml`, `pdf`, `svg`, `txt`, `json`, `xml` |
  | Paint | — | `png`, `jpg`, `jpeg`, `jpe`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `tga` |
  | Calendar | — | `ics`, `ical`, `icalendar` |
  | Mail | `mailto` | — |

- **Any kit app** claims at start: `BusSetup::new(id).opens(claim)`, or
  `.opens_own()` for one of Sola's own (its built-in claim, so both sources
  agree). The kit writes the claim to
  `~/.config/sola/handlers/claims/<app id>.json` (`{app_id, schemes, types,
  since}`; one file per app, so apps never write each other's) and keeps
  `since` from the first time.

### The handler for a scheme or a type

In order, the first that still exists (its program is in Sola's bin
directory, or its `.desktop` file is there):

1. The one you chose: `defaults` in `~/.config/sola/handlers/defaults.json`
   (`"mailto:": "sola-mail"`, `".png": "sola-paint"`). Written by Settings
   and by a confirmed request, never by a claim.
2. Sola's own claimant (the table above).
3. Other claimants, the earliest `since` first. A later claim never moves
   ahead of an earlier one: that is what keeps apps from stomping on each
   other.
4. `~/.config/mimeapps.list`, `[Default Applications]`
   (`x-scheme-handler/<scheme>`).
5. A `.desktop` file whose `MimeType` lists `x-scheme-handler/<scheme>`, in
   `$XDG_DATA_HOME/applications` (`~/.local/share/applications`) then each
   `$XDG_DATA_DIRS` entry's `applications` (default `/usr/local/share`,
   `/usr/share`), by file name.

Steps 4–5 are for schemes; a file type nothing in Sola claims has no
handler in slice 1.

### Delivery

A claiming kit app answers one more call-plane method, **`open`**
(`{target}`), which the kit adds to its advertised methods; the app gets it
through `call_subscription` like any other call and replies when it has
opened the target. To open:

1. The opener resolves the handler.
2. A Sola app whose `open` is in the call catalog: invoke it.
3. Not running: a `LaunchApp` for the app (its program, no arguments), so
   the session starts it and tracks it; wait up to 20 s for its `open` to
   appear in the catalog, then invoke it.
4. No call plane or bus: start `<app> --open <target>`, the per-app path the
   Browser, Paint and Calendar already have (desk-restore).
5. A `.desktop` handler: run its `Exec` with the field codes filled in (`%u`
   `%U` `%f` `%F` the target, `%i` `%c` `%k` dropped; double quotes group).

### The opener

One library, `sola-open` (on `sola-core`, `sola-bus`, `sola-call`; no iced),
used by:

- `/bin/xdg-open` on Oath — the crate's binary, named `xdg-open`, in
  `pkg:sola`. Exit status as xdg-open's: 0 opened, 1 usage, 2 no such file,
  3 nothing opens it, 4 the handler failed.
- `solactl open` (the same routing).
- Kit apps: `sola_kit::open(target)`, off the UI thread. Terminal, Projects,
  Mail and Calendar links go through it, so a `mailto:` in Terminal reaches
  Mail and a `claude-cli:` reaches Claude Code.
- The Browser: a navigation to a scheme it does not load (anything but
  `http`, `https`, `file`, `about`, `data`, `blob`, `javascript`,
  `view-source`, `chrome`, `devtools`, `filesystem`, `ws`, `wss`, `sola`)
  is cancelled and, when you clicked it, handed to the opener; without a
  click it is only logged, so a page cannot start apps by itself. Its Open on a download and the Oath app's home link use it too,
  not `xdg-open` by name.

A target is a URL when it starts with a scheme (`[a-z][a-z0-9+.-]*:`), a
file otherwise; `file://` is a file. A file's type is its extension, lower
case.

### Asking to be the handler (slice 2)

`sola_kit::request_default(kind)` (a scheme or a type) emits
`DefaultHandlerRequest {app_id, kind}`. The shell shows "Open mailto: links
with *App* instead of *Mail*?" with **Keep Mail** (the default) and **Use
*App***. Only **Use** writes `defaults`. An app that is already the handler
gets no prompt.

### Settings (slice 2)

**Settings → Default Apps**: every scheme and type with a claimant, its
handler, and a menu of its claimants (and `.desktop` handlers for schemes).
Choosing one writes `defaults`.

## Slices

1. **Opener and registry.** `sola-open` (builtin claims, claim files,
   defaults, resolution, `.desktop` fallback, delivery); `xdg-open` in
   `pkg:sola`; `solactl open` on it; the kit's `opens()`, `open` method and
   `sola_kit::open`; Browser, Paint, Calendar answer `open`; Mail claims
   `mailto` and opens a compose window from it; Sola's link clicks and the
   Browser's unknown schemes, downloads and the Oath app's home link go
   through it.
2. **Choosing.** Settings → Default Apps; `request_default` and the shell's
   prompt.

## Not doing

- Desktop-environment detection, a MIME database (`shared-mime-info`),
  content sniffing, D-Bus portals (`org.freedesktop.portal.OpenURI`).
- `$BROWSER` for the desk: most tools fall back to `xdg-open`; Projects'
  panes take tmux's environment, set elsewhere.
- Opening on this desk from a shell on another computer.
