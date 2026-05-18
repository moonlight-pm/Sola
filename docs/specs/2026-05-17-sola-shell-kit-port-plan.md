# sola-shell → sola-kit Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Each task is dispatched to a fresh subagent with full context. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/specs/2026-05-17-sola-shell-kit-port-design.md` — read it before starting.

**Goal:** Port all four `sola-shell` surfaces (menubar, launcher, menu, switcher) from `sola-app` (GTK4/WebKit6) to `sola-kit` (CEF/Remix v3) in a single milestone, plus the small additive kit extension that makes multi-window kit apps possible.

**Architecture:** sola-shell stays one process with one `ShellApp` Rust struct. Four windows, each with its own `Main` Remix v3 root and its own asset bundle. Kit gains `WindowConfig::root_component` (per-window TSX root) and `WindowConfig::initial_state` (per-window seed JSON injected as `window.__solaInitial`).

**Tech stack:** Rust, sola-kit, sola-bus, sola-core, Remix v3 (`@remix-run/ui`), CEF (`cef` 147.1.0).

**Branch policy:** Joshua explicitly authorized direct-to-`master` commits for this milestone. No worktree, no PR. CLAUDE.md's worktree rule is waived for this port only.

**Install policy (still in force):** `cargo make install` requires explicit per-call user permission. Subagents MUST NOT run `cargo make install` or `cargo make install <app>`. `cargo make build` is fine and required for verification.

---

## Pre-Task Notes for Every Subagent

- Use Serena's symbolic tools for code reads/edits (per workspace CLAUDE.md). Don't `cat` or `Read` whole code files when an overview + targeted `find_symbol` suffices.
- After every task, commit with a conventional-commit-prefixed message ending with `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>`. No `--no-verify`, no `--amend`. Stage specific files, not `git add .`.
- After every code change, `cargo make build` must succeed (workspace-wide). If it fails, fix or escalate — do not commit broken builds.
- Do NOT run sola or any `/opt/sola/bin/*` binary; the user is the only one who launches sola.

---

## File Structure (target state after all tasks complete)

```
crates/sola-kit/
  src/
    window.rs            # WindowConfig adds: root_component, initial_state
    lib.rs               # build_importmap takes root path; inject_kit_head emits __solaInitial
    ctx.rs               # add_window threads per-window root + initial_state into inject_kit_head
  web/
    lib/
      index.tsx          # reads window.__solaInitial, passes as <Main initial={…}/>
crates/sola-shell/
  Cargo.toml             # sola-kit replaces sola-app + GTK + WebKit
  src/
    app.rs               # SolaApp impl + ShellApp state, retargeted to kit; ≤ ~500 LOC
    keys.rs              # unchanged
    zoning.rs            # unchanged
    menubar/{mod,assets}.rs      # mod.rs: setup_menubar + open_menu/close_menu helpers
    launcher/{mod,assets,state}.rs  # mod.rs absorbs open_launcher/close_launcher/launch_and_close/render_launcher
    menu/{mod,assets,state}.rs      # mod.rs absorbs open_menu/close_menu helpers (where appropriate)
    switcher/{mod,assets,state}.rs  # mod.rs absorbs switcher render helpers
  web/
    assets/                       # unchanged (flower.svg, pillars.svg)
    menubar.tsx                   # exports Main; mounts <Menubar/>
    launcher.tsx                  # exports Main; mounts <Launcher/>
    menu.tsx                      # exports Main; mounts <Menu/>
    switcher.tsx                  # exports Main; mounts <Switcher/>
    components/
      menubar/{menubar,app-title,tray}.tsx + menubar.css
      launcher/{launcher,app-row}.tsx + launcher.css
      menu/{menu,menu-item}.tsx + menu.css
      switcher/{switcher,switcher-card}.tsx + switcher.css
    tsconfig.json                 # mirrors kit's; allowImportingTsExtensions, jsx react-jsx, jsxImportSource @remix-run/ui
```

Deleted at end: `web/index.html`, `web/launcher.html`, `web/menu.html`, `web/overlay.html`, `web/src/*.ts`.

---

## IPC Contract Inventory (extracted from current shell)

**Inbound (JS → Rust via `invoke(cmd, args)`):**

| Window  | Command      | Args shape                                   |
|---------|--------------|----------------------------------------------|
| menubar | `open_menu`  | `{ source: string, index: u64, anchor_x: f64 }` |
| menubar | `close_menu` | `{}`                                         |
| menu    | `dismiss`    | `{}`                                         |
| menu    | `action`     | `{ app_id: string, action_id: string }`     |
| switcher | `select`    | `{ index: u64 }`                             |
| launcher | `query`     | `{ text: string }`                           |
| launcher | `nav`       | `{ dir: "up"\|"down" }` OR `{ index: u64 }` |
| launcher | `launch`    | `{ app_id?: string }`                        |
| launcher | `close`     | `{}`                                         |

**Outbound (Rust → JS via `window.send_to_js(&Value)` — `__solaRecv` envelopes):**

| Target window | Event name      | Envelope shape                                         |
|---------------|-----------------|--------------------------------------------------------|
| menubar       | `focus`         | `{ event: "focus", app_name: string, menu_labels: string[] }` |
| menubar       | `close_menu`    | `{ event: "close_menu" }`                              |
| menubar       | `toast`         | `{ event: "toast", message: string }`                  |
| menu          | `show`          | `{ event: "show", items: MenuItem[], anchor_x: f64 }`  |
| menu          | `clear`         | `{ event: "clear" }`                                   |
| launcher      | `reset`         | `{ event: "reset" }`                                   |
| launcher      | `render`        | `{ event: "render", apps: AppEntry[], selected: u64 }` |
| switcher      | `render`        | `{ event: "render", apps: AppEntry[], selected: u64 }` |

All outbound now use `send_to_js(&json!({…}))`. The current code mixes `send_to_js` (menubar) with `eval_js(format!("…"))` (menu/launcher/switcher); the port unifies on `send_to_js`.

---

## Task 1: Kit — add WindowConfig fields and plumb defaults

**Files:**
- Modify: `crates/sola-kit/src/window.rs` (WindowConfig struct + defaults)
- Modify: `crates/sola-monitor/src/app.rs` (existing add_window call — add new fields as `None`)
- Modify: `crates/sola-settings/src/app.rs` (existing add_window call — add new fields as `None`)
- Modify: `crates/sola-kit/src/components/popover.rs` (if it constructs a WindowConfig — check via `grep -n "WindowConfig" crates/sola-kit/src/`)

