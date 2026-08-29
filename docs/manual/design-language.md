# Sola design language

**Status:** living reference  
**Audience:** humans redesigning sola-kit / sola-shell, and agents (Grok Build) implementing visual work  
**Related:** `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md`, `docs/specs/2026-06-06-shell-customization-design.md`, Apple [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/)

---

## 1. North star

**Sola is a cool graphite tool UI** — dense chrome, sparse cyan signal, quiet
selection — with **macOS Dark Mode density and hierarchy** as craft reference,
not a pixel-perfect HIG clone.

Seed atoms live in `sola-kit::theme::hex::*` and `sola-core` `Palette::seed`
(must stay in sync). Dogfood kit chrome in the storybook, not in external mocks.

We borrow Apple’s *structural* ideas — materials, elevation steps, menu bar
roles, control density, type roles — and diverge where Sola’s windowing model
or the graphite brand requires it.

### Explicit departures (for now)

| Area | macOS default | Sola |
|------|---------------|------|
| Palette | System greys | **Cool graphite** (`#0c0e12` / `#151922` / …) |
| Window layout | Freeform windows with title bars | **New windows float** at app size; **zoning** is opt-in snap |
| Zoned (tiled) windows | Title bar + traffic lights | **No title bars** on zoned windows |
| Floating windows | System/CSD title bars | **Client decorations** when floating (`Topic::WindowFloating`) |
| Primary controls | Flat system blue | Soft accent fill, **dark label**, optional glow |
| Hairlines | Solid system separator | Soft **white@α** edges |
| Everything else | HIG dark mode density | Follow density; prefer graphite tokens |

Further departures may be added later as real usage demands them. Do not invent
stylistic uniqueness for its own sake beyond the graphite brand.

---

## 2. What “graphite tool UI” means here

Craft rules distilled from HIG density + the sola-kit design system pass.

### 2.1 Appearance and depth

- **Dark graphite for all chrome** — menu bar, menus, launcher, switcher, popovers, kit apps. Content areas may stay dark or follow the app; shell chrome is always dark-first.
- **Elevation through background steps *and soft materials***, not heavy borders. Base → raised → hover/selected. Soft hairlines (opaque sRGB white-mix ~7% on the surface) separate layers when needed.
- **Cards and primary fills are not flat slabs.** Kit chrome uses iced **linear gradients** (`style::card_fill`, `primary_fill`, `hero_fill`, `stage_fill`, `canvas_ambient`) so panels read as lit graphite.
- **Foreground content must stay readable.** Prefer cool off-white primary text over dim-on-dim.
- **Accent is for selection, focus, and live status** — not large filled regions of shell chrome. One filled primary per control group.

### 2.2 Materials and what iced can do

CSS comps use tools iced only partially has. Map intentionally:

| Comp technique | Iced 0.14 | Kit approach |
|----------------|-----------|--------------|
| `linear-gradient(...)` | **Yes** — `Background::Gradient(Linear)` | `style::linear_bg` / `card_fill` / `primary_fill` / … |
| Soft top highlight on cards / primaries | Yes (vertical linear) | bake into card / primary styles |
| Dual **radial** ambient wash on canvas | **No** (radial TBD upstream) | multi-stop **linear** approx (`canvas_ambient`) |
| `backdrop-filter: blur` materials | **No** | opaque raised greys / soft mixes on base |
| Layered multi-background on one node | One fill per container | stack containers or bake into one gradient |
| Inset box-shadow (rim light) | Shadow is outer only | slightly lighter gradient start stop |
| `color-mix(... transparent)` edges | Alpha inflates on borders | **opaque** `mix_white(surface, t)` / `mix(a, b, t)` |

Materials (menu bar, menus, sidebars, HUD-style overlays) still *aim* for blur + translucent fills when the compositor supports them. Until then:

- Prefer **gradient lift + graphite steps** over flat `#151922` everywhere.
- Prefer **behind-window** blend for floating shell chrome when possible; opaque fallbacks stay graphite, not pure black.
- Avoid decorative multi-hue marketing gradients. Soft accent/selection washes only.

Shell-specific alpha already lives in bus tokens (`shell-menubar-bg`, `shell-backdrop-dim`, `shell-switcher-bg`, …). Prefer those over hard-coded RGBA in views.

### 2.3 Layout density

macOS desktop chrome is **dense and quiet**:

- Menu bar: compact height, small type, generous horizontal scanning, low visual noise.
- Menus: tight row height, clear separators, standard checkmarks / key equivalents alignment.
- Overlays (Spotlight-like launcher, Mission Control–like switcher): one clear focus, large hit targets for primary rows, restrained decoration.

