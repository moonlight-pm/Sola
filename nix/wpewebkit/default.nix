# WPE WebKit derivation — vendored locally because nixpkgs hasn't carried
# wpewebkit since the 25.x removal (cog dependency churn). Same upstream
# tarball as webkitgtk; the only structural difference between the two
# builds is `-DPORT=WPE` vs `-DPORT=GTK`. The source contains both ports;
# the CMake flag selects which one to compile.
#
# Adapted from nixpkgs's `pkgs/development/libraries/webkitgtk/default.nix`.
# Drops the GTK4 toolkit and accessibility / gamepad / spell-check deps
# that the WPE port doesn't link against, and adds libwpe + wpebackend-fdo
# for the FDO platform backend that emits frames as DMA-BUFs (the whole
# reason we want WPE).
#
# Heavy first build — WebKit is a large C++ project. Cached in /nix/store
# afterward; only re-runs on version bump or derivation edit.
{
  lib,
  clangStdenv,
  fetchurl,
  perl,
  python3,
  ruby,
  bison,
  gperf,
  cmake,
  ninja,
  pkg-config,
  gettext,
  gnutls,
  libgcrypt,
  libgpg-error,
  wayland,
  wayland-protocols,
  wayland-scanner,
  libwebp,
  libxkbcommon,
  libavif,
  libepoxy,
  libjxl,
  cairo,
  expat,
  libxml2,
  libsoup_3,
  libxslt,
  harfbuzzFull,
  icu,
  pcre2,
  libjpeg,
  util-linux, # libmount (gio-2.0 dep that gtk4 was transitively bringing in)
  hyphen,
  woff2,
  libinput,
  libdrm,
  libsysprof-capture,
  libpthread-stubs,
  nettle,
  libtasn1,
  p11-kit,
  libidn,
  libGL,
  libGLU,
  libgbm,
  libintl,
  lcms2,
  fontconfig,
  freetype,
  openssl,
  sqlite,
  gst-plugins-base,
  gst-plugins-bad,
  bubblewrap,
  libseccomp,
  libbacktrace,
  systemdLibs,
  xdg-dbus-proxy,
  replaceVars,
  glib,
  unifdef,
  addDriverRunpath,
  libwpe,
  libwpe-fdo,
  systemdSupport ? lib.meta.availableOn clangStdenv.hostPlatform systemdLibs,
}:

