# vendored @remix-run/ui

Source: https://github.com/remix-run/remix/tree/main/packages/ui/src

Upstream commit: aee2ee03fad2113f7fd027d1d10bf19ea961d03c

Imported with:

```sh
cd /tmp && git clone --depth 1 --filter=blob:none --sparse \
  https://github.com/remix-run/remix.git remix-clone
cd remix-clone && git sparse-checkout set packages/ui
cp -r packages/ui/src/. <worktree>/crates/sola-kit/web/vendor/remix-ui/
cp packages/ui/LICENSE <worktree>/crates/sola-kit/web/vendor/remix-ui/LICENSE
```

Then dropped subtrees we don't use:

- `server/` — SSR / streaming, sola-kit doesn't SSR
- `test/`, `test.ts` — test harness, not runtime

To re-vendor against a newer upstream, run the same incantation and
update the commit SHA above.
