# Fonts

Sola ships no font files. Every Sola application finds fonts by family
name: when it starts, it loads the fonts in the directories fontconfig's
configuration lists into iced's font database
(`sola_kit::fonts::ensure_system_fonts`), once per process. A family that
is not installed falls back to another face, so a font added later is seen
only by applications started after it.

## The fonts on Oath

`pkg:sola` needs Oath's font packages, so they are present wherever Sola
is:

| Package | Families |
|---------|----------|
| `pkg:font-noto` | Noto Sans, Noto Serif, Noto Sans Mono, Noto Sans Symbols, Noto Sans Math, and the other scripts web pages need |
| `pkg:font-noto-cjk` | Noto Sans CJK and Noto Serif CJK |
| `pkg:font-noto-emoji` | Noto Color Emoji |
| `pkg:font-iosevka-term-slab` | Iosevka Term Slab |

Sola's services set `FONTCONFIG_FILE` to `pkg:fontconfig`'s configuration,
`/oath/store/pkg/fontconfig/live/etc/fonts/fonts.conf`, and the
applications the session manager starts inherit it. That file lists the
font packages' directories and `~/.local/share/fonts` (on Oath,
`/home/.local/share/fonts`). It also maps the generic families: sans-serif
and system-ui to Noto Sans, serif to Noto Serif, monospace to Iosevka Term
Slab and then Noto Sans Mono, emoji to Noto Color Emoji, and math to Noto
Sans Math. Browser's engine resolves a page's generic families through
it; Sola's own applications ask for families by name.

## Which family each role uses

Applications never name a family directly; they ask for a role
([Theme and toolkit](theme.md)). The default theme names SF Pro Text for
the interface roles and Iosevka Term Slab for mono. When the family a role
names is not installed, the role takes the first installed family in its
line:

| Roles | Preferred | Then | Then |
|-------|-----------|------|------|
| UI, UI medium, display, chrome | SF Pro Text | Inter | Noto Sans |
| Mono | Iosevka Term Slab | JetBrains Mono | Noto Sans Mono |

Oath ships neither SF Pro Text nor Inter, so interface text is Noto Sans,
and mono text is Iosevka Term Slab.

## Adding a family

Put the family's `.ttf` or `.otf` files in `~/.local/share/fonts/`, run
`fc-cache -f`, and start the applications that should use it again. To
restart the shell, kill it (`pkill -x sola-shell`); PID 1 starts it again
([Restarts](process-manager.md#restarts)). Confirm that
fontconfig sees a family with:

```bash
fc-list : family | grep -E 'Noto Sans|Iosevka Term Slab|SF Pro Text'
```

The family name fontconfig reports must be the name the theme uses.

SF Pro Text is licensed by Apple for its own platforms, and Oath does not
ship it. Install it only from a copy you are entitled to use; once it is
installed, interface text uses it, since the default theme names it.

## No font picker

Sola has no font picker: the roles name the families above, and the
theme changes only in Settings → Appearance, which does not choose fonts.
