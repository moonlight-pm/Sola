# Install Sola from a locally staged release-shaped tree (bin/ + share/ + cef/).
#
# Host `cargo build` binaries embed the *build host* dynamic linker store path.
# That path is usually absent inside the guest image → execve ENOENT
# ("Failed to spawn client: No such file or directory" from cage).
# We re-link every ELF to this derivation's nixpkgs interpreter + rpath.

{
  stdenv,
  lib,
  patchelf,
  stage,
  wayland,
  libxkbcommon,
  libGL,
  libglvnd,
  vulkan-loader,
  mesa,
  alsa-lib,
  zlib,
  libffi,
  openssl,
  fontconfig,
  freetype,
  expat,
  glib,
}:

let
  runtimeLibs = [
    stdenv.cc.cc
    wayland
    libxkbcommon
    libGL
    libglvnd
    vulkan-loader
    mesa
    alsa-lib
    zlib
    libffi
    openssl
    fontconfig
    freetype
    expat
    glib
  ];
  rpath = lib.makeLibraryPath runtimeLibs;
in
stdenv.mkDerivation {
  pname = "sola";
  version = "local-stage";

  src = stage;

  nativeBuildInputs = [ patchelf ];

  dontStrip = true;
  # We patchelf manually; skip the generic fixup that would re-process.
  dontPatchELF = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin $out/lib/cef $out/share
    if [ -d bin ]; then
      cp -a bin/. $out/bin/
    fi
    if [ -d share ]; then
      cp -a share/. $out/share/
    fi
    if [ -d cef ] && [ "$(ls -A cef 2>/dev/null || true)" != "" ]; then
      cp -a cef/. $out/lib/cef/
    fi

    interp="$(cat $NIX_CC/nix-support/dynamic-linker)"
    full_rpath="${rpath}:$out/lib/cef:/run/current-system/sw/share/nix-ld/lib"

    # Re-point every ELF so the guest can exec it. Browser engines may still
    # miss CEF/WPE until those are staged; that is OK for installer dogfood.
    for f in $out/bin/*; do
      if [ -f "$f" ] && patchelf --print-interpreter "$f" >/dev/null 2>&1; then
        echo "patchelf $f"
        chmod u+w "$f"
        patchelf --set-interpreter "$interp" --set-rpath "$full_rpath" "$f" || true
      fi
    done

    runHook postInstall
  '';

  meta = with lib; {
    description = "Sola — local stage install for VM images (patchelf'd for guest)";
    platforms = [ "x86_64-linux" ];
  };
}
