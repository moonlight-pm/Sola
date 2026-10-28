# CEF with H.264 / AAC

Public CEF tarballs (`cef-builds.spotifycdn.com`) are Chromium-branded:
`proprietary_codecs=false`. Steam store DASH trailers are AV1/H.264 + AAC,
so the carousel spinner never gets a source.

Spotify only **hosts** those builds. They are not in the media stack.

This directory builds a CEF commit with the flags below. It is still
pinned to 147.0.10 (`inner.sh`: branch `7727`, commit `d58e84d`) while
`cef-version` is 156.0.14 (2026-10-27). A 156 build moves the pin to
branch `8078`, commit `4fa4ab5`, and needs a fresh checkout: the resume
and `--no-update` paths rebuild whatever `~/.cache/sola/cef-build/` holds.
Flags:

```
proprietary_codecs=true ffmpeg_branding=Chrome is_official_build=true
chrome_pgo_phase=0 use_vaapi=false
```

Multi-hour Chromium compile in podman Ubuntu 22.04 (~40–80 GB under
`~/.cache/sola/cef-build/`). Caps at half the host CPUs (`CEF_BUILD_CPUS`
to override). MPEG-LA applies if the resulting `libcef.so` is
redistributed.

```sh
scripts/cef-codecs/build.sh                 # compile
scripts/cef-codecs/install-into-cache.sh    # copy over ~/.cache/sola/cef-<ver>/
cargo make install browser --release        # helper re-execs onto new libcef
```
