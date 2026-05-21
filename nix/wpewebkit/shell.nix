# Dev shell for sola-browser-wpe: brings our vendored wpewebkit into
# scope alongside the WPE-platform libs (`libwpe`, `libwpe-fdo`) and
# pkg-config. Inside this shell:
#
#   pkg-config --libs wpe-webkit-2.0      # resolves cleanly
#   cargo build --bin wpe-probe           # build.rs can find headers + libs
#
# Used by `cargo make build sola-browser-wpe` via a sola-make wrapper
# (TODO) that shells out into `nix-shell` before invoking cargo.
let
  pkgs = import <nixos> {};
  wpewebkit = pkgs.callPackage ./default.nix {
    inherit (pkgs.gst_all_1) gst-plugins-base gst-plugins-bad;
  };
in
pkgs.mkShell {
  buildInputs = [
    wpewebkit
    # Should be propagated by wpewebkit itself — wpe-webkit-2.0.pc has
    # `Requires: ... wpe-1.0 wpe-platform-2.0` — but we keep them as
    # plain buildInputs in default.nix to avoid the derivation-hash
    # flip that would force a full WebKit rebuild. Re-evaluate on a
    # planned rebuild cycle.
    pkgs.libwpe
    pkgs.libwpe-fdo
  ];
  nativeBuildInputs = [
    pkgs.pkg-config
  ];
}
