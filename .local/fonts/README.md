# Local font stash (not in git)

**Do not commit font files.** They are large and often license-restricted.
This directory is gitignored except for this README.

Sola resolves fonts by **family name** via system fontconfig (see
`crates/sola-kit/src/fonts.rs`). Nothing under `.local/fonts/` is loaded
automatically — it is a **backup / reinstall source** for machines that
already paid for / legally obtained these faces.

## Layout

```
.local/fonts/
  README.md          # this file (tracked)
  SF.zip             # optional: original archive (~400 MB)
  SF/                # extracted SF faces (otf/ttf)
    SF-Pro-Text-*.otf
    SF-Pro-Display-*.otf
    SF-Pro-Rounded-*.otf
    SF-Compact-*.otf
    SF-Compact-Text-*.otf
    …
```

Typical family names after install (from this archive):

| Role | Family name (fontconfig) |
|------|---------------------------|
| UI body / chrome | `SF Pro Text` |
| Display / large titles | `SF Pro Display` |
| Compact / rounded | `SF Compact Text`, `SF Pro Rounded`, … |

This stash does **not** include SF Mono. Mono default is **Iosevka Term Slab**
(system package). JetBrains Mono remains a fine fallback.

## Reinstall onto a machine

```bash
# from repo root
mkdir -p ~/.local/share/fonts/sola-sf
cp .local/fonts/SF/*.otf .local/fonts/SF/*.ttf ~/.local/share/fonts/sola-sf/ 2>/dev/null
fc-cache -f
fc-list : family | grep -E 'SF Pro|SF Compact'
```

Then either:

- pick **SF Pro Text** in the storybook Theme page (propagates via `Topic::Theme`), or
- rely on seed defaults once the seed prefers SF (kit falls back to Inter if missing).

## Replenish the stash

If this tree is empty on a new machine:

1. Obtain Apple’s SF fonts under a license that allows your use.
2. Drop `SF.zip` here (or copy the `Library/Fonts/*` faces into `SF/`).
3. Never `git add` the binaries — only this README is tracked.

## Defaults (product)

| Role | Seed default | Fallback if missing |
|------|--------------|---------------------|
| ui / chrome / display | `SF Pro Text` (when installed) | `Inter` |
| mono | `Iosevka Term Slab` | `JetBrains Mono` |
