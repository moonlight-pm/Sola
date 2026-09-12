# sola.computer — marketing site product

<!-- impeccable:product-schema 1 -->

> Site messaging only. Desktop product truth stays in root
> [`PRODUCT.md`](../../PRODUCT.md).

## Platform

web

## Stack

**Thoxa HTTP container** in the Thoxa repo (`containers/sola`), same shape as
thoxa.dev: JSX pages, `std/http/server`, SQLite notify list, Debian image,
Wicket workload on aulos (`registry.wicket.cloud/sola-landing:latest`, host
`sola.computer`).

Design source: Paper file
[sola.computer](https://app.paper.design/file/01KZF8TSPFDJZ4APR05E2ADXBJ)
**Teaser · Desktop / Teaser · Mobile**. The longer **Landing** artboards are
not the live site.

## Users

**Primary:** Linux / NixOS builders who will install Sola and dogfood it. Technical, expects honest status, real install paths, and no vaporware.

**Secondary (not primary CTA):** colleagues and friends already in the collaborator circle; macOS-curious visitors who discover the site.

## Product Purpose

**Sola** is a Wayland desktop shell — a full compositor session and desktop environment built in Rust. It ships a multi-process desktop (process manager, event bus, River bridge, shell chrome, first-party apps) meant to feel like a cool graphite tool UI with macOS Dark Mode density as craft reference.

**sola.computer** is the public marketing site. **Live today** is a teaser:
what Sola is, four spec rows, session roster, and **notify when the ISO is
ready**. Download / NixOS install CTAs wait until media exists.

## Positioning

**What is interesting:** a mac-like Wayland desktop — menubar, launcher, switcher — where shell and first-party apps are one product (same **sola-kit**, same live theme). Omarchy plugin support is coming, first-class.

**Also true, not the lead:** multi-process + bus (restart resilience); ordinary Linux apps run without ceremony.

**Not the pitch:** gaming, Steam, or “IPC over a bus.” Those are not the open.

## Marketing messaging (copy authority)

**Audience job:** Decide whether Sola is a real desktop worth installing on NixOS / Linux — not learn the crate graph.

### Hierarchy (say only this)

1. **Mac-like Linux desktop** — menubar, launcher, switcher; sola-kit + live theme across first-party apps  
2. **Omarchy plugins** — first-class support, coming (not shipped)  
3. **Install** — teaser: notify for ISO (not yet released). Later: ISO primary; NixOS module alternate  
4. **Honest early** — dogfood, not a finished OS  

Architecture (bus, supervisor, River) is optional “how it is built” — short, never the hero.

### Landing copy (canonical)

**Eyebrow:** Wayland desktop · pure Rust  

**Headline (teaser, live):** Sola Desktop  

**Sub (teaser, live):** A mac-like desktop for Linux. First-class Omarchy plugin support is coming.

**Primary CTA (teaser, live):** Notify me — one email when the ISO is ready.  

**Aside (teaser, live):** ISO in progress · not yet released · Coming soon  

**Headline (full landing, Paper only):** One kit. The whole desktop.  

**Sub (full landing, Paper only):** Sola is a graphite Wayland shell where menubar, launcher, and first-party apps share sola-kit — the same Iced components and a theme that updates live. Omarchy plugins are coming, first-class.

**Primary CTA (full landing, not live):** Download ISO  
**Secondary CTA (full landing, not live):** NixOS install  
**Aside (full landing, not live):** x86_64 · early dogfood  

**After screenshot — Kit**  
**Title:** sola-kit  
**Body:** Buttons, fields, sidebars, cards, fonts, and theme atoms. Shell chrome and first-party apps import the same crate. Change the palette once; everything follows.

**Desktop**  
**Title:** Desktop  
**Body:** Menubar, launcher, switcher — mac-like session chrome.

**Plugins**  
**Title:** Plugins  
**Body:** Omarchy plugins — first-class support, coming.

**What you get** (compact, not a feature novel)  
Shell · Terminal · Browser · Mail · Calendar · Workspaces · Settings · Spotify · Paint · Preview · Arcade · Monitor · Scope · KVM · Wrappers  

**Install**  
**Title:** Boot the ISO. Land in Sola.  
**Body:** Installer media with a short wizard. Or enable the NixOS module if you already live on flakes.

**Footer:** Early dogfood · sola.computer  

### Do not write

- Hero about “processes” or “the bus”  
- Gaming / Steam as the open (Steam is an ordinary app; do not name it)  
- Multi-step Steam how-tos  
- Card grids that restate the same idea six ways  
- Copy invented to fill a layout slot  
- Omarchy plugins as shipped (they are coming)  

## Capabilities (marketing-honest)

- sola-kit + live theme across shell / kit apps  
- Shell: menubar, launcher, switcher; float + opt-in zoning — mac-like session  
- **Omarchy plugins** — coming, first-class (not live yet)  
- First-party apps (many partial): terminal, browser, mail, calendar, workspaces, settings, spotify, paint, preview, arcade, monitor, scope, kvm, wrappers  
- Under the hood (secondary): multi-process supervisor, sola-bus, sola-river ↔ River  
- Install: ISO (assumed for marketing) + NixOS module path  

**Honesty constraints:** early dogfood; no invented customers/benchmarks; partial apps not oversold.

## Voice

Quiet, precise, technical-friendly. Prefer product experience over infrastructure. Short sentences. No filler. Cyan is for signal, not slogans.

## Brand commitments

- **Product graphite extended:** cool graphite (`#0c0e12` / `#151922` / …), sparse cyan accent (`#3dd6f5`), same world as shell/kit scaled for web — not a separate playful brand
- Flower mark (sola flower SVG) as product mark when needed
- Domain: **sola.computer**

## Primary action (marketing)

**Live teaser:** **Notify me** (email when the ISO is ready).  
**Later (full landing):** **Download / Install** once ISO hosting exists. Secondary: NixOS module.

## Constraints

- No invented testimonials, customers, pricing, or performance numbers
- Match as-built truth; early-product framing required
- Live site is the Paper **Teaser**, not the full Landing artboard
- Do not claim ISO download until media exists

## Open decisions

- When to ship the full Landing artboard vs keep the teaser
- Exact ISO hosting URL / release channel copy when wired
- Whether download is a separate route or an anchor on landing
