# Common: showing that a send happened

**Status:** overtaken (2026-11-02). Joshua replaced Common with pinned windows,
toggled from the Window menu, with a pin on the row. Kept for the options,
should pinning a window want more of a sign. Super+Shift+0 no longer exists.

## The problem

Super+Shift+0 worked the first time Joshua tried it, and he took it for
broken: "I just didn't think it did because nothing changed". A window sent to
Common stays where it is, so nothing moves. What does change is small and
still: its app's icon on the Common island, a pin on its row, and no close dot
on its titlebar.

## Options, smallest first

1. **Row pulse.** The row flashes the accent colour and the pin stamps in, a
   little oversized before settling. It explains the pin, but only marks the
   change and does not say where the window went.
2. **Island pulse.** The Common island lights up briefly and the icon pops in.
   This shows the destination, and it would work for every Super+Shift+N,
   since an ordinary send just makes the window vanish.
3. **Window flash.** An accent border flashes around the window itself, where
   you are already looking. It is the hardest to miss but says nothing about
   what happened, and it probably needs River's help to draw.
4. **Flying icon.** A copy of the app icon flies from the row up to the Common
   island. This explains it best, but the sidebar and the top bar are separate
   surfaces, so it needs an overlay across both, and it could get tiresome.
5. **Toast.** "Mail is now on every workspace." The clearest, and the shell
   already has toasts, but it becomes noise once you know the feature.

**Refused actions** need something too. Super+Shift+0 on a window already in
Common, Super+Shift+N on a common window outside Common, and Super+W on a
pinned window all do nothing silently, which reads as a broken key. The pin
could give a quick shake, plus a toast the first time ("Mail can only leave
from Common").

## Recommendation, if Common stays

1 and 2 together, plus the shake for refusals. The row says this window
changed, the island says where it went, and both are in chrome the shell
already draws. An ordinary send can use the island half, so every send behaves
the same way. Skip the flying icon, and try the window flash only if those
still get missed.

Not checked: what the animation takes in the code. The top bar may be more
work than the sidebar.