Prefer **status density over hierarchy theater**. Shell is not a marketing site: no hero headers, no oversized cards, no staggered entrance animations.

### 2.4 Typography

Follow a **role system**, not ad-hoc sizes:

| Role | Use | Sola mapping |
|------|-----|----------------|
| Chrome | Menu bar labels, menu items, menubar status/clock | `fonts::chrome()` / kit UI roles |
| Body UI | Settings rows, dialogs, lists | `fonts::ui()` / `ui_medium()` — kit `text::body` **13** |
| Prose | Mail bodies, long-form reading | kit `text::prose` **14** + `prose` letter renderer |
| Display | Rare emphasis (app title in menubar can be medium weight) | `fonts::display()` sparingly — `heading` 22 / `subheading` 15 |
| Caption | Help, secondary labels | `text::caption` **11** |
| Mono / data | Code, terminal, detail-panel numbers | `fonts::mono()` — `text::code` 12 |

Control pads: `PAD_CONTROL` `[7, 14]`, `PAD_CONTROL_SM` `[5, 11]`; field
inputs default to padding `[7, 12]`. Radii: SM **5** / MD **7** / LG **10**.
Prefer `button::labeled` / `labeled_sm` over inventing pads.

macOS uses SF Pro for UI and SF Mono for data. **Menubar is all chrome** —
status values and the clock match menu titles (regular weight); only the
focused-app name uses medium. Reserve mono for code and dense detail
panels, not menu-bar extras. **Do not introduce extra display faces** for
shell chrome. Max two UI faces + mono.

### 2.5 Color roles (semantic, not raw hex in views)

All UI color should resolve through the kit / bus theme:

| Intent | Behavior | Kit / bus direction (seed) |
|--------|----------|----------------------------|
| Window / canvas | Darkest graphite | `bg` / `bg-primary` `#0c0e12` |
| Raised (sidebar, card, menu) | One step up | `bg_raised` / `bg-secondary` `#151922` |
| Hover / selected row | Further lift; selection is graphite, not darkened neon | `bg_hover` `#1e2533`, selection `#2c333e` |
| Soft hairlines | White@α separators | kit `hairline` / `hairline_strong` |
| Hard edges | Stronger chrome | `border` `#2a3344` |
| Primary label | Cool off-white | `fg` / `text-primary` `#e9ecf2` |
| Secondary label | Muted blue-grey | `fg_muted` `#8b94a8` |
| Accent | Sparse: focus, primary, key status | `accent` `#3dd6f5` |
| Success / warning / danger | Soft semantic | `#3ecf8e` / `#e8b84a` / `#f07178` |

**Do not invent new hex literals in shell views** when a token exists or should exist. If a color is shell-specific (alpha materials), add a `shell-*` token rather than a one-off.

### 2.6 Controls and components

macOS controls are familiar: restrained radius, clear disabled state, quiet hover, obvious selected/active.

For Sola:

- Prefer **kit components** (`button`, `field` / `form_row`, `sidebar`,
  `popover`, `toolbar`, `text`, `card`, …) over one-off widgets.
- Use **`button::labeled` / `labeled_sm`** and named pads (`PAD_CONTROL`,
  `PAD_CONTROL_SM`) rather than ad-hoc padding.
- Ghost buttons: muted text at rest; hover = grey lift + full fg (no cyan
  wash). One primary (filled accent, dark label) per control group.
- Secondary: soft fill + strong hairline — not a bare outline.
- Field / letter text selection uses the quiet `selection` atom
  (graphite lift, not darkened neon), not `primary.weak`.
- Terminal / Workspaces **grid** selection overlays the neon `accent`
  (`#3dd6f5`) at alpha — same chroma as focus, not the graphite atom
  and not a darkened-cyan mix.
- Cards: soft hairline + light shadow; use `card::plain` when elevation is
  enough without a border.
- Badges: soft tone@α fills + matching borders (not solid slabs).
- States to always consider: **default, hover, active/selected, disabled, empty, error**.
- Popovers and menus: **tools, not hero cards** — compact padding, no oversized titles.
- Radius: SM/MD/LG at 5/7/10 — consistent, not “web card” 16–24px everywhere.

### 2.7 Motion

- Short, functional open/close for menus, launcher, switcher.
- No staggered list reveals, bounce, or decorative motion in shell chrome.
- Respect “less is more”: motion that aids orientation is fine; motion that sells is not.

### 2.8 Content and voice

- Labels name **user concepts** (Quit Terminal, Wi‑Fi), not internal IDs.
- Active voice on actions (“Save”, not “Submit”).
- Errors say what failed and what to do next; no apologies.
- Empty states invite the next action.

