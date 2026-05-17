{ stdenv, fetchurl, patchelf, zstd, lib }:

let
  release = import ./release.nix;
in
stdenv.mkDerivation {
  pname = "sola";
  version = release.version;

  src = fetchurl {
    url = "https://github.com/moonlight-pm/Sola/releases/download/v${release.version}/sola-${release.version}-linux-x86_64.tar.zst";
    hash = release.hash;
  };

  nativeBuildInputs = [ patchelf zstd ];

  unpackPhase = ''
    runHook preUnpack
    mkdir source
    tar --use-compress-program=unzstd -xf $src -C source
    runHook postUnpack
  '';

  sourceRoot = "source";

  # The tarball ships pre-built ELF binaries plus the CEF Release tree.
  # We do not want nixpkgs' generic strip / patchelf to touch any of
  # this — libcef.so already carries a DT_RUNPATH pointing at
  # /run/current-system/sw/share/nix-ld/lib for its ~26 transitive
  # deps, and the Sola binaries are debug-info-laden by design (so
  # crash backtraces are meaningful).
  dontStrip = true;
  dontPatchELF = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin $out/lib/cef
    cp -r bin/. $out/bin/
    cp -r cef/. $out/lib/cef/

    # The CEF-linking binaries shipped in the tarball have their
    # RUNPATH pre-pointed at /opt/sola/cef (so the tarball is usable
    # outside Nix too). Re-point them at the Nix store location.
    for bin in sola-kit sola-monitor sola-settings; do
      if [ -e $out/bin/$bin ]; then
        patchelf --set-rpath \
          "$out/lib/cef:/run/current-system/sw/share/nix-ld/lib" \
          $out/bin/$bin
      fi
    done

    runHook postInstall
  '';

  meta = with lib; {
    description = "Sola — a Wayland desktop shell";
    homepage = "https://github.com/moonlight-pm/Sola";
    platforms = [ "x86_64-linux" ];
  };
}
