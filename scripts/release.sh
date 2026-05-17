#!/usr/bin/env bash
# Build, bundle, and publish a Sola release tarball.
#
# Usage: scripts/release.sh <version>     (e.g. 0.1.0)
#
# Bundles /opt/sola/bin/* plus the patched CEF Release tree from the
# build host's cache, pre-patches CEF-linking binaries' RUNPATH to
# point at /opt/sola/cef (so the bundle is usable bare too, not just
# through the Nix derivation), tar+zstd compresses, computes the SRI
# hash, updates nix/release.nix, commits, tags, and uploads to GitHub
# Releases.

set -euo pipefail

if [ -z "${1:-}" ]; then
  echo "usage: $0 <version>     (e.g. 0.1.0)" >&2
  exit 1
fi

VERSION="$1"
TAG="v${VERSION}"
ROOT="$(git rev-parse --show-toplevel)"
CEF_VERSION="$(cat "$ROOT/cef-version")"
CEF_RELEASE="$HOME/.cache/sola/cef-${CEF_VERSION}/Release"
TARBALL_NAME="sola-${VERSION}-linux-x86_64.tar.zst"
STAGING="$(mktemp -d -t sola-release-XXXXXX)"
TARBALL="$STAGING/${TARBALL_NAME}"

cleanup() { rm -rf "$STAGING"; }
trap cleanup EXIT

cd "$ROOT"

# Preflight
[ -d "$CEF_RELEASE" ] || {
  echo "CEF cache not found at $CEF_RELEASE" >&2
  echo "Run 'cargo make install' first to populate it." >&2
  exit 1
}
[ -d /opt/sola/bin ] || {
  echo "/opt/sola/bin not found — run 'cargo make install' first." >&2
  exit 1
}
[ -z "$(git status --porcelain)" ] || {
  echo "Working tree dirty — commit or stash before releasing." >&2
  exit 1
}

echo ">>> staging bundle in $STAGING"
mkdir -p "$STAGING/sola/bin" "$STAGING/sola/cef"
cp -r /opt/sola/bin/. "$STAGING/sola/bin/"
cp -r "$CEF_RELEASE"/. "$STAGING/sola/cef/"

echo ">>> re-pointing CEF-linking binaries at /opt/sola/cef"
for bin in sola-kit sola-monitor sola-settings; do
  if [ -e "$STAGING/sola/bin/$bin" ]; then
    patchelf --set-rpath \
      "/opt/sola/cef:/run/current-system/sw/share/nix-ld/lib" \
      "$STAGING/sola/bin/$bin"
  fi
done

echo ">>> compressing (zstd -19)"
tar -C "$STAGING/sola" --use-compress-program='zstd -T0 -19' -cf "$TARBALL" .
ls -lh "$TARBALL"

echo ">>> computing SRI hash"
HASH="$(nix hash file --type sha256 --base64 "$TARBALL")"
SRI="sha256-${HASH}"

echo ">>> updating nix/release.nix"
cat > nix/release.nix <<EOF
{
  version = "${VERSION}";
  hash = "${SRI}";
}
EOF

git add nix/release.nix
git commit -m "release: v${VERSION}"
git tag -a "$TAG" -m "Sola ${VERSION}"
git push github master "$TAG"

echo ">>> publishing GitHub release"
gh release create "$TAG" "$TARBALL" \
  --repo moonlight-pm/Sola \
  --title "Sola ${VERSION}" \
  --notes "Sola desktop shell ${VERSION}. See INSTALL.md for setup."

echo ""
echo "Done. v${VERSION} is live."
echo "Colleague pulls it on his next 'nix flake update' + 'nixos-rebuild switch'."
