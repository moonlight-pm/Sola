# site — sola.moonlight.pm

The public site: the project page (`/`) — what Sola is, its design, the
applications, the processes of a session, status, building — and the
manual (`/manual`), which is `docs/manual/*.md` rendered as one numbered
page. A Thoxa program, served from a container on Wicket. Not part of the
desktop.

| Path | What |
|------|------|
| `main.thx` | Entry: listen, open the local browser |
| `app/routes.thx` | `GET /`, `GET /manual`, static files, the 404 page; security and cache headers |
| `app/ui/page.thx` | The HTML shell: head, the bar (Manual, Source), the footer |
| `app/pages/home.thx` | The home page and the 404 page |
| `app/pages/manual.thx` | `/manual`: the chapters in `names`, a numbered contents column |
| `app/markdown.thx` | Markdown to nodes (from thoxa.moonlight.pm): chapter links become anchors, other relative links go to GitHub |
| `app/highlight.thx` | Code blocks: comments in shell, Nix, and TOML |
| `app/config.thx` | Listen port, asset root, and manual root from the environment |
| `web/` | `style.css`, the flower mark, and `desk.png` |

The look follows [`DESIGN.md`](../DESIGN.md): graphite, one cyan signal.

## Keeping it true

Every claim on the home page comes from the repo. When an application, a
process, or the release state changes, update the page with
[`docs/capabilities.md`](../docs/capabilities.md) and
[`docs/architecture.md`](../docs/architecture.md). The application list is
`appList()` and the process table `partList()` in `app/pages/home.thx`; an
application's `chapter` links its name to the manual.

`web/desk.png` is the `Desk` element (`data-name="Desk"`, 1296 × 760 CSS
px) of [`design/foundation/07-desk.html`](../design/foundation/07-desk.html),
captured at 2× in headless Chrome after `document.fonts.ready`. Render it
again when that board changes; the caption says it is a drawing.

The manual is read from disk at each request (locally) and copied into the
image, so a change to `docs/manual` reaches the site at the next deploy. A
new chapter goes in `names` in `app/pages/manual.thx` and in the table in
[`docs/manual/README.md`](../docs/manual/README.md), which also has the
Markdown subset the renderer supports.

## Local

Needs `thoxa` on `PATH` (a build of the Thoxa repo). From the repo root:

```sh
thoxa run site
# prints http://127.0.0.1:8089/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_PORT` | `8089` locally. The image sets `8080`. |
| `THOXA_STATIC` | `site/web` |
| `THOXA_MANUAL` | `docs/manual`. The image sets `/app/manual`. |

The listen port is the first of `PORT` (Wicket), `WICKET_PORT_HTTP`,
`THOXA_PORT` that is set.

## Headers

`finish` in `app/routes.thx` adds them to every response: a
Content-Security-Policy (styles and images from here, fonts from
`fonts.gstatic.com` with the Google Fonts stylesheet; no script, frames,
or forms), `nosniff`, `Referrer-Policy`, and HSTS. A 200 carries an ETag,
so a repeat request gets a 304. Pages and CSS are `no-cache`; images and
fonts are reused for a day. A page that loads something new needs the CSP
widened there.

## Deploy (Wicket on aulos)

From the repo root, with the Thoxa repo checked out at `../Thoxa`:

```sh
podman build -f site/Containerfile --ignorefile site/.containerignore \
  --build-context thoxa=../Thoxa \
  -t sola-landing:latest -t registry.wicket.cloud/sola-landing:latest .
wicketctl registry push aulos --image sola-landing:latest
wicketctl workload apply aulos --file site/workload.toml --confirm
```

`workload apply` pulls the new `:latest` and replaces the container.
(`wicketctl workload roll` also exists, but its job sat in `pending` on
aulos on 2026-10-19.)

The image builds `thoxa` from that checkout, emits the site's C, compiles
it with `thoxa_rt`, and copies `site/web` and `docs/manual` in. It runs the
Thoxa of the checkout, not a binary from the host, and serves the manual of
the commit it was built from. The server runs as UID 65534 and exits on SIGTERM.
Wicket injects `PORT`. The workload keeps the id of the `sola` workload
that Thoxa's `workloads/sola` created.

## Domains

`sola.moonlight.pm` is the address. `*.moonlight.pm` already points at
aulos, so the name needs no record of its own. `sola.computer` is parked:
the apex A record is gone and no route serves it. `*.sola.computer` stays
for `bot.sola.computer` (the Bots phone API).
