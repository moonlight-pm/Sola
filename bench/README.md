# Browser bench harness

Tools that drive `sola-browser` (WPE) runs and produce CPU / RSS samples.

## Build

```sh
cargo make build sola-browser
```

## Run

```sh
bench/run-bench.sh https://slate.auto 30 docs/notes/data/wpe-slate
```

Optional summarizer (if comparing historical CEF CSVs from tag
`pre-cef-removal`):

```sh
bench/summarize.py docs/notes/data/wpe-slate docs/notes/data/cef-slate \
    -o docs/notes/2026-05-21-wpe-vs-cef-bench.md
```

## Notes

- The harness `pkill`s leftover `sola-browser*` before sampling.
- Historical dual-engine comparison:
  `docs/notes/2026-05-21-wpe-vs-cef-bench.md` and
  `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`
  (CEF crate removed from tree; recover from git tag `pre-cef-removal`).
