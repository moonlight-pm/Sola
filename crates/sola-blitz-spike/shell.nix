# Isolated Skia build env. `cargo make build sola-blitz-spike` wraps cargo
# in this shell when present (see sola-make isolated.rs).
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
    python3
    ninja
    cmake
    clang
  ];
  buildInputs = with pkgs; [
    fontconfig
    freetype
    libglvnd
    libxkbcommon
    wayland
    openssl
  ];
  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
}
