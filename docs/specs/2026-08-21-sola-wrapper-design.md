# sola-wrapper — websites as first-class Sola apps

**Date:** 2026-08-21  
**Status:** landed on master (2026-08-25); Edit / links / notify / huddle smoked 2026-08-29  
**Related:** [CEF port](2026-05-04-cef-port-design.md); [browser profiles](2026-08-10-sola-browser-profiles-design.md); [Applications list-detail](2026-08-05-sola-settings-applications-list-detail-design.md); [notifications](2026-08-25-sola-notifications-design.md)

| | |
|--|--|
| **Implementation** | Crate `crates/sola-wrapper`. Argv `sola-wrapper <id>`. Settings Applications can create/edit a wrapper (`kind` + `url`); command is synthesized. CEF via `sola-browser` (`default-features = false`) plus `profiles::bind_external` so cookies are not under the browser root. Edit menu (⌘X/C/V/A) via shell `MenuAction` + iced clipboard. Web `Notification` → `Topic::AppNotification` with the wrapper `app_id`. Off-site popups → `sola_core::open_url`. Same-site / `about:blank` NEW_POPUP is a windowless CEF tab (Slack huddle). `getUserMedia` → kit Allow / Block (`media.json`) + Chromium `MEDIASTREAM_*` content settings. |
| **Dogfood** | **On master.** Slack paints. Operator: [`manual/sola-wrapper.md`](../manual/sola-wrapper.md). Edit, off-site links, desk notifications, huddle OSR + mic **smoked** 2026-08-29 (`wrapper` debug). LifeCam huddle camera **smoked** 2026-09-01. |
| **Gaps** | No Bitwarden / downloads / tab chrome; no throwaway `--url`; no PWA manifest install. |

## Intent

Wrap a website so it is a real Sola app in the launcher and switcher — own label, icon, Wayland `app_id`, and login — instead of shipping vendor Linux clients (example: Slack). One binary, many configured instances.

This is **not** sola-browser. No tab strip, omnibox, profiles menu, vault, downloads panel, or tab groups.

## Locked

| Topic | Choice |
|-------|--------|
| Crate / binary | `sola-wrapper` |
| UI | Iced + sola-kit. No WebView, Electron, or apocrypha GTK. |
| Engine | CEF via existing sola-browser work. Do **not** enable `accelerated_osr`. |
| Settings | Enhance Applications (list + detail). No parallel “web apps” page. |
| Identity | Argv is **`sola-wrapper <id>`**, not a raw URL. URL lives on the Application record. |
| `app_id` | The configured id (`slack`), **not** a single `sola-wrapper` for every site. Command remains `/opt/sola/bin/sola-wrapper slack`. |
| Schema | Optional fields on `sola_core::Application` (`kind`, `url`) with `#[serde(default)]` so old YAML still loads. Fields are always serialized (postcard bus payloads cannot skip them). No second sticky topic. |
| State | Per-id durable CEF profile, isolated from sola-browser and from other wrappers. |
| Process | One iced chrome + one `--engine` helper **per wrapper id**. Slack and Discord do not share `root_cache_path`. |
| Single-instance | A second `sola-wrapper slack` raises the existing Slack window. |
| Chrome | Minimal kit CSD / float chrome + the page. |

## Argv

```text
sola-wrapper <id>                         # chrome
sola-wrapper --engine --profile=<id>      # headless CEF helper (spawned by this binary)
```

- `<id>` is the Application `app_id` (filesystem-safe; no `/`, `\`, or `..`).
- CEF renderer/GPU/utility workers are the same binary via `CefEngine::dispatch_subprocess` (must run first in `main`).
- `sola-wrapper --url https://…` is **not v1**. Identity must stay stable for profiles, launcher catalog, switcher icons, and session restore.

## Catalog

Source of truth is bus `Topic::Application` (persistent, keyed by `app_id`), written by Settings.

Launch lookup:

1. `~/.config/sola/state.yaml` `Application:` sequence (same file the bus host persists).
2. Fail with a stderr hint to add the id under Settings → Applications.

If the bus is down, (1) is still the last durable catalog. Live sticky replay is not required to start.

For wrapper entries, Settings synthesizes:

```text
command = /opt/sola/bin/sola-wrapper <app_id>
kind    = wrapper
url     = https://…
```

Launcher/session keep spawning `command`. Builtins in sola-shell stay user-uneditable; wrappers are user apps.

## Profile paths

```text
~/.config/sola/wrapper/<id>/          # durable CEF user-data
  cef/                                # root_cache_path (cookies, localStorage)
$XDG_CACHE_HOME/sola/wrapper/<id>/    # discardable cache
$XDG_RUNTIME_DIR/sola/wrapper/<id>.sock   # chrome singleton
```

Never `browser_data_root()` (`~/.local/share/sola/browser/…`).

## CEF reuse

Depend on `sola-browser` as a **library** with `default-features = false` (no Bitwarden).

`CefEngine::spawn` already launches `current_exe() --engine --profile=<id>`. Wrappers bind that id with `profiles::bind_external` so:

- `list()` returns only this id (no prewarm of browser profiles)
- engine sockets live under the wrapper data dir
- `root_cache_path` is `…/wrapper/<id>/cef`

Extraction of a `sola-cef` crate is **not** this slice. If bind_external + lib dep fights later (process globals, helper reap), extract then — do not fork a second CEF crate.

## Navigation

Load the configured URL. Same-window navigation stays in the wrapper (SPA, SSO redirects in the main frame).

**Outbound links (locked 2026-08-28):** `target=_blank`, `window.open`, and ⌘-click are CEF popups (native window cancelled). The wrapper has no tabs. http(s) to a **different registrable domain** than the start URL opens in **sola-browser** via [`sola_core::open_url`](../../crates/sola-core/src/open_url.rs) (same path as terminal / mail / `solactl open`). Same-site popups stay in-app (cookie / SPA). `javascript:`, `mailto:`, `about:`, `data:` are ignored.

Main-frame navigations are **not** cancelled (Google SSO in the Slack window must keep working).

## Media (huddles)

Slack huddles are `window.open('about:blank')` + `document.write` (NEW_POPUP). Cancelling that popup makes `window.open` return null and the huddle button does nothing. Same-site / `about:blank` / NEW_POPUP is allowed as a **windowless CEF browser**; the wrapper paints that tab until it closes. Off-site http(s) still go to sola-browser.

CEF OSR has no Chromium permission bubble; Alloy default-denies `getUserMedia`. The wrapper (and sola-browser) intercept `OnRequestMediaAccessPermission` and mic/camera `OnShowPermissionPrompt`, show the kit Allow / Block overlay, and persist at `…/wrapper/<id>/media.json` (browser: `profiles/<uuid>/media.json`). Allow grants the requested capture bits (mic and/or camera and/or screen) **and** writes Chromium `MEDIASTREAM_CAMERA` / `MEDIASTREAM_MIC` content settings for that origin so `permissions.query` matches. Playback does not prompt.

## Settings UX

Applications detail:

- Existing fields: `app_id`, `label`, `icon`
- **Web wrapper** checkbox (kit `form_row` + checkbox)
- When checked: **URL** field; command is a muted synthesized caption, not an editor
- When unchecked: **command** field as today

Icon picking stays pack name or filesystem path.

## Non-goals (v1)

Bitwarden fill, tab groups, sharing cookies with sola-browser, PWA manifest install, multiple windows per id, vendor Slack `.deb`, throwaway URL windows.
