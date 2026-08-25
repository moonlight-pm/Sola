{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config ];
  buildInputs = with pkgs; [
    fontconfig
    freetype
    wayland
    libxkbcommon
    vulkan-loader
    libglvnd
  ];
}