# https://webkitgtk.org/2024/10/04/webkitgtk-2.46.html — upstream
# recommends building with clang. We follow.
clangStdenv.mkDerivation (finalAttrs: {
  pname = "wpewebkit";
  version = "2.52.3";

  outputs = [
    "out"
    "dev"
  ];

  # https://github.com/NixOS/nixpkgs/issues/153528 — same constraint as
  # webkitgtk, separate debug info so we can link in 4GB address space.
  separateDebugInfo = clangStdenv.hostPlatform.isLinux && !clangStdenv.hostPlatform.is32bit;

  # WPE upstream ships its own release tarball — distinct from the
  # `webkitgtk-*.tar.xz` archive. Both come from the same WebKit
  # repository but each release strips port-specific files the other
  # one doesn't need (the webkitgtk tarball drops `OptionsWPE.cmake`
  # and friends, which is why we can't just reuse it with `-DPORT=WPE`).
  src = fetchurl {
    url = "https://wpewebkit.org/releases/wpewebkit-${finalAttrs.version}.tar.xz";
    hash = "sha256-tRsdsebumdF3H0o1jBKP3ieneYTfIO5stZhY5SBmLQs=";
  };

  patches = [
    # Same patch the webkitgtk derivation applies — BubblewrapLauncher.cpp
    # is shared between the GTK and WPE ports, so we need the same Nix
    # store bind mounts available inside the sandbox.
    (replaceVars ./fix-bubblewrap-paths.patch {
      inherit (builtins) storeDir;
      inherit (addDriverRunpath) driverLink;
    })
  ];

  nativeBuildInputs = [
    bison
    cmake
    gettext
    gperf
    ninja
    perl
    perl.pkgs.FileCopyRecursive # used by copy-user-interface-resources.pl
    pkg-config
    python3
    ruby
    glib # for gdbus-codegen
    unifdef
    wayland-scanner
  ];

  buildInputs = [
    cairo # required by WebCore even when using skia
    expat
    freetype
    fontconfig
    libavif
    libepoxy
    libjxl
    gnutls
    gst-plugins-bad
    gst-plugins-base
    # webkitgtk's full buildInputs include `harfbuzz` only — the GTK
    # port pulls icu integration via gtk4's transitive deps. WPE
    # doesn't, so we use harfbuzzFull (harfbuzz with icu integration
    # built in) plus pcre2 (otherwise glib-2.0's pkg-config check
    # fails because libpcre2-8.pc isn't visible).
    harfbuzzFull
    icu
    pcre2
    libjpeg
    util-linux
    hyphen
    woff2
    libinput
    libdrm
    libGL
    libGLU
    libgbm
    libgcrypt
    libgpg-error
    libidn
    libintl
    lcms2
    libpthread-stubs
    libsysprof-capture
    libtasn1
    libwebp
    libxkbcommon
    libxml2
    libxslt
    libbacktrace
    nettle
    p11-kit
    sqlite
    libseccomp
    wayland
    wayland-protocols
    # The WPE-specific bits — what differentiates us from the GTK port
    # and gives us the DMA-BUF export pipeline. Kept in buildInputs
    # (not propagatedBuildInputs) so a propagation tweak doesn't blow
    # the derivation hash and force a full rebuild. Consumers must add
    # libwpe + libwpe-fdo themselves; nix/wpewebkit/shell.nix handles
    # this transparently. Promote to propagated on the next deliberate
    # rebuild.
    libwpe
    libwpe-fdo
  ]
  ++ lib.optionals systemdSupport [
    systemdLibs
  ];

  propagatedBuildInputs = [
    libsoup_3
  ];

  cmakeFlags =
    let
      cmakeBool = x: if x then "ON" else "OFF";
    in
    [
      "-DPORT=WPE"
      # We want the FDO-backed exportable view backend, which is what
      # wpebackend-fdo provides. ENABLE_WPE_PLATFORM=ON gives us the
      # newer wpe-platform API; the legacy API stays available as the
      # libwpe-based interface that wpebackend-fdo plugs into.
      "-DENABLE_WPE_PLATFORM=ON"
      "-DUSE_GBM=ON"
      # Things we don't need for our use cases:
      "-DENABLE_DOCUMENTATION=OFF"
      "-DENABLE_GAMEPAD=OFF"
      "-DENABLE_INTROSPECTION=OFF"
      "-DENABLE_WEBDRIVER=OFF"
      # USE_ATK defaults ON in OptionsWPE.cmake and pulls atk (GTK's
      # accessibility toolkit) — we have no accessibility surface yet
      # and don't want the GTK dep. Revisit if we ship a screen reader.
      "-DUSE_ATK=OFF"
      # Speech synthesis requires Flite or LibSpiel — neither shipped
      # for our use case. We can re-enable when a wrapped app needs TTS.
      "-DENABLE_SPEECH_SYNTHESIS=OFF"
      # Have to be explicitly specified per upstream commit
      # https://github.com/WebKit/WebKit/commit/a84036c6d1d66d723f217a4c29eee76f2039a353
      "-DBWRAP_EXECUTABLE=${lib.getExe bubblewrap}"
      "-DDBUS_PROXY_EXECUTABLE=${lib.getExe xdg-dbus-proxy}"
    ]
    ++ lib.optionals (!systemdSupport) [
      "-DENABLE_JOURNALD_LOG=OFF"
    ];

  postPatch = ''
    patchShebangs .
  '';

  requiredSystemFeatures = [ "big-parallel" ];

  meta = {
    description = "Web content rendering engine, WPE port (vendored)";
    homepage = "https://wpewebkit.org/";
    license = lib.licenses.bsd2;
    pkgConfigModules = [
      "wpe-webkit-2.0"
      "wpe-javascriptcore-2.0"
      "wpe-web-process-extension-2.0"
    ];
    platforms = lib.platforms.linux;
  };
})
