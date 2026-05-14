# vendored @chenglou/pretext

Source: <https://www.npmjs.com/package/@chenglou/pretext>

Upstream version: `0.0.7`

Imported with:

```sh
curl -sSL https://registry.npmjs.org/@chenglou/pretext/-/pretext-0.0.7.tgz \
  -o /tmp/pretext.tgz
mkdir -p /tmp/pretext-pack && tar -C /tmp/pretext-pack -xzf /tmp/pretext.tgz
cp -r /tmp/pretext-pack/package/dist <worktree>/crates/sola-kit/web/vendor/pretext/
cp /tmp/pretext-pack/package/{LICENSE,package.json} \
   <worktree>/crates/sola-kit/web/vendor/pretext/
```

We ship the pre-built `dist/` from the npm tarball — pure ES modules
with relative `./*.js` imports, no bundler needed. `src/` and
`pages/` are dropped (test sources + demos), as is `CHANGELOG.md`.

The asset bundle's dir mount filters `.d.ts` automatically; CEF only
sees `.js` files. The `@chenglou/pretext` importmap entry resolves
to `/vendor/pretext/dist/layout.js` (the `main` export). The
`./rich-inline` subpath is not currently wired — add it to the
importmap if a real consumer needs it.

To re-vendor against a newer upstream, run the same incantation
with the new version number above.