---

## 3. Windowing model (the real product difference)

This is the main intentional break from macOS chrome.

### Default float + opt-in zoning

- **New windows without a zone assignment float** at the **client-requested size** (centered by the compositor). The shell emits `Topic::WindowFloating` so kit apps know to draw CSD (titlebar / drag / close).
- **Zoning is opt-in** (Meta+numpad snaps). A saved zone assignment still restores on relaunch; explicit float (`Meta`+numpad `*`) persists `Zone::Float` + float geometry.
- **Zoned windows have no title bars.** App content meets the zone edge. Window identity and controls live in the **menu bar**, switcher, and floating chrome — not in per-window title bars for tiled clients.
- **Floating windows draw client decorations** (kit `titlebar` / `floating_frame` when floating). Mental model: **float = CSD + app size; zoned = no title bar + zone frame**.

### Implications for visual design

- Do not design “traffic light” clusters into zoned client chrome.
- Floating kit apps **must** honor `Topic::WindowFloating` and show title chrome when floating.
- Shell must make **focused app and window** obvious for zoned clients without title bars (menu bar app name, switcher).
- Launcher / switcher carry more weight as navigation affordances than on stock macOS.

---

## 4. Shell surface guide

Use this when restyling or reviewing shell UI. Default comparison target: **macOS dark mode equivalent**.

| Surface | macOS analogue | Sola notes |
|---------|----------------|------------|
| Menubar | Menu bar | Left: system + app menus. Right: status (stats, clock). Compact, quiet, high scanability. |
| App menus | Menu bar menus | Standard hierarchy, separators, key equivalents. |
| Launcher | Spotlight | Single focused field + results list; dimmed backdrop; not a dashboard. |
| Switcher | App / window switcher | MRU, keyboard-first; translucent backplate via shell tokens. |
| Stat indicators | Menu bar extras | Separate items (CPU, GPU, MEM, RX, TX) like other status items — not stacked dual-line widgets. |
| Stat / calendar popovers | Menu bar dropdowns | Anchored under indicator; compact detail, not marketing cards. |
| Bluetooth | Menu bar extras (Control Center-ish) | Quiet lucide glyph left of stats; popover is the same Menu overlay (`Panel::Bluetooth`). Off vs on on the icon. Not a Waybar module. [freeze](../specs/2026-08-29-shell-bluetooth-menubar-design.md). |
| Toasts (whispers) | Menu-bar status | Transient 13pt chrome in the 28px bar. `Opening…`, screenshot path. Not for attention. |
| Notifications | Banners + Notification Center | Desk cards that drop from the bar; missed pile in the right cluster. See [notifications freeze](../specs/2026-08-25-sola-notifications-design.md). |

---

## 5. Implementation anchors (do not reinvent)

Visual work should flow through existing machinery:

1. **Compile-time kit atoms** — `sola_kit::theme::hex` defaults  
2. **Bus theme** — `Topic::Theme`, palette tokens, presets  
3. **Shell tokens** — `shell-*` group (menubar, backdrop, switcher, launcher)  
4. **Font roles** — installed from theme; components call role accessors only  
5. **Components** — `sola-kit` widgets + storybook pages as the regression surface  
6. **Storybook** — dogfood every kit visual change  

Order of work for redesigns:

1. Adjust tokens / roles / component styles  
2. Propagate through kit  
3. Restyle shell surfaces against kit  
4. Only then add new components (when reuse is impossible)

Avoid permanent snowflake colors and paddings in `sola-shell` views.

---

## 6. Working with agents (Grok Build and others)

Agents have no taste. They fill gaps. This document, screenshots, and tight prompts are the constraints.

### 6.1 Taste triangle

Every design pass needs:

1. **This design language** (system + north star)  
2. **Aesthetic constraints** (section 7)  
3. **References** — screenshots of current Sola + optional macOS reference captures  

### 6.2 Prompt pattern

Bad:

> Make the menubar nicer.

Good:

> Menubar right cluster: keep CPU/GPU/MEM/RX/TX/clock as separate kit-style indicators. Match macOS menu bar density (compact type, quiet labels). Prefer shell/kit tokens; no new hex. Do not change Metric or bus protocol. Plan first, then implement.

Always pin for Sola work:

- Stack: **iced + sola-kit only** (no WebView for shell/kit)  
- **Tokens first**  
- **Reuse kit components**  
- **Storybook** for kit-side changes  
- **One surface per pass** unless explicitly scoped broader  

### 6.3 Screenshot loop

Verification for Sola is not a web browser loop:

