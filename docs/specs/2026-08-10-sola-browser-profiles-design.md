# sola-browser profiles — design freeze

**Date:** 2026-08-10  
**Status:** **Frozen** — layout + Profiles menubar + chrome-bar select on
`naturalethic/cef-browser` (2026-08-13); CEF helpers; parked last-frames
for instant switch (miss blanks)  
**Branch context:** `naturalethic/cef-browser` (was `naturalethic/browser`)  
**Related:** [hardening plan](../plans/2026-08-09-sola-browser-hardening.md) P1.3;
open-questions D8.

## Intent

A **profile** is a separate **web identity + workspace**:

- Distinct cookies / site storage / engine cache (sessions that survive restart)
- Distinct **open tabs** (tabs *are* bookmarks — no classic bookmark or reading-list UI)
- **Not** a full second browser for every preference

**Runtime:** one active profile at a time (one iced window). Switch /
create / delete-active rewrites the registry. The chrome process stays
up; CEF for each profile lives in a headless helper so cookie roots stay
isolated and switch does not reload pages.

## Identity

| Field | Rule |
|-------|------|
| **id** | UUID (opaque, filesystem-safe) |
| **name** | Friendly label; default **`Primary`** |
| **active** | Exactly one active id in the registry |

## On-disk layout

### XDG data — WebKit + tabs + registry

```text
~/.local/share/sola/browser/
  profiles.json                 # registry (version, active, profiles[])
  profiles/
    <uuid>/                     # WebKit data_dir (opaque — never merge/split)
      # cookies, storage, serviceworkers, mediakeys, …
      session.json              # open tabs + active index (+ sidebar if kept)
  shared/                       # browser-wide durable data (not WebKit)
    # history, downloads index, … (as features ship)
```

### XDG cache — discardable

```text
~/.cache/sola/browser/
  profiles/
    <uuid>/                     # WebKit cache_dir
```

Deleting `~/.cache/sola/browser` (or the whole cache tree) **must recover**:
WebKit recreates cache; cookies/storage/tabs/registry live under **data**.

### XDG config — app prefs (not per-profile web identity)

Use a **directory**, not flat `browser-*.json` at the sola config root:

```text
~/.config/sola/browser/
  vault.json                    # Bitwarden chrome prefs (remember email, …) — shared
  # other browser chrome prefs as needed
```

Use `JsonConfigIn` with `APP_DIR = "browser"` (already in `sola-core`), not
root-level `browser-session.json` / `browser-vault.json`.

**Why not only `~/.config/sola/browser/` for WebKit?**  
Config is for *settings*. Cookies and large site storage belong in **share**
(and cache in **cache**). Profiles live under share/cache; chrome prefs under
config/browser/.

## What is per-profile vs shared

| Per profile | Shared (browser-wide) |
|-------------|------------------------|
| Entire WebKit **data_dir** + **cache_dir** | Preferences (theme, search, density, colors, …) |
| Open **tabs / session** (`session.json`) | **History** lookup (when shipped) |
| | **Downloads** list / default dir (when shipped) |
| | **Autofill / vault chrome** (Bitwarden unlock prefs) |
| | Zoom / chrome content prefs we store ourselves |
| | Site permissions **if** easy outside WebKit data dir; else they ride with the profile (no cross-dir copy) |

**Out of scope:** classic bookmarks, reading list, extensions (TBD/never).

**Principle:** anything in the WebKit data dir is one blob per profile. Do not
invent merge/copy of cookies or storage between profiles.

## Registry (`profiles.json`)

Minimum:

```json
{
  "version": 1,
  "active": "<uuid>",
  "profiles": [
    { "id": "<uuid>", "name": "Primary" }
  ]
}
```

## First run / cleanup (no migration)

1. If no registry: create UUID, name `Primary`, set active, create empty
   `profiles/<uuid>/` (data) and cache dir.
2. **Do not migrate** old flat WebKit trees or old session files.
3. **Delete obsolete paths** when present (one-shot cleanup):

| Path | Notes |
|------|--------|
| `~/.local/share/sola/browser/cookies.db` | Pre-profile flat jar |
| `~/.local/share/sola/browser/storage/` | |
| `~/.local/share/sola/browser/serviceworkers/` | |
| `~/.local/share/sola/browser/mediakeys/` | |
| `~/.cache/sola/browser/*` outside `profiles/` | Old flat cache |
| `~/.config/sola/browser-session.json` | → profile `session.json` |
| `~/.config/sola/browser-vault.json` | → `browser/vault.json` |
| `~/.config/sola/browser.yaml` | Legacy |
| `~/.config/sola/browser/history.yaml` | Legacy port-era |
| `~/.config/sola/browser/tabs/*.yaml` | Legacy per-tab yaml |

User re-signs into sites once after the cutover.

## Runtime (v1)

1. Load registry (or first-run init).
2. Resolve `active` profile paths.
3. Create `WebKitNetworkSession` with that profile’s data_dir + cache_dir.
4. Sandbox: allow those dirs RW for Network/Web processes (`add_path_to_sandbox`).
5. Cookie policy: accept always; ITP off for personal dogfood reliability
   (revisit if product wants stricter tracking prevention).
6. Load/save **tabs** from `profiles/<uuid>/session.json`.
7. Vault prefs from `~/.config/sola/browser/vault.json` (shared).

**Switcher UI:** Profiles menubar — list (checked active), New / Rename /
Delete… dialogs; switch keeps the window and swaps the front CEF helper.

## Implementation notes

- Prefer absolute canonical paths for engine cookie/storage dirs.
- Prefer `JsonConfigIn` for config files under `browser/`.
- Session is **data** under the profile, not config (tabs are profile state).
- Bitwarden vault remains **process-global / shared prefs**; not per web profile.
- CEF `root_cache_path` = `profiles/<uuid>/cef/` under the active data dir.

## Implementation status

| Item | Status |
|------|--------|
| Registry + Primary + UUID | **done** (`src/profiles.rs`) |
| Data/cache under `profiles/<uuid>/` | **done** |
| Session `session.json` under profile | **done** |
| Vault `~/.config/sola/browser/vault.json` | **done** |
| First-run wipe of flat/legacy paths | **done** |
| CEF user data under profile | **done** (`…/cef/`) |
| Profiles menubar switch + manage | **done** (2026-08-12) |
| One-window instant switch + per-profile CEF helpers | **done** (2026-08-12) |
| Chrome-bar kit identity select (aligned to tab column) | **done** (2026-08-12) |
| Chrome parks last composites; instant tab/profile paint | **done** (2026-08-13) |
| History / downloads under `shared/` | **not yet** |

## Gaps (explicit)

- History / downloads storage under `shared/`.
- First visit to a profile this session still opens its session tabs (later switches resume the helper).

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-10 | Multi-profile-ready layout now; one active profile; switcher later |
| 2026-08-10 | UUID + friendly name (default Primary); `profiles/<id>/` |
| 2026-08-10 | Split data/cache; cache wipe safe |
| 2026-08-10 | Registry `profiles.json` at share root |
| 2026-08-10 | No migration; delete old flat data |
| 2026-08-10 | Tabs per profile; prefs/history/downloads/autofill shared |
| 2026-08-10 | No classic bookmarks — tabs are bookmarks |
| 2026-08-10 | Shared durable non-WebKit data under `share/.../browser/shared/` |
| 2026-08-10 | Config under `~/.config/sola/browser/` (not flat `browser-*.json`) |
| 2026-08-12 | Profiles menubar + dialogs; switch/create/delete-active in one window; CEF under profile helpers |
