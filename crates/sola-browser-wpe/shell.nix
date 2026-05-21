# Per-crate shell.nix. sola-make's isolated-crate builder looks for
# this file at `crates/<name>/shell.nix` and, if present, wraps the
# `cargo build` invocation with `nix-shell <this> --run …`. Our
# canonical dev shell lives at `nix/wpewebkit/shell.nix`; this file
# is the convention-named pointer at it so `cargo make build
# sola-browser-wpe` and `cargo make install` work without the user
# having to enter the shell manually.

import ../../nix/wpewebkit/shell.nix
