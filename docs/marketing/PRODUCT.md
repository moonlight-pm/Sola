# sola.moonlight.pm — public project page

<!-- impeccable:product-schema 1 -->

> Site messaging only. Desktop product truth stays in root
> [`PRODUCT.md`](../../PRODUCT.md).

## Platform

web

## Stack

**Thoxa HTTP container** in this repo at [`site/`](../../site/) (moved
from Thoxa's `workloads/sola` on 2026-10-19): JSX pages,
`std/http/server`, Debian image, Wicket workload `sola` on aulos
(`wl-f2835fc5634e`, image `registry.wicket.cloud/sola-landing:latest`,
host `sola.moonlight.pm`). Build and deploy steps:
[`site/README.md`](../../site/README.md).

`sola.computer` is **parked** (2026-10-19): no route, no apex A record.
`*.sola.computer` stays for `bot.sola.computer` (Bots phone API).

## Users

**Primary:** Linux / NixOS developers deciding whether Sola is real and
worth building. Technical; expects honest status and real paths.

**Secondary:** colleagues and friends already running it.

## Product Purpose

A project page, not a campaign. Same family as thoxa.moonlight.pm and
oath.moonlight.pm: what it is, design, what exists, how it is built,
status with the gaps named, how to build it.

## Pages (as built 2026-10-19)

The bar is the flower and **Sola**, then **Manual** and **Source**, as on
thoxa.moonlight.pm and oath.moonlight.pm.

`/manual` is [`docs/manual`](../manual/README.md) rendered as one numbered
page with a contents column; its README holds the chapter order and the
Markdown subset. The manual is about the system — setup, architecture, each
core component, solactl in full — and lists the applications rather than
documenting them.

`/`:

1. **Intro** — flower + "Sola"; one-paragraph definition; facts line
   (Pre-release · NixOS, x86-64 · No releases yet · Manual · Source); then
   the desk from `design/foundation/07-desk.html`, rendered to
   `site/web/desk.png` and captioned as a drawing.
2. **Design** — four bold-lead paragraphs: one toolkit + live theme;
   separate restartable processes; native, not web (CEF only where pages
   are the content); scriptable (`solactl`, `solactl mcp`), with a short
   command block.
3. **Applications** — table: name (linked to its manual chapter when it
   has one), one factual line, crate. Shell stable; apps early and
   incomplete (links the manual and capabilities).
4. **Architecture** — process table (`sola`, bus, call, river bridge,
   shell, session, kit).
5. **Status** — pre-release, one workstation, no images; "Not there yet"
   list.
6. **Building** — NixOS module, `cargo make install-cef` / `install`,
   link to CONTRIBUTING.

## Voice

Plain declarative sentences, as in the Thoxa and Oath pages. Say what a
thing is and does. No slogans, eyebrows, pills, "coming soon", or promises
of future features. Name gaps instead of hedging.

## Do not write

- Roadmap promises (the old teaser's "Omarchy plugins — coming")
- Invented users, customers, benchmarks, or performance numbers
- "Download" or "install" calls to action before an image exists
- Internal hostnames, private paths, or desk screenshots without asking

## Brand commitments

- Visual law: root [`DESIGN.md`](../../DESIGN.md) — graphite ladder, one
  cyan signal (here: the pre-release ring, link underlines, focus), flower
  mark in cyan.
- Fonts: SF Pro Text → Inter; Iosevka Term Slab → JetBrains Mono (Google
  Fonts; the CSP allows only those).
- Dark only (the product is dark).

## Open decisions

- A real screenshot of the desk on the page (needs Joshua to pick a clean
  one).
- When an install image exists: where it is hosted and how the page links
  it.
