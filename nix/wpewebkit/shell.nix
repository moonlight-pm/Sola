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
    # libxkbcommon is included transitively from WPEKeymapXKB.h —
    # bindgen needs its include path even though we don't bind any
    # xkbcommon functions ourselves.
    pkgs.libxkbcommon
    # libEGL — wpe_fdo_initialize_for_egl_display() takes an
    # EGLDisplay, which we get via eglGetDisplay(EGL_DEFAULT_DISPLAY).
    # WPE's WebProcess also needs it for the GPU side rendering.
    pkgs.libGL
  ];
  nativeBuildInputs = [
    pkgs.pkg-config
    # Rust toolchain so `nix-shell --pure` still has cargo/rustc.
    # Keeps cargo on the same nixpkgs Rust as everything else.
    pkgs.cargo
    pkgs.rustc
    # bindgen needs libclang at build time. LIBCLANG_PATH below points
    # the bindgen build-dep at it.
    pkgs.clang
    # cargo needs a CA bundle to fetch from crates.io under --pure.
    pkgs.cacert
  ];

  # Surface the CA bundle through the env vars cargo / rustls / curl
  # honor — without these, `cargo fetch` fails SSL handshake.
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

  # bindgen's libloading scan won't find libclang under --pure without
  # an explicit pointer; LIBCLANG_PATH is the standard env var bindgen
  # honors.
  LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

  # WebKit looks here for WPEWebProcess / WPENetworkProcess / WPEGPUProcess
  # at runtime. Without it the engine can't fork its workers and the
  # first webkit_web_view_load_uri call hangs forever.
  WEBKIT_EXEC_PATH =
    let
      wpewebkit = pkgs.callPackage ./default.nix {
        inherit (pkgs.gst_all_1) gst-plugins-base gst-plugins-bad;
      };
    in
    "${wpewebkit}/libexec/wpe-webkit-2.0";
}
