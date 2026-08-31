# Visual parity against a running reference

When matching iced (or any shipped window) in this worktree — lab twins,
“pixel-perfect”, “looks off”, layout/style copy — follow
[`.grok/skills/sola-visual-parity/SKILL.md`](../skills/sola-visual-parity/SKILL.md)
**before** the first CSS/layout edit.

Capture with `solactl compositor screenshot -a APP -w TITLE`. If two
captures hash the same, the windows share a zone: raise the other and
recapture. Do not guess from a user crop alone.
