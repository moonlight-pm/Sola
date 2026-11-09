# site — sola.moonlight.pm

The public site: the project page (`/`) — what Sola is, its design, the
applications, the processes of a session, status, building — and the
manual (`/manual`), which is `docs/manual/*.md` rendered as one numbered
page. Not part of the desktop.

A small Rust program in its own Cargo workspace writes it all as plain files
into `dist/`, and Cloudflare serves them as a Worker with static assets.

| Path | What |
|------|------|
| `pages/page.html` | The HTML shell: head, the bar (Manual, Source), the footer. `{{name}}` is filled in by `src/site.rs` |
| `pages/home.html` | The home page |
| `pages/privacy.html` | `/privacy`: what Calendar does with connected accounts. Google's verification reads it; every claim has to stay true of `crates/sola-calendar` and `crates/sola-settings` |
| `pages/manual.html` | The frame of `/manual`: the title and the contents column |
| `pages/missing.html` | The 404 page |
| `src/manual.rs` | Markdown to HTML (pulldown-cmark): the chapters in `CHAPTERS`, numbered; chapter links become anchors, other relative links go to GitHub; comments coloured in shell, Nix, and TOML blocks |
| `src/site.rs` | Puts the pages together; `STATIC` lists the files in `static/` |
| `src/main.rs` | Writes every file into `dist/` |
| `static/` | `style.css`, `_headers`, the flower mark, and `desk.webp` |
| `cloudflare.config.ts`, `wrangler.config.ts` | The Worker and where its files are |
| `package.json`, `package-lock.json` | Pin `cf` and the Wrangler it builds with |

The look is the desk in the home page screenshot: [`DESIGN.md`](../DESIGN.md)'s
graphite ladder tinted violet, one rose signal, contrast as DESIGN.md's.

## Keeping it true

Every claim on the home page comes from the repo. When an application, a
process, or the release state changes, update the page with
[`docs/capabilities.md`](../docs/capabilities.md) and
[`docs/architecture.md`](../docs/architecture.md). The applications and the
processes are the two `<dl>` lists in `pages/home.html`; a process links to
its chapter of the manual.

`static/desk.webp` is a screenshot of Joshua's desk (1920 × 1080, taken
2026-10-29), WebP at quality 95 (`magick shot.png -alpha off -strip
-quality 95 -define webp:method=6 desk.webp`, about 240 KB). The caption
gives its date; a new screenshot gets a new date, and the colors in
`style.css` follow it.

A new chapter goes in `CHAPTERS` in `src/manual.rs` and in the table in
[`docs/manual/README.md`](../docs/manual/README.md), which also has the
Markdown rules the manual keeps to. `cargo test` fails until the two lists
match, or if an anchor or a link on the site has no target.

## Local

```sh
cd site
cargo test
cargo run          # writes dist/
```

## Headers

`static/_headers` sets them on every response: a Content-Security-Policy
(styles and images from here, fonts from `fonts.gstatic.com` with the Google
Fonts stylesheet; no script, frames, or forms), `nosniff`, `Referrer-Policy`,
and HSTS. Images are reused for a day; pages and CSS are checked on every
load, Cloudflare's default. A page that loads something new needs the CSP
widened there.

## Deploy

[`.github/workflows/site.yml`](../.github/workflows/site.yml) tests and builds
on every pull request that touches `site/` or `docs/manual/`, and deploys on a
push to `master`, so the public manual follows `master`. To deploy by hand:
`gh workflow run site.yml`.

It reads `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` from the repo's
secrets. The public name is the repo's `SITE_DOMAIN` variable; without it the
site deploys to its `workers.dev` address only.

On Oath, build and deploy in CI: `npx cf` fails there (no `/usr/bin/env`).

## Domains

`sola.moonlight.pm` is the address. Cloudflare manages its DNS record (it shows
as `AAAA 100::`; leave it alone). `sola.computer` is parked: the apex A record
is gone and nothing serves it. `*.sola.computer` stays for `bot.sola.computer`
(the Bots phone API).
