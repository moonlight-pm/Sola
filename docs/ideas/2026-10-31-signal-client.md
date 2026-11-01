# Signal client — parked

**Status:** parked (Joshua, 2026-10-31: "just stick this research in ideas
… its parked"). Research only; nothing built. Do not implement from this
file — promote a slice into a freeze + plan + `CURRENT.md` **Now** first.

**Asked** (Joshua, 2026-10-31): "sometime over the last year i had
attempted to do a custom signal client for sola (or a previous os
experiment) — at the time it was pretty buggy and fidgety … research how
feasible this is now — consider any open source work others have done."

## The earlier attempt

Not found: not in this repo or its history, not in the other repos in
`~/Projects`, not in Joshua's GitHub repos (personal, moonlight-pm,
niarada), not in agent transcripts or notes. The only trace is Signal
Desktop installed from nixpkgs-unstable. Had it been Rust, it was on
presage, whose last release (0.7.0, May 2025) still had link failures,
decryption-failure loops, phone-sent messages missing and no history on
link — likely the "buggy and fidgety".

## Verdict

Feasible for everyday messaging, never set-and-forget. Signal does not
support third-party clients and changes what its servers require a few
times a year; every unofficial stack breaks for days to weeks until it
catches up. That part has not changed; how fast fixes land has.

## The stacks (as of 2026-10-31)

| | State | Fits Sola | Main risk |
|--|--|--|--|
| **presage** (Rust, AGPL-3.0) | No release since 0.7.0 (2025-05-13); `main` very active (merges 2026-10-27); pins libsignal v0.99 while upstream is v0.106.1. Linked device: yes. sqlite store (sled gone). | Pure Rust, no D-Bus | A git rev of a volunteer reimplementation of Signal's service layer. 2026: linking broke on the SPQR requirement (Mar) and an HTTP 409 (Jul — library fix in 5 days, apps 1–4 weeks); forged sync messages accepted Apr–Oct. Can't change groups; no stories, calls, history on link. |
| **signal-cli** (Java, GPL-3.0) | v0.14.9 (2026-10-07), monthly. JSON-RPC daemon on stdio / Unix socket / HTTP; D-Bus optional. SPQR support two days after enforcement. Group admin, stories. Its service layer is a fork of Signal-Android's own Java code, so it inherits Signal's fixes. | Java 25 (GraalVM native build experimental); ~100 MB RSS (unverified) | Releases older than ~3 months may stop working. Keys in plain files. |
| **libsignal** directly (AGPL-3.0) | "Use outside of Signal is unsupported"; git only; Rust API breaks every minor (gRPC default v0.102, Tahini v0.106). | Rust | You become presage. Only funded teams keep up (mautrix-signal: Go + `libsignal-ffi`, within ~10 days). |
| Apps | Flare 0.22.7 (GTK4 on presage; D-Bus for secrets and portals); gurk-rs v0.10.2 (TUI); Whisperfish (Sailfish); signal-rusted (iced on presage, created 2026-10-27, no users). | — | Proof iced + presage works; nothing to reuse. Signal Desktop's service logic is Electron-bound TypeScript. |

Policy: no sign Signal kicks third-party linked devices; enforcement is
technical (required capabilities, version expiry). A linked device that
stays offline ~45 days is unlinked (support page not loaded to confirm).

## Recommendation (if unparked)

signal-cli as the engine, in a service shaped like Vault's:

```text
svc:sola-signal
  sola-signald (Rust) ── runs signal-cli `jsonRpc` on stdio, restarts it
                      ── bus: unread counts, notifications for the shell
                      ── call plane `signal`: status, open a conversation
  Signal (kit app)    ── private socket: conversations, send, attachments
```

Always on, so messages notify while the app is closed and the device is
never unlinked for inactivity. The app ↔ daemon protocol stays
engine-neutral so presage can replace signal-cli later (if it tags
releases and gains history on link) without touching the app.

- **Gets:** 1:1 and group messages, attachments, replies, reactions,
  receipts, typing, desk notifications, `sgnl://` through `sola-open`.
- **Not:** past history — neither engine transfers it on link (presage
  #396 open; nothing in signal-cli's changelog), so the client starts
  empty. Calls stay on the phone or Signal Desktop.
- **Costs:** a Java 25 package on Oath; signal-cli bumped monthly (at
  least quarterly); an occasional relink. Signal pieces are AGPL/GPL —
  keep them in their own process and package.

## Open (Joshua's calls)

1. Engine: signal-cli + a JVM, or presage to stay all-Rust and take more
   breakage.
2. Is a client that starts with no history acceptable.

## Sources

- presage — <https://github.com/whisperfish/presage> (tags, #396, #426, #482)
- libsignal-service-rs — <https://github.com/whisperfish/libsignal-service-rs>
- libsignal releases — <https://github.com/signalapp/libsignal/releases>
- signal-cli — <https://github.com/AsamK/signal-cli> (#1993: deregistered on old versions)
- gurk-rs 409 on link — <https://github.com/boxdot/gurk-rs/issues/556>
- Whisperfish notes — <https://whisperfish.rubdos.be/>
- mautrix-signal — <https://github.com/mautrix/signal/releases>
- signal-rusted — <https://github.com/Guilherme-M-Mateus/signal-rusted>
- LibreSignal and Signal's servers (2016) — <https://github.com/LibreSignal/LibreSignal/issues/37>