- [ ] **Step 1: Read kit WindowConfig**

```bash
# Use Serena (per workspace CLAUDE.md):
mcp__plugin_serena_serena__find_symbol name_path_pattern=WindowConfig relative_path=crates/sola-kit/src/window.rs include_body=true
```

Note current field set. There is no `Default` impl today — every call site uses explicit struct init.

- [ ] **Step 2: Add the two new fields**

Use Serena's `replace_symbol_body` on `WindowConfig`:

```rust
pub struct WindowConfig {
    pub title: String,
    pub size: (i32, i32),
    pub position: Option<(i32, i32)>,
    pub decorated: bool,
    pub transparent: bool,
    pub assets: &'static AssetBundle,
    pub zoned: bool,
    pub keyboard_target: bool,
    /// Per-window override for the Remix v3 root component path. When
    /// `None`, the kit falls back to `SolaApp::ROOT_COMPONENT`. The
    /// referenced file is served under `app://` from this window's
    /// asset bundle and must export a `Main` factory.
    pub root_component: Option<&'static str>,
    /// Per-window seed state. When `Some`, the kit injects
    /// `<script>window.__solaInitial = <json>;</script>` into the head
    /// of this window's `index.html`. The kit's `index.tsx` reads it
    /// synchronously and passes it as the `initial` prop to `Main`.
    /// When `None`, `window.__solaInitial` is set to `null`.
    pub initial_state: Option<serde_json::Value>,
}
```

Make sure `serde_json` is in `Cargo.toml` for sola-kit (it almost certainly is — check `grep serde_json crates/sola-kit/Cargo.toml`; add `serde_json = "1"` if missing).

- [ ] **Step 3: Update existing call sites**

Find all current `WindowConfig {` constructions across the workspace:

```bash
grep -rn "WindowConfig\s*{" crates/ --include="*.rs"
```

Add the two new fields to each (always `root_component: None, initial_state: None,` — these are existing single-window kit apps that should behave identically):

- `crates/sola-monitor/src/app.rs:36`
- `crates/sola-settings/src/app.rs:108`
- Any other site `grep` reveals.

- [ ] **Step 4: Verify build**

```bash
cargo make build
```

Expected: clean build of entire workspace. Existing kit apps unchanged in behavior.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/window.rs crates/sola-monitor/src/app.rs crates/sola-settings/src/app.rs
git commit -m "$(cat <<'EOF'
feat(sola-kit): add WindowConfig root_component + initial_state

Two optional per-window overrides that prepare the kit for multi-window
apps. Existing single-window callers (monitor, settings) get None for
both; behavior is unchanged. Per-window plumbing into the importmap
and HTML head injection lands in the following tasks.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Kit — per-window root_component (importmap rewrite)

**Files:**
- Modify: `crates/sola-kit/src/lib.rs` (`inject_kit_head` signature + body)
- Modify: `crates/sola-kit/src/ctx.rs` (`add_window` passes per-window root)

- [ ] **Step 1: Read current inject_kit_head + ctx.add_window**

```bash
mcp__plugin_serena_serena__find_symbol name_path_pattern=inject_kit_head relative_path=crates/sola-kit/src/lib.rs include_body=true
mcp__plugin_serena_serena__find_symbol name_path_pattern=AppCtx/add_window relative_path=crates/sola-kit/src/ctx.rs include_body=true
```

Confirm current call: `crate::inject_kit_head(&html_raw, self.root_component, cfg.assets)` in ctx.rs:83.

- [ ] **Step 2: Change inject_kit_head + caller to use per-window root**

In `ctx.rs::add_window`, replace the existing call:

```rust
// Before:
let html = crate::inject_kit_head(&html_raw, self.root_component, cfg.assets);

// After:
let root = cfg.root_component.unwrap_or(self.root_component);
let html = crate::inject_kit_head(&html_raw, root, cfg.assets);
```

`inject_kit_head`'s signature doesn't change — it already takes a `&str` root. The change is purely at the call site: per-window override wins, app-wide constant is the fallback.

- [ ] **Step 3: Verify build**

```bash
cargo make build
```

Expected: clean build. Existing apps still use `root_component: None` so behavior unchanged.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/ctx.rs
git commit -m "$(cat <<'EOF'
feat(sola-kit): per-window root_component override

WindowConfig::root_component, when Some, overrides the app's
ROOT_COMPONENT for that one window's importmap. Lets a single SolaApp
host multiple windows with different Main components — the shape
sola-shell needs (menubar, launcher, menu, switcher all in one app).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Kit — initial_state via window.__solaInitial + Main props

**Files:**
- Modify: `crates/sola-kit/src/lib.rs` (`inject_kit_head` — emit `<script>window.__solaInitial = …;</script>` before the importmap)
- Modify: `crates/sola-kit/src/ctx.rs` (`add_window` — pass `cfg.initial_state` into the head-injection step)
- Modify: `crates/sola-kit/web/lib/index.tsx` (read `__solaInitial`, pass to `<Main initial={…} />`)
- Modify: `crates/sola-monitor/web/main.tsx` (accept `initial` prop with default of null — should be a no-op for monitor)
- Modify: `crates/sola-settings/web/main.tsx` (same)
- Modify (docs): `crates/sola-kit/CLAUDE.md` if present, else the top-level workspace `Sola/CLAUDE.md`'s sola-kit section — add a paragraph documenting the multi-window pattern

- [ ] **Step 1: Read current state**

```bash
mcp__plugin_serena_serena__find_symbol name_path_pattern=inject_kit_head relative_path=crates/sola-kit/src/lib.rs include_body=true
# Already-read: crates/sola-kit/web/lib/index.tsx
# Already-read: crates/sola-kit/src/ctx.rs::add_window
```

- [ ] **Step 2: Change inject_kit_head to optionally emit __solaInitial**

Update the function signature to accept the initial state, and emit a script tag before the importmap in the injected head:

```rust
pub(crate) fn inject_kit_head(
    html: &str,
    root_component: &str,
    assets: &'static AssetBundle,
    initial_state: Option<&serde_json::Value>,
) -> String {
    let importmap = build_importmap(root_component);
    let css_links = kit_css_links(assets);
    let initial_json = match initial_state {
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    // Order matters: __solaInitial must be set before index.tsx imports
    // run, because Main reads it synchronously.
    let injection = format!(
        "<script>window.__solaInitial = {initial_json};</script>\n\
         {importmap}\n\
         {BOOTSTRAP_SCRIPT}\n\
         {css_links}\n"
    );
    inject_before_head_close(html, &injection)
}
```

(Exact ordering of `BOOTSTRAP_SCRIPT`, `importmap`, `css_links` should match what's there today — keep that ordering, just insert the `__solaInitial` script first.)

- [ ] **Step 3: Update the ctx.rs call site**

```rust
let html = crate::inject_kit_head(
    &html_raw,
    root,
    cfg.assets,
    cfg.initial_state.as_ref(),
);
```

- [ ] **Step 4: Update kit's index.tsx**

```tsx
import { createRoot } from "@remix-run/ui";
import { setupKit } from "@sola/kit";
import { Main } from "@sola/app-root";

declare global {
  interface Window {
    __solaInitial: unknown;
  }
}

setupKit();
const initial = window.__solaInitial ?? null;
createRoot(document.body).render(<Main initial={initial} />);
```

- [ ] **Step 5: Update monitor + settings Main signatures**

Add an `initial` field to their Main props type. They currently ignore it — that's fine. Example:

```tsx
// crates/sola-monitor/web/main.tsx — current signature is something like:
// export function Main(handle: Handle<{}>) { … }
// After:
export function Main(handle: Handle<{ initial: unknown }>) {
  // body unchanged — monitor ignores `initial`
  …
}
```

Confirm both monitor + settings still compile their TSX (errors will show at build time when swc strips types).

- [ ] **Step 6: Document the pattern**

Append to the kit CLAUDE.md section (workspace root `Sola/CLAUDE.md`, under "Web Frontends: Remix v3 (sola-kit)"):

```markdown
### Multi-window apps

A single `SolaApp` can host multiple windows with different root
components and per-window seed state. On each `ctx.add_window(cfg)`:

- `cfg.root_component: Option<&'static str>` overrides
  `SolaApp::ROOT_COMPONENT` for that window's importmap entry of
  `@sola/app-root`. Lets one app mount different `Main` components
  per window (e.g. sola-shell's menubar/launcher/menu/switcher).
- `cfg.initial_state: Option<serde_json::Value>` is serialized into
  `<script>window.__solaInitial = <json>;</script>` and injected into
  the head of that window's `index.html`. The kit's `index.tsx` reads
  it synchronously and passes it to `Main` via the `initial` prop.
  `None` becomes `null`.

`Main`'s signature must accept the prop:
`function Main(handle: Handle<{ initial: T | null }>)`.
```

- [ ] **Step 7: Verify build**

```bash
cargo make build
```

Expected: clean workspace build.

- [ ] **Step 8: Commit**

```bash
git add crates/sola-kit/src/lib.rs crates/sola-kit/src/ctx.rs crates/sola-kit/web/lib/index.tsx \
        crates/sola-monitor/web/main.tsx crates/sola-settings/web/main.tsx CLAUDE.md
git commit -m "$(cat <<'EOF'
feat(sola-kit): per-window initial_state via window.__solaInitial

inject_kit_head now emits a <script> setting window.__solaInitial to
the per-window seed JSON (or null) before the importmap. The kit's
index.tsx reads it synchronously and passes it to <Main initial={…}/>.
Monitor + settings update their Main signatures to accept (and ignore)
the prop. Documented in the workspace CLAUDE.md.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Kit — unit test for build_importmap + manual regression check

**Files:**
- Modify: `crates/sola-kit/src/lib.rs` (add `#[cfg(test)] mod importmap_tests`)

- [ ] **Step 1: Read build_importmap**

```bash
mcp__plugin_serena_serena__find_symbol name_path_pattern=build_importmap relative_path=crates/sola-kit/src/lib.rs include_body=true
```

- [ ] **Step 2: Add unit test**

Insert at end of `lib.rs`:

```rust
#[cfg(test)]
mod importmap_tests {
    use super::build_importmap;

    #[test]
    fn importmap_resolves_app_root_to_given_path() {
        let im = build_importmap("/menubar.tsx");
        // The importmap is a <script type="importmap">…</script> block.
        // Just check the substring — exact JSON shape isn't load-bearing
        // for this test, only that the per-window root flows through.
        assert!(
            im.contains("\"@sola/app-root\": \"app:///menubar.tsx\""),
            "expected app-root to map to /menubar.tsx, got:\n{im}"
        );
    }

    #[test]
    fn importmap_default_root_path_works() {
        let im = build_importmap("/main.tsx");
        assert!(im.contains("\"@sola/app-root\": \"app:///main.tsx\""));
    }
}
```

If the actual importmap entry format differs (e.g. it doesn't prefix `app://` or uses a different key), adjust the assertion to match what `build_importmap` actually emits. The point is: **per-window root must flow through to the importmap**.

- [ ] **Step 3: Run the test**

```bash
cargo test -p sola-kit importmap_tests
```

Expected: 2/2 pass.

- [ ] **Step 4: Verify full workspace build still clean**

```bash
cargo make build
```

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/lib.rs
git commit -m "$(cat <<'EOF'
test(sola-kit): cover per-window root_component in importmap

Pins the behavior introduced in the previous two commits: build_importmap
must resolve @sola/app-root to whatever root path it's given, so a
multi-window app can mount different Main components per window.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Shell — Cargo swap + ShellApp retarget + placeholder Mains

**Files:**
- Modify: `crates/sola-shell/Cargo.toml`
- Modify: `crates/sola-shell/src/app.rs` (sola_app → sola_kit; per-window `root_component` + `initial_state` added)
- Modify: `crates/sola-shell/src/menubar/mod.rs`
- Create: `crates/sola-shell/src/menubar/assets.rs` (probably already partial — check `ls crates/sola-shell/src/menubar/`)
- Modify: `crates/sola-shell/src/launcher/mod.rs`
- Modify: `crates/sola-shell/src/menu/mod.rs`
- Modify: `crates/sola-shell/src/switcher/mod.rs`
- Modify: `crates/sola-shell/src/launcher/assets.rs`, `crates/sola-shell/src/menu/assets.rs`, `crates/sola-shell/src/switcher/assets.rs` (existing — confirm they use kit's `asset_bundle!` macro shape)
- Create: `crates/sola-shell/web/menubar.tsx` (placeholder Main)
- Create: `crates/sola-shell/web/launcher.tsx` (placeholder Main)
- Create: `crates/sola-shell/web/menu.tsx` (placeholder Main)
- Create: `crates/sola-shell/web/switcher.tsx` (placeholder Main)
- Create: `crates/sola-shell/web/tsconfig.json` (mirror of `crates/sola-kit/web/tsconfig.json`)

This is the largest task. Subagent should expect ~30–60 min. Do NOT delete the old `web/*.html` or `web/src/*.ts` yet — they're orphaned by the asset-bundle changes but live alongside the new TSX until Task 10's sweep. (Keeping them around makes the intermediate build state easier to compare.)

- [ ] **Step 1: Cargo.toml swap**

Replace shell's `[dependencies]` block:

```toml
[dependencies]
sola-kit = { path = "../../crates/sola-kit" }
sola-bus = { path = "../../crates/sola-bus" }
sola-core = { path = "../../crates/sola-core" }
tracing = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Drop: `sola-app`, `gtk4`, `gdk4`, `glib`, `gio`, `webkit6`.

- [ ] **Step 2: Replace sola_app imports**

```bash
grep -rn "sola_app\|sola-app" crates/sola-shell/src/ --include="*.rs"
```

For each hit, change `sola_app::` → `sola_kit::`. The exported types are the same names (`AppCtx`, `AppRuntime`, `BusRegistry`, `SolaApp`, `WindowConfig`, `WindowHandle`).

Specifically `crates/sola-shell/src/app.rs` line 7 changes from
`use sola_app::{AppCtx, AppRuntime, BusRegistry, SolaApp, WindowConfig, WindowHandle};` to
`use sola_kit::{AppCtx, AppRuntime, BusRegistry, SolaApp, WindowConfig, WindowHandle};`.

- [ ] **Step 3: Add the new WindowConfig fields to every shell add_window call**

Four call sites in `crates/sola-shell/src/app.rs` (today, lines 78/80/92/104 area — find them via `grep -n "ctx.add_window\|setup_menubar" crates/sola-shell/src/app.rs`):

For each, add `root_component` (per-window TSX path) and `initial_state` (`None` for now — Task 6+ will populate per surface):

```rust
// menubar (in crates/sola-shell/src/menubar/mod.rs::setup_menubar):
ctx.add_window(WindowConfig {
    title: "menubar".into(),
    size: (1920, zoning::MENUBAR_HEIGHT),
    position: Some((0, 0)),
    decorated: false,
    transparent: true,
    assets: MENUBAR_ASSETS,
    zoned: false,
    keyboard_target: true,
    root_component: Some("/menubar.tsx"),
    initial_state: None,
})
```

(Note: kit's `WindowConfig` does NOT have `initial_state` as a legacy field — Task 1 added it. The legacy `sola-app::WindowConfig` had a different `initial_state` shape; that's gone with the dependency swap. There's no need to preserve any legacy `initial_state` usage from before this port.)

Same for launcher (`/launcher.tsx`), menu (`/menu.tsx`), switcher (`/switcher.tsx`). Use the today's `size`/`position`/`decorated`/`transparent`/`assets`/`zoned`/`keyboard_target` values exactly as they are today — geometry doesn't change in this port.

- [ ] **Step 4: Check + create asset bundles**

For menubar: today's `setup_menubar` references `MENUBAR_ASSETS` from `menubar/mod.rs`. Check whether `menubar/assets.rs` exists:

```bash
ls crates/sola-shell/src/menubar/
```

If not, create `crates/sola-shell/src/menubar/assets.rs` mirroring the shape of `crates/sola-shell/src/launcher/assets.rs`. The bundle must include `menubar.tsx`, `components/menubar/**`, and any other static files that menubar needs. Use the kit's `asset_bundle!` macro (see `crates/sola-kit/src/assets.rs` and other apps' assets.rs for examples).

For launcher/menu/switcher: their `assets.rs` files exist (`ls crates/sola-shell/src/{launcher,menu,switcher}/assets.rs`). Confirm they use kit's `asset_bundle!` (post-swap they need to; kit and sola-app may have different macros). Update if needed.

- [ ] **Step 5: Create placeholder Main TSX files**

For each of the four surface roots, create a minimal Remix v3 placeholder:

```tsx
// crates/sola-shell/web/menubar.tsx
import { type Handle } from "@remix-run/ui";

export function Main(handle: Handle<{ initial: unknown }>) {
  return () => (
    <div style="background: #181818; color: #fff; font-family: sans-serif; padding: 4px 12px;">
      sola-shell · menubar (placeholder)
    </div>
  );
}
```

Same pattern for `launcher.tsx`, `menu.tsx`, `switcher.tsx` — different label text in each.

- [ ] **Step 6: Add web/tsconfig.json**

Copy from `crates/sola-kit/web/tsconfig.json`. Adjust paths if needed so that `@sola/*` resolves through the kit (mirror what monitor or settings does — check `crates/sola-monitor/web/` for the tsconfig if present, or use the kit's directly).

- [ ] **Step 7: Build**

```bash
cargo make build
```

Expected: clean build. If the swap missed an import or a WindowConfig field, fix and rebuild.

- [ ] **Step 8: Verify shell binary still has all four surface windows**

This is a code-review check; do NOT launch sola.

```bash
mcp__plugin_serena_serena__find_symbol name_path_pattern=ShellApp/new relative_path=crates/sola-shell/src/app.rs include_body=true
```

Confirm: four `add_window` calls, each with a distinct `root_component`, each binding into `ShellWindows` (menubar/launcher/menu/switcher).

- [ ] **Step 9: Commit**

```bash
git add crates/sola-shell/Cargo.toml crates/sola-shell/src/app.rs \
        crates/sola-shell/src/menubar/ crates/sola-shell/src/launcher/mod.rs \
        crates/sola-shell/src/menu/mod.rs crates/sola-shell/src/switcher/mod.rs \
        crates/sola-shell/src/launcher/assets.rs crates/sola-shell/src/menu/assets.rs \
        crates/sola-shell/src/switcher/assets.rs \
        crates/sola-shell/web/menubar.tsx crates/sola-shell/web/launcher.tsx \
        crates/sola-shell/web/menu.tsx crates/sola-shell/web/switcher.tsx \
        crates/sola-shell/web/tsconfig.json
git commit -m "$(cat <<'EOF'
refactor(sola-shell): scaffold port to sola-kit

Swaps sola-app + GTK + WebKit deps for sola-kit. ShellApp keeps its
shape (focused window/MRU/applications/menu cache/zoning state); only
the window handle type changes. All four windows (menubar, launcher,
menu, switcher) construct via kit's WindowConfig with per-window
root_component pointing at placeholder Main components. Old web/*.html
+ web/src/*.ts are left in place for the per-surface ports to replace.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Port menubar

**Files:**
- Modify: `crates/sola-shell/src/menubar/mod.rs` (initial_state seed; helper functions for state push)
- Modify: `crates/sola-shell/src/app.rs` (`on_js_command` menubar arms unchanged in shape but call into menubar/mod helpers; existing `send_to_js` envelope shape unchanged)
- Replace: `crates/sola-shell/web/menubar.tsx` (real implementation)
- Create: `crates/sola-shell/web/components/menubar/menubar.tsx`
- Create: `crates/sola-shell/web/components/menubar/app-title.tsx`
- Create: `crates/sola-shell/web/components/menubar/tray.tsx`
- Create: `crates/sola-shell/web/components/menubar/menubar.css`
- Modify: `crates/sola-core/src/theme.rs` (add menubar component bindings to the default theme)

**Reference:** legacy behavior lives in `crates/sola-shell/web/src/menubar.ts` and `crates/sola-shell/web/index.html` (CSS + DOM). Read both before authoring the TSX so you reproduce the existing look + interactions.

**Contract** (from IPC inventory):
- Inbound: `invoke("open_menu", { source, index, anchor_x })`, `invoke("close_menu", {})`.
- Outbound: `{event:"focus", app_name, menu_labels}`, `{event:"close_menu"}`, `{event:"toast", message}`.
- Initial state: `{ focused: { app_name: string, menu_labels: string[] } | null }`.

- [ ] **Step 1: Read legacy menubar behavior**

```bash
# Use the Read tool on these — they're TS/HTML, not Rust:
# - crates/sola-shell/web/src/menubar.ts
# - crates/sola-shell/web/index.html (CSS + DOM markup)
```

Note the menu-label hover-vs-click behavior, focused-app rendering, clock placement, and toast presentation.

- [ ] **Step 2: Write components**

Authoring guidance — for each:

- **`components/menubar/menubar.tsx`** (`<Menubar>`): root layout (flex row); composes `<AppTitle/>` left, `<Tray/>` right. Holds focused-app state in a closure (mutated on receipt of `focus` and `close_menu` envelopes via `recv` from `@sola/ipc`). Calls `handle.update()` after state mutations.
- **`components/menubar/app-title.tsx`** (`<AppTitle>`): renders the focused app name and a list of menu labels. Each label: `mix={[on("click", () => invoke("open_menu", { source: "click", index, anchor_x: e.clientX }))]}` (the click handler will need a `mouseEvent`-capturing wrapper — see `@sola/kit`'s `on` for typing). Hover behavior matches today's `hoverMenu` — only emits open_menu while another menu is already open, to allow cursor traversal.
- **`components/menubar/tray.tsx`** (`<Tray>`): clock (use `setInterval` in component init; clean up on unmount via Remix's facilities), eventual toast slot.
- **`components/menubar/menubar.css`**: class-based selectors referencing `var(--sola-menubar-*)`. Match the look in today's `index.html` `<style>` block — same colors, paddings, font sizing. Use the existing palette tokens (`--bg-primary`, `--text-primary`, etc.) only through the scoped `--sola-menubar-*` vars.

Use `@sola/kit`'s `on` (not `@remix-run/ui`'s) for all event handlers per workspace CLAUDE.md.

**`crates/sola-shell/web/menubar.tsx`** becomes:

```tsx
import { type Handle } from "@remix-run/ui";
import { Menubar } from "./components/menubar/menubar";

interface MenubarInitial {
  focused: { app_name: string; menu_labels: string[] } | null;
}

export function Main(handle: Handle<{ initial: MenubarInitial | null }>) {
  const initial = handle.props.initial ?? { focused: null };
  return () => <Menubar initial={initial} />;
}
```

- [ ] **Step 3: Wire Rust side — seed initial_state**

In `crates/sola-shell/src/menubar/mod.rs::setup_menubar`, populate `initial_state`:

```rust
pub fn setup_menubar(ctx: &mut AppCtx, initial: serde_json::Value) -> WindowHandle {
    ctx.add_window(WindowConfig {
        title: "menubar".into(),
        // … existing fields …
        root_component: Some("/menubar.tsx"),
        initial_state: Some(initial),
    })
}
```

Caller (in `app.rs::ShellApp::new`) computes the initial focus snapshot at construction time:

```rust
let menubar_initial = serde_json::json!({ "focused": null });
let menubar = setup_menubar(ctx, menubar_initial);
```

No focused app exists at shell startup, so `null` is correct. Subsequent updates flow through the existing `send_to_js` calls (already an `{event:"focus", …}` envelope shape — no change needed on the Rust side beyond the four cleanups in Step 4).

- [ ] **Step 4: Rust envelope cleanup**

The four existing menubar `send_to_js(json!({…}))` call sites are already envelopes — keep them. Only verify there are no remaining `eval_js(format!("…"))` calls targeting the menubar window:

```bash
grep -n "menubar.eval_js\|windows.menubar.eval_js" crates/sola-shell/src/
```

Expected: none. If any exist, convert them to `send_to_js(&json!({event: "…", …}))`.

- [ ] **Step 5: Theme — add menubar bindings to default**

Read `crates/sola-core/src/theme.rs::Theme::default` (or wherever the default theme is built) and the `2026-05-07-sidebar-and-theme-protocol-design.md` spec for the binding shape. Add component bindings for `"menubar"` declaring the slots used in `menubar.css`. Use the existing palette tokens for the bindings — pick the same atoms today's menubar CSS uses.

Example slot names (adjust to what `menubar.css` actually references):
- `bg`, `fg`, `border-bottom`, `app-title-fg`, `menu-label-fg`, `menu-label-hover-bg`, `clock-fg`.

- [ ] **Step 6: Build**

```bash
cargo make build
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/menubar/ crates/sola-shell/src/app.rs \
        crates/sola-shell/web/menubar.tsx crates/sola-shell/web/components/menubar/ \
        crates/sola-core/src/theme.rs
git commit -m "$(cat <<'EOF'
feat(sola-shell): port menubar to sola-kit/Remix v3

<Menubar> root composes <AppTitle> and <Tray>; receives focus +
close_menu + toast envelopes through @sola/ipc's recv; emits
open_menu/close_menu via invoke. Initial focus state is seeded
through WindowConfig::initial_state. Theme bindings for the menubar
component land in the default theme.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Port launcher

**Files:**
- Modify: `crates/sola-shell/src/launcher/mod.rs` (`setup_launcher`, initial_state seed; helpers)
- Modify: `crates/sola-shell/src/app.rs` (replace `eval_js("resetForOpen()")` and `eval_js(format!("renderApps({}, {})", …))` with `send_to_js` envelopes; per-surface methods may move to `launcher/mod.rs` opportunistically — full app.rs decomposition is Task 10)
- Replace: `crates/sola-shell/web/launcher.tsx` (real implementation)
- Create: `crates/sola-shell/web/components/launcher/launcher.tsx`
- Create: `crates/sola-shell/web/components/launcher/app-row.tsx`
- Create: `crates/sola-shell/web/components/launcher/launcher.css`
- Modify: `crates/sola-core/src/theme.rs` (launcher bindings)

**Reference:** legacy `crates/sola-shell/web/src/launcher.ts` + `crates/sola-shell/web/launcher.html`.

**Contract:**
- Inbound: `invoke("query", { text })`, `invoke("nav", { dir: "up"|"down" } | { index })`, `invoke("launch", { app_id? })`, `invoke("close", {})`.
- Outbound: `{event:"reset"}`, `{event:"render", apps: [{app_id, label, icon?}], selected: u64}`.
- Initial: `{ apps: AppEntry[], selected: u64, query: string }` (typically `{ [], 0, "" }` at startup).

- [ ] **Step 1: Read legacy launcher behavior** (Read the .ts and .html).

- [ ] **Step 2: Author TSX components**

- **`launcher.tsx`**: search input at top (Type Tab focused on open), result list below, escape closes. Holds local input text in component state, fires `invoke("query", { text })` on input change (debounced if today does so — preserve that). Arrow up/down call `invoke("nav", { dir })`. Click on a row calls `invoke("launch", { app_id })`. Enter on input calls `invoke("launch", {})` (no app_id = use current selection).
- **`app-row.tsx`** (`<AppRow>`): one row with icon + label, selected-class on hover/keyboard-selected.
- **`launcher.css`**: match today's launcher look. Reference `var(--sola-launcher-*)` only.

`crates/sola-shell/web/launcher.tsx` is the same shape as menubar — pulls `Launcher` from components and seeds it with initial.

- [ ] **Step 3: Rust envelope cleanup**

In `app.rs::open_launcher` and `app.rs::render_launcher` (and `app.rs::close_launcher` if it pushes anything), replace `eval_js(...)` with `send_to_js`:

```rust
// Before:
self.windows.launcher.eval_js("resetForOpen()");
// After:
self.windows.launcher.send_to_js(&serde_json::json!({"event": "reset"}));

// Before:
let json = launcher::state::render_json(&self.applications, &self.launcher.filtered_ids);
let js = format!("renderApps({}, {})", json, self.launcher.selected);
self.windows.launcher.eval_js(&js);
// After:
let apps: serde_json::Value =
    launcher::state::render_value(&self.applications, &self.launcher.filtered_ids);
self.windows.launcher.send_to_js(&serde_json::json!({
    "event": "render",
    "apps": apps,
    "selected": self.launcher.selected,
}));
```

If `launcher::state::render_json` only returns a `String`, add a `render_value` sibling that returns a `serde_json::Value` directly. Keep `render_json` if other callers need it; remove if not.

- [ ] **Step 4: Seed initial_state in setup_launcher**

```rust
let launcher_initial = serde_json::json!({ "apps": [], "selected": 0, "query": "" });
let launcher = setup_launcher(ctx, launcher_initial);
```

- [ ] **Step 5: Theme — add launcher bindings to default**

Open `crates/sola-core/src/theme.rs` and find where the default `Theme` is constructed (the function `Theme::default()` or a `default_theme()` builder — confirm via Serena's `get_symbols_overview`). Refer to the protocol spec at `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md` for the exact binding shape if anything is unclear.

Add component bindings for `"launcher"`:

- For each `--sola-launcher-<slot>` referenced in `launcher.css`, declare a slot in the launcher's `ComponentBindings`, point it at an existing palette token, and ensure the token's eligible-selection-groups list contains the slot's selection group.
- Run `Theme::validate()` mentally (or via an existing test, if present) to confirm every binding's token exists and its group/kind is consistent.

Pick palette tokens that match the legacy launcher's appearance (read the `<style>` block in the legacy `launcher.html` for the original color/spacing choices, then map to the closest existing palette atoms).

- [ ] **Step 6: Build**

```bash
cargo make build
```

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/launcher/ crates/sola-shell/src/app.rs \
        crates/sola-shell/web/launcher.tsx crates/sola-shell/web/components/launcher/ \
        crates/sola-core/src/theme.rs
git commit -m "$(cat <<'EOF'
feat(sola-shell): port launcher to sola-kit/Remix v3

<Launcher> root composes a search input and a list of <AppRow>s.
Inbound query/nav/launch/close envelopes via @sola/kit's invoke;
outbound reset + render envelopes via send_to_js. Replaces the
legacy eval_js(format!("renderApps(...)")) pattern with typed JSON
envelopes. Launcher theme bindings added to the default theme.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Port menu

**Files:**
- Modify: `crates/sola-shell/src/menu/mod.rs` (`setup_menu`, initial_state)
- Modify: `crates/sola-shell/src/app.rs` (replace `eval_js("showMenu({}, {})")` and `eval_js("clearMenu()")` with `send_to_js` envelopes)
- Replace: `crates/sola-shell/web/menu.tsx`
- Create: `crates/sola-shell/web/components/menu/menu.tsx`
- Create: `crates/sola-shell/web/components/menu/menu-item.tsx`
- Create: `crates/sola-shell/web/components/menu/menu.css`
- Modify: `crates/sola-core/src/theme.rs` (menu bindings)

**Reference:** legacy `crates/sola-shell/web/src/menu.ts` + `crates/sola-shell/web/menu.html`.

**Contract:**
- Inbound: `invoke("dismiss", {})`, `invoke("action", { app_id, action_id })`.
- Outbound: `{event:"show", items, anchor_x}`, `{event:"clear"}`.
- Initial: `{ visible: false }`.

- [ ] **Step 1: Read legacy menu behavior**.

- [ ] **Step 2: Author TSX components**

- **`menu.tsx`** (`<Menu>`): conditionally-rendered dropdown positioned at `anchor_x`. Holds `items + anchor_x` state, mutated by `show` / `clear` envelopes via `recv`. Click outside → `invoke("dismiss", {})`; click item → `invoke("action", { app_id, action_id })`.
- **`menu-item.tsx`** (`<MenuItem>`): one row; supports divider type vs label type. Shortcut hint rendered when present. Disabled rows are non-interactive.
- **`menu.css`**: matches today's dropdown look (border, shadow, hover bg, divider line). Reference `var(--sola-menu-*)` only.

- [ ] **Step 3: Rust envelope cleanup**

In `app.rs::open_menu`:

```rust
// Before:
let json = serde_json::to_string(&items).unwrap_or_default();
self.windows.menu.eval_js(&format!("showMenu({}, {})", json, anchor_x));
// After:
self.windows.menu.send_to_js(&serde_json::json!({
    "event": "show",
    "items": items,
    "anchor_x": anchor_x,
}));
```

In `app.rs::close_menu`:

```rust
// Before:
self.windows.menu.eval_js("clearMenu()");
// After:
self.windows.menu.send_to_js(&serde_json::json!({"event": "clear"}));
```

- [ ] **Step 4: Seed initial_state**

```rust
let menu_initial = serde_json::json!({ "visible": false });
```

- [ ] **Step 5: Theme — add menu bindings to default**

Open `crates/sola-core/src/theme.rs`. Add component bindings for `"menu"`:

- For each `--sola-menu-<slot>` referenced in `menu.css`, declare a slot in the menu's `ComponentBindings`, point it at an existing palette token, and ensure the token's eligible-selection-groups list contains the slot's selection group.
- Refer to `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md` for the binding shape.

Pick palette tokens matching the legacy menu's appearance (read the `<style>` block in the legacy `menu.html` for the original colors).

- [ ] **Step 6: Build**

```bash
cargo make build
```

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/menu/ crates/sola-shell/src/app.rs \
        crates/sola-shell/web/menu.tsx crates/sola-shell/web/components/menu/ \
        crates/sola-core/src/theme.rs
git commit -m "$(cat <<'EOF'
feat(sola-shell): port menu to sola-kit/Remix v3

<Menu> dropdown rendered at the requested anchor_x; receives show +
clear envelopes via @sola/ipc's recv; emits dismiss + action via
invoke. Replaces the legacy eval_js("showMenu(...)") + eval_js("clearMenu()")
calls with typed envelopes. Menu theme bindings added to the default theme.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Port switcher

**Files:**
- Modify: `crates/sola-shell/src/switcher/mod.rs` (`setup_switcher`, initial_state)
- Modify: `crates/sola-shell/src/app.rs` (replace `eval_js("renderSwitcher({}, {})")` with `send_to_js` envelopes)
- Replace: `crates/sola-shell/web/switcher.tsx`
- Create: `crates/sola-shell/web/components/switcher/switcher.tsx`
- Create: `crates/sola-shell/web/components/switcher/switcher-card.tsx`
- Create: `crates/sola-shell/web/components/switcher/switcher.css`
- Modify: `crates/sola-core/src/theme.rs` (switcher bindings)

**Reference:** legacy `crates/sola-shell/web/src/overlay.ts` + `crates/sola-shell/web/overlay.html`.

**Contract:**
- Inbound: `invoke("select", { index })`.
- Outbound: `{event:"render", apps: [{app_id, label, icon?}], selected: u64}`.
- Initial: `{ visible: false, apps: [], selected: 0 }`.

- [ ] **Step 1: Read legacy switcher behavior**.

- [ ] **Step 2: Author TSX components**

- **`switcher.tsx`** (`<Switcher>`): full-screen overlay (`position: fixed; inset: 0; background: rgba(0,0,0,0.5);`), centered horizontal strip of `<SwitcherCard>`s. Holds `apps + selected` state, mutated by `render` envelope. Mouse-enter on a card → `invoke("select", { index })`. Card click → `invoke("select", { index })` (Rust handles confirm-on-release elsewhere — don't try to replicate that in JS).
- **`switcher-card.tsx`** (`<SwitcherCard>`): one app preview tile; selected-class when `index === selected`.
- **`switcher.css`**: matches today's overlay (semi-transparent dark backdrop, card grid styling, selected-ring).

- [ ] **Step 3: Rust envelope cleanup**

In `app.rs::on_windows` and any other site that calls `eval_js(format!("renderSwitcher(...)"))`:

```rust
// Before:
let json = self.switcher_apps_json();
self.windows.switcher.eval_js(&format!(
    "renderSwitcher({}, {})", json, self.switcher.selected,
));
// After:
let apps = self.switcher_apps_value(); // returns serde_json::Value
self.windows.switcher.send_to_js(&serde_json::json!({
    "event": "render",
    "apps": apps,
    "selected": self.switcher.selected,
}));
```

If `switcher_apps_json` returns a `String`, add a `switcher_apps_value` returning `serde_json::Value`. Drop `switcher_apps_json` if it has no other callers.

- [ ] **Step 4: Seed initial_state**

```rust
let switcher_initial = serde_json::json!({ "visible": false, "apps": [], "selected": 0 });
```

- [ ] **Step 5: Theme — add switcher bindings to default**

Open `crates/sola-core/src/theme.rs`. Add component bindings for `"switcher"`:

- For each `--sola-switcher-<slot>` referenced in `switcher.css`, declare a slot in the switcher's `ComponentBindings`, point it at an existing palette token, and ensure the token's eligible-selection-groups list contains the slot's selection group.
- Refer to `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md` for the binding shape.

Pick palette tokens matching the legacy switcher's appearance (read the `<style>` block in the legacy `overlay.html` for the original colors — typically a semi-transparent dark backdrop and a selected-ring color).

- [ ] **Step 6: Build**

```bash
cargo make build
```

- [ ] **Step 7: Commit**

```bash
git add crates/sola-shell/src/switcher/ crates/sola-shell/src/app.rs \
        crates/sola-shell/web/switcher.tsx crates/sola-shell/web/components/switcher/ \
        crates/sola-core/src/theme.rs
git commit -m "$(cat <<'EOF'
feat(sola-shell): port switcher to sola-kit/Remix v3

<Switcher> renders a full-screen overlay with a strip of <SwitcherCard>s;
receives render envelopes via @sola/ipc's recv; emits select via invoke.
Replaces the legacy eval_js("renderSwitcher(...)") with a typed JSON
envelope. Switcher theme bindings added to the default theme.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Decompose app.rs + sweep legacy web files

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (extract per-surface methods)
- Modify: `crates/sola-shell/src/menubar/mod.rs` (absorb menubar-only helpers)
- Modify: `crates/sola-shell/src/launcher/mod.rs` (absorb launcher-only helpers: `open_launcher`, `close_launcher`, `launch_and_close`, `render_launcher`)
- Modify: `crates/sola-shell/src/menu/mod.rs` (absorb menu-only helpers: `open_menu`, `close_menu` if cleanly extractable)
- Modify: `crates/sola-shell/src/switcher/mod.rs` (absorb switcher-only helpers)
- Delete: `crates/sola-shell/web/index.html`
- Delete: `crates/sola-shell/web/launcher.html`
- Delete: `crates/sola-shell/web/menu.html`
- Delete: `crates/sola-shell/web/overlay.html`
- Delete: `crates/sola-shell/web/src/menubar.ts`
- Delete: `crates/sola-shell/web/src/launcher.ts`
- Delete: `crates/sola-shell/web/src/menu.ts`
- Delete: `crates/sola-shell/web/src/overlay.ts`
- Delete: `crates/sola-shell/web/src/` (directory, once empty)

- [ ] **Step 1: app.rs decomposition strategy**

Read current `app.rs` symbol-by-symbol:

```bash
mcp__plugin_serena_serena__get_symbols_overview relative_path=crates/sola-shell/src/app.rs
```

For each `impl ShellApp` method, decide:

- **Stays in `app.rs`**: `new`, `on_focus`, `on_windows`, `on_zones`, `on_output_geometry`, `on_set_app_menu`, anything that orchestrates across surfaces or is a top-level bus handler.
- **Moves to `menubar/mod.rs`**: `clear_menubar_focus`, `push_toast` — menubar-only state pushes.
- **Moves to `launcher/mod.rs`**: `open_launcher`, `close_launcher`, `launch_and_close`, `render_launcher` — launcher-only state pushes + lifecycle.
- **Moves to `menu/mod.rs`**: `open_menu`, `close_menu` — menu-only state pushes + lifecycle.
- **Moves to `switcher/mod.rs`**: any switcher-only render helpers (`switcher_apps_value`, etc.).

These can stay as free functions taking `&mut ShellApp` + `&mut AppCtx`, OR move to an `impl ShellApp` block in the surface module's `mod.rs` (Rust allows splitting impl blocks across files via `mod` inclusion — confirm the existing crate structure already does this; if not, prefer free fns for minimum churn).

Target: `app.rs` ≤ ~500 LOC after this step (down from 1192). If it's still over 500, look for more extraction targets (e.g., key-chord handling logic in `app.rs` may belong in `keys.rs`).

- [ ] **Step 2: Apply decomposition**

Move methods one at a time, building between moves:

```bash
cargo make build
```

Catch broken imports as you go. Use Serena's `find_referencing_symbols` to find all callers before moving anything:

```bash
mcp__plugin_serena_serena__find_referencing_symbols name_path_pattern=ShellApp/open_launcher relative_path=crates/sola-shell/src/app.rs
```

Update callers to use the new path.

- [ ] **Step 3: Delete legacy web files**

```bash
rm crates/sola-shell/web/index.html
rm crates/sola-shell/web/launcher.html
rm crates/sola-shell/web/menu.html
rm crates/sola-shell/web/overlay.html
rm crates/sola-shell/web/src/menubar.ts
rm crates/sola-shell/web/src/launcher.ts
rm crates/sola-shell/web/src/menu.ts
rm crates/sola-shell/web/src/overlay.ts
rmdir crates/sola-shell/web/src 2>/dev/null || true
```

- [ ] **Step 4: Confirm no asset bundle references the deleted files**

```bash
grep -rn "index.html\|launcher.html\|menu.html\|overlay.html\|src/menubar\|src/launcher\|src/menu\|src/overlay" crates/sola-shell/
```

Expected: zero hits in source files. Hits in compiled artifacts under `target/` are fine.

- [ ] **Step 5: Final build**

```bash
cargo make build
```

Expected: clean workspace build.

- [ ] **Step 6: Spot-check app.rs size**

```bash
wc -l crates/sola-shell/src/app.rs
```

Expected: ≤ ~500. If meaningfully over (say > 700), do a brief follow-up extraction pass.

- [ ] **Step 7: Commit**

```bash
git add -A crates/sola-shell/
git commit -m "$(cat <<'EOF'
refactor(sola-shell): decompose app.rs + remove legacy web stack

Per-surface methods (open/close/render for launcher, menu, switcher;
clear_menubar_focus + push_toast for menubar) move out of app.rs into
their surface modules. app.rs is now focused on SolaApp impl,
cross-surface coordination, and top-level bus handlers. Legacy HTML
+ TS files for the WebKit-based shell are deleted — replaced by the
Remix v3 TSX components landed in Tasks 6–9.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Completion checklist (subagent-driven-development's final reviewer runs this)

After all 10 tasks land, confirm:

- [ ] `cargo make build` — clean.
- [ ] `cargo test -p sola-kit` — passes (includes the new importmap test).
- [ ] `cargo test -p sola-shell` — passes (zoning, keys, launcher::state existing tests still green).
- [ ] `grep -rn "gtk4\|gdk4\|webkit6\|sola-app\|sola_app" crates/sola-shell/` — zero hits.
- [ ] `grep -rn "eval_js" crates/sola-shell/src/` — zero hits (every site now uses typed envelopes via `send_to_js`).
- [ ] `wc -l crates/sola-shell/src/app.rs` — ≤ ~500.
- [ ] `ls crates/sola-shell/web/` — contains `assets/`, `components/`, `menubar.tsx`, `launcher.tsx`, `menu.tsx`, `switcher.tsx`, `tsconfig.json` only; no `*.html`, no `src/`.
- [ ] CLAUDE.md mentions the multi-window pattern (Task 3, Step 6).

Manual smoke after handoff (user runs, not subagent):

- menubar shows focused app + menus, clock updates, hover raises focused window.
- launcher: super-tap opens, typing filters, enter launches, escape dismisses.
- menu: menubar click opens at the right x, click outside closes, keyboard navigation works.
- switcher: super-tab cycles MRU, release confirms, escape cancels.
