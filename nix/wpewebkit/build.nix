# Top-level expression for `nix-build nix/wpewebkit/build.nix`. Pulls
# nixpkgs from the system channel, then callPackage's our derivation
# with the right dependency set. Result symlink lands in the current
# working directory; the output exposes `lib/libWPEWebKit-2.0.so` and
# `share/pkgconfig/wpe-webkit-2.0.pc` (the bits sola-browser will
# eventually link against).
let
  pkgs = import <nixos> {};
in
pkgs.callPackage ./default.nix {
  # gst-plugins-* live under gst_all_1, not at the top level. callPackage's
  # auto-binding can't find them by attr name alone — pass them explicitly.
  inherit (pkgs.gst_all_1) gst-plugins-base gst-plugins-bad;
}
