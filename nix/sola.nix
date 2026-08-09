{ stdenv, fetchurl, zstd, lib }:

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

  nativeBuildInputs = [ zstd ];

  unpackPhase = ''
    runHook preUnpack
    mkdir source
    tar --use-compress-program=unzstd -xf $src -C source
    runHook postUnpack
  '';

  sourceRoot = "source";

  # The tarball ships pre-built ELF binaries. Keep DWARF/symbol tables
  # for meaningful crash backtraces; do not strip.
  dontStrip = true;
  dontPatchELF = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin $out/share
    cp -r bin/. $out/bin/
    if [ -d share ]; then
      cp -r share/. $out/share/
    fi

    runHook postInstall
  '';

  meta = with lib; {
    description = "Sola — a Wayland desktop shell";
    homepage = "https://github.com/moonlight-pm/Sola";
    platforms = [ "x86_64-linux" ];
  };
}
