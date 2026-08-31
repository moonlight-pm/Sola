---
name: sola-visual-parity
description: >
  Pixel-match a Sola surface to its iced (or other) reference using live
  captures. Use when porting iced apps to sola-kit-spike / HTML kit labs,
  matching layout or styles, pixel-perfect, “looks off”, “match the original”,
  or comparing lab vs iced. /sola-visual-parity
---

# Visual parity (live capture, not memory)

Impeccable / frontend-design are craft. They do **not** prove two windows
match. For iced → HTML-kit (or any “copy this chrome”) the source of truth
is **pixels on the running desk**, plus the iced layout constants in code.

## Before the first style edit

1. Confirm both windows exist: `solactl compositor windows`.
2. Capture **each** surface (not the full output unless that is the surface):

```bash
mkdir -p /tmp/sola/parity
solactl compositor screenshot -a sola-mail -w Mail -o /tmp/sola/parity/iced.png
solactl compositor screenshot -a sola-mail-lab -w 'Mail (lab)' -o /tmp/sola/parity/lab.png
md5sum /tmp/sola/parity/iced.png /tmp/sola/parity/lab.png
identify /tmp/sola/parity/iced.png /tmp/sola/parity/lab.png
```

3. **Read both PNGs** with the image tool. Sample chrome (sidebar, search,
   toolbar icons, list row, letter). If `md5` matches, the two windows
   share a zone and you captured whoever is on top twice. Raise the other
   (`Meta+Tab` / click it) and recapture. Do not proceed on a duplicate.
4. Pull numbers from iced source (`LIST_W`, `SIDEBAR_W`, `CHROME_H`, pads)
   **and** from the iced screenshot. Code constants win when they disagree
   with stale CSS; screenshots win when CSS “looks close.”

## Loop

Edit kit components (not app-only copies). Rebuild the lab binary. Restart
it if self-watch did not re-exec. Recapture **lab**. Compare the same crop
(chrome strip, list, letter). Repeat until the mismatch the human named is
gone — then check neighbors (search, icons, header, letter pad).

`solactl compositor sample` is the pointer RGBA probe (sola-scope). Use it
when a hex is in dispute.

## Done

You may not claim parity from a user crop, a storybook page, or “I updated
the CSS.” Claim only with two distinct captures you opened.

## Not this skill

- Craft / hierarchy / copy — impeccable.
- Distinctive visual direction — frontend-design.
- Screenshot implementation — `docs/visual/README.md` + river capture.
