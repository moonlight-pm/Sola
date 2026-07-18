#!/usr/bin/env python3
"""Raw keyboard probe for sola-terminal (and any other terminal).

Run INSIDE the terminal you want to diagnose (new tab is fine; no need to
restart Grok). Press keys; each chunk of input is printed as hex + a guess.

What to try:
  Enter              → expect 0d              (CR)
  Shift+Enter        → expect 1b 0d           (ESC CR — same as Alt+Enter)
  Ctrl+Enter         → expect 1b 5b 31 33 3b 35 75   (CSI 13;5u)
  Alt+Enter          → expect 1b 0d           (ESC CR)
  a / Shift+a        → 61 / 41
  Ctrl+C             → quit

If Shift+Enter prints only 0d, the OUTER terminal is not seeing Shift
(modifier tracking). If it prints 1b 0d, newline encoding works (Grok/Claude
both treat ESC CR as newline).

Usage:
  python3 crates/sola-terminal/scripts/keydebug.py
  # or after install / from anywhere:
  python3 /home/joshua/Workspace/Sola/crates/sola-terminal/scripts/keydebug.py
"""

from __future__ import annotations

import os
import sys
import termios
import tty


def decode_guess(data: bytes) -> str:
    if data == b"\r":
        return "plain Enter (CR) — NO shift/ctrl distinction"
    if data == b"\n":
        return "LF (0x0a) — often Ctrl+J"
    if data == b"\x1b\r":
        return "Shift/Alt+Enter (ESC CR) — portable newline ✓"
    if data == b"\x1b[13;2u":
        return "Shift+Enter (CSI 13;2u)"
    if data == b"\x1b[13;5u":
        return "Ctrl+Enter (CSI 13;5u) ✓"
    if data == b"\x1b[13;3u":
        return "Alt+Enter (CSI 13;3u)"
    if data == b"\x1b[13u":
        return "Enter as CSI-u (no modifiers)"
    if data == b"\x03":
        return "Ctrl+C"
    if data == b"\x1b":
        return "Escape"
    # Generic CSI-u: ESC [ digits [;digits...] u
    if data.startswith(b"\x1b[") and data.endswith(b"u"):
        body = data[2:-1].decode("ascii", "replace")
        return f"CSI-u sequence ({body})"
    if data.startswith(b"\x1b["):
        return f"CSI/other escape ({data!r})"
    if len(data) == 1 and 32 <= data[0] < 127:
        return f"printable {data!r}"
    return "unknown / multi-key / partial"


def main() -> int:
    fd = sys.stdin.fileno()
    if not os.isatty(fd):
        print("stdin is not a TTY — run this inside sola-terminal", file=sys.stderr)
        return 1

    old = termios.tcgetattr(fd)
    # Ask for kitty disambiguate if the outer supports it (optional; our
    # encoder should emit CSI-u for Shift+Enter even without this).
    sys.stdout.write("\x1b[>1u")
    sys.stdout.flush()

    print("keydebug — raw input probe")
    print("  Enter / Shift+Enter / Ctrl+Enter / Alt+Enter / letters")
    print("  Ctrl+C to quit\r")
    print("-" * 60 + "\r")
    sys.stdout.flush()

    try:
        tty.setraw(fd)
        while True:
            chunk = os.read(fd, 64)
            if not chunk:
                break
            if chunk == b"\x03":
                sys.stdout.write("\r\n^C — bye\r\n")
                break
            hex_str = " ".join(f"{b:02x}" for b in chunk)
            ascii_str = "".join(
                chr(b) if 32 <= b < 127 else "." for b in chunk
            )
            guess = decode_guess(chunk)
            # raw mode: explicit CR/LF
            sys.stdout.write(f"hex: {hex_str}\r\n")
            sys.stdout.write(f"  ascii: {ascii_str!r}\r\n")
            sys.stdout.write(f"  → {guess}\r\n")
            sys.stdout.write("\r\n")
            sys.stdout.flush()
    finally:
        # Pop kitty keyboard flags if we pushed them.
        sys.stdout.write("\x1b[<u")
        sys.stdout.flush()
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        print("\nrestored terminal.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