1. Capture current surface(s) (idle menubar, open menu, launcher, switcher, storybook).  
2. Agree a short visual plan (keep / change / forbidden).  
3. Implement in a **worktree**.  
4. `cargo make build` (user installs and runs from TTY).  
5. New screenshots → critique against this doc and macOS reference.  
6. Commit when the pass is good; start the next pass clean.

Attach multiple states, not one hero image.

### 6.4 Pass sizing

One signature move per pass:

| Pass examples | Out of scope for that pass |
|---------------|----------------------------|
| Menubar density + type | Launcher redesign |
| Launcher card + list hierarchy | Theme atom overhaul |
| Switcher tiles | New font families |
| Kit button/field density | Shell layout rewrites |

### 6.5 Plan before diff

For visual work, prefer plan mode or an explicit “brief only, no code” turn so direction is approved before files change.

---

## 7. Constraints and anti-patterns

### Do

- Match macOS dark mode hierarchy, density, and control calm  
- Use semantic tokens and font roles  
- Keep shell quieter than app content  
- Treat accent as sparse signal  
- Prefer materials/translucency where the stack allows  
- Design for keyboard-first switcher/launcher  

### Don’t

- Invent a “unique Sola brand look” beyond the listed departures  
- Purple gradients, Inter-only stacks, oversized radii, glass marketing cards  
- Hard-coded colors/spacing in shell when tokens exist  
- Stacked dual-line menubar widgets where separate status items will do  
- Title bars on zoned windows  
- Landing-page hierarchy (hero, numbered marketing steps) in shell chrome  
- Decorative motion  

### Review checklist (use on every visual PR)

- [ ] Would this read as reasonable macOS dark mode chrome to a Mac user?  
- [ ] Departures only for zoning / no title bars (or newly documented)?  
- [ ] Colors from kit/bus/shell tokens?  
- [ ] Type uses role accessors, not raw families?  
- [ ] Spacing on a small consistent scale?  
- [ ] States: hover / selected / disabled / empty considered?  
- [ ] Storybook updated if kit changed?  
- [ ] No new snowflake styles in shell views?  

---

## 8. Tunability

The north star is macOS dark mode, but **presets and tokens** are how it stays personal:

- Theme presets (`Topic::Theme`) for palette  
- Shell tokens for menubar/switcher/launcher chrome  
- Font role selections for family swaps (license permitting)  

When a hard-coded look blocks tuning, **promote it to a token** rather than leaving a magic number in a view.

Future departures from macOS should be **written into §1** of this document when they become intentional — not introduced silently in code.

---

## 9. Suggested redesign order

When executing visual polish against this language (P0–P8 of the macOS L&F
roadmap are done):

1. ~~**Tokens & type baseline**~~ — done (P2).  
2. ~~**Menubar**~~ — done (P3).  
3. ~~**Menus & popovers**~~ — done (P4).  
4. ~~**Launcher**~~ — done (P5).  
5. ~~**Switcher**~~ — done (P6).  
6. ~~**Kit controls**~~ — done (P7; storybook is the regression surface).  
7. ~~**Settings / other kit apps**~~ — done (P8; inherit kit helpers, no per-app themes).  

---

## 10. References

### Product / in-repo

- **macOS L&F roadmap (phased, screenshot-first):** `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md`  
- **Screenshot capture plan (P0 tooling):** `docs/specs/2026-07-20-screenshot-capture-plan.md`  
- Theme protocol: `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md`  
- Shell customization tokens: `docs/specs/2026-06-06-shell-customization-design.md`  
- Shell iced port: `docs/specs/2026-05-22-sola-shell-iced-port-design.md`  
- Kit theme implementation: `crates/sola-kit/src/theme.rs`  
- Shell style extraction: kit `ShellStyle` / storybook Shell page  

### Apple HIG (authoritative external)

Use these when resolving ambiguity — Sola’s answer should usually match:

- [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/)  
- [Dark Mode](https://developer.apple.com/design/human-interface-guidelines/dark-mode)  
- [Color](https://developer.apple.com/design/human-interface-guidelines/color)  
- [Materials](https://developer.apple.com/design/human-interface-guidelines/materials)  
- [Layout](https://developer.apple.com/design/human-interface-guidelines/layout)  
- [Menus](https://developer.apple.com/design/human-interface-guidelines/menus)  
- [The menu bar](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar)  
- [Windows](https://developer.apple.com/design/human-interface-guidelines/windows)  

### Agent workflow (general)

- System first, screens second  
- Screenshot compare loops  
- `DESIGN.md`-style durable visual rules (this file)  
- Small passes, commit good baselines, revert bad ones  

---

## 11. One-line summary

**Look like macOS Dark Mode; tile without title bars; put every other visual decision through kit tokens and this document.**
