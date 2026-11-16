# Signal client — a Rust client of our own (research)

**Status:** started 2026-11-16 as
[`../plans/2026-11-16-sola-signal-plan.md`](../plans/2026-11-16-sola-signal-plan.md)
(branch `signal`). This file is the research behind it; the plan has what
is built.

**Direction** (Joshua, 2026-11-16): "a fully custom client, in rust. That
means no java … I'm ok with having to keep the thing working when signal
changes .. it will be ongoing … a fully independent app - not relying on
secondary cli's .. it shall be a single binary." This replaces the
2026-10-31 recommendation (signal-cli behind a daemon; kept at the end).

**First asked** (Joshua, 2026-10-31): "sometime over the last year i had
attempted to do a custom signal client for sola (or a previous os
experiment) — at the time it was pretty buggy and fidgety … research how
feasible this is now". That attempt was not found anywhere (repos,
GitHub, transcripts); had it been Rust, it was on presage 0.7.0.

## Verdict (2026-11-16)

Achievable: one binary, `sola-signal`, a kit app that is a **linked
device** of the phone, with its own service layer written against
Signal's own Rust library. Everyday messaging first (1:1, groups,
attachments, reactions, replies, edits, receipts, typing, history on
link); group administration, stories and calls later. Never
set-and-forget: Signal changes what its servers require a few times a
year, and the fix is then ours to write.

What makes it reasonable now: Signal is moving its service API out of the
apps and into **libsignal**. Signal-Android and Signal-Desktop both open
their chat, unauthenticated and provisioning connections through
`libsignal-net`, and send messages through its typed API. That library is
Rust, so a Rust client calls it with no bridge. Each release moves more of
the wire into it (v0.108.0 added `getAccountIdentity`), and it already
hides the move from REST-over-websocket to gRPC. The part we own gets
smaller each release, and following Signal is more often a tag bump.

## What exists (read from the sources, 2026-11-16)

| | State | Use to us |
|--|--|--|
| **libsignal** (Signal, Rust, AGPL-3.0) | v0.108.0 (2026-11-13), about weekly; git only; "use outside of Signal is unsupported"; Rust API breaks between minors. Android 8.36.2 and Desktop 8.38.0 both pin 0.107.0. | **The base.** `protocol` (sessions, sealed sender, PQXDH, SPQR, sender keys), `zkgroup` (groups, profile credentials), `net` + `net/chat` (connections, typed API: send sealed / unsealed / multi-recipient / sync, pre-keys, profile credentials, usernames, upload forms, devices, registration, key transparency; CDSI; the chat server's gRPC protos), `provisioning` (the link envelope), `message-backup` (the history archive), `account-keys`, `usernames`. |
| **Signal-Desktop** (TypeScript, AGPL-3.0) | Released weekly. `ts/textsecure`: 21.7k lines. | **First reference**: it is the linked desktop device. Linking, sync messages, the history import, the capabilities a linked device must declare. |
| **Signal-Android** (Java/Kotlin, AGPL-3.0) | Released weekly. `lib/libsignal-service`: 29.9k lines, 14 proto files (2.5k lines). | **Second reference**, and where the protos are copied from. Most complete service layer. |
| **mautrix-signal** `signalmeow` (Go, AGPL-3.0) | v26.11 (2026-11-16), monthly, funded. 14.2k lines over `libsignal-ffi`. | **Third reference**: the smallest complete independent client, and proof that one small team keeps up. |
| **libsignal-service-rs** (Rust, AGPL-3.0) | `main` 2026-11-05; pins libsignal v0.99.0 (a bump to 0.106.2 is an open PR). 13.4k lines. Its own websocket and reqwest; `libsignal-net` only for contact discovery. | Reading only. Nine libsignal minors behind, and not on the transport Signal is moving to. |
| **presage** (Rust, AGPL-3.0) | `main` 2026-11-15, `0.8.0-dev`, no tag since 0.7.0 (2025-05). 8.6k lines. ~37 open PRs, most from one contributor, holding create/change groups, the history archive, stories, sender keys, multi-recipient send, the pre-key rework. | Reading only. Review is the bottleneck: the features exist and wait. |
| **signal-rusted** (Rust, AGPL-3.0) | iced 0.14 over vendored presage, 22.9k lines, created 2026-10-27, no users. Imports link-time history with `libsignal-message-backup`. | Proof that iced + history on link works. |
| **RingRTC** (Signal, Rust + WebRTC, AGPL-3.0) | v2.75.0. A `native` feature (cubeb audio) and a prebuilt WebRTC. | The path to calls, later. |
| Flare, gurk-rs, Whisperfish | All on presage. | Nothing to reuse. |
| signal-cli | Java. | Ruled out. |

## Shape

One crate, `crates/sola-signal`, its own package (`pkg:sola-signal`),
licensed AGPL-3.0-only as everything it links is. One binary:

```text
sola-signal
  ui/        kit app, drawn as Mail is: conversations, a thread, a composer
  engine/    link, connect, receive, send, groups, attachments, profiles,
             sync, storage service, history import   (ours)
  store/     rusqlite: keys, sessions, conversations, messages
  proto/     copied from Signal-Android, with a script that shows the diff
  ── libsignal at one pinned tag (protocol, zkgroup, net, …)
```

It keeps running with its window closed (an `iced::daemon`), so messages
notify and the device is not unlinked for inactivity. Bus: an unread
count for the menubar, `AppNotification`. `sgnl://` through `sola-open`.

Size of what we own, by the references: 10–15k lines of engine and store
(libsignal-service-rs + presage are 22k with their own transport;
signalmeow is 14k), plus the UI.

## Slices

1. **Spike, no UI** — link by QR, receive and decrypt, send 1:1, sync
   transcripts, receipts. Proves the stack on the current libsignal tag.
2. **Daily driver** — the app; groups (read state, send with sender
   keys), attachments, profiles and avatars, contacts, reactions,
   replies, edits, deletes, typing, disappearing timers, notifications.
3. **History on link** — the transfer archive the phone offers when
   linking, parsed with `libsignal-message-backup`.
4. **Later** — create and change groups, stories, usernames, stickers,
   link previews, key transparency; calls (RingRTC) last.

Out: payments, donations, registering as the primary device.

## Costs

- **Upkeep.** Pin one libsignal tag; bump it about monthly with Desktop's
  and Android's changes read beside it. A server-side requirement (2026:
  SPQR in March, an HTTP 409 on link in July) stops the client until we
  ship the fix.
- **The receive path is security code.** What to trust is ours to get
  right (presage accepted forged sync messages April–October 2026). Port
  Desktop's checks as written, with tests.
- **AGPL-3.0.** The binary, including the kit linked into it, is AGPL:
  whoever gets the binary may ask for that source.
- **Build.** libsignal builds BoringSSL (Signal's `boring` fork) and its
  protos: it needs libclang, cmake and protoc. Since 2026-11-16 novus has
  Oath's `pkg:libclang` and the tree has its own protoc (`sola-protoc`).
- **Policy.** No sign Signal removes third-party linked devices;
  enforcement is technical (required capabilities, version expiry). Its
  terms are not known to name third-party clients (not verified).

## Decided (Joshua, 2026-11-16: "All this is good")

AGPL-3.0 for the crate; a linked device; Desktop the first reference;
the build tools from Oath (it gave libclang, R91, and advised protox for
protoc). Still open: where the keys rest (the sqlite file, or Vault).

## The 2026-10-31 recommendation (replaced)

signal-cli (Java, GPL-3.0, monthly releases, a fork of Signal-Android's
service code) as the engine behind a `sola-signald` service, with a kit
app as its client and an engine-neutral protocol so presage could replace
it. Rejected 2026-11-16: no Java, no second process. That note also
called building on libsignal directly "you become presage"; that stands,
and is now the accepted cost.

## Sources

- libsignal — <https://github.com/signalapp/libsignal> (`RELEASE_NOTES.md`, `rust/net/chat/src/api/`, `rust/net/grpc/proto/`)
- Signal-Desktop — <https://github.com/signalapp/Signal-Desktop> (`ts/textsecure/`, `protos/`)
- Signal-Android — <https://github.com/signalapp/Signal-Android> (`lib/libsignal-service/`)
- mautrix-signal — <https://github.com/mautrix/signal> (`pkg/signalmeow/`)
- presage — <https://github.com/whisperfish/presage> (#396, #447, #448)
- libsignal-service-rs — <https://github.com/whisperfish/libsignal-service-rs> (#365, #453, #502)
- signal-rusted — <https://github.com/Guilherme-M-Mateus/signal-rusted>
- RingRTC — <https://github.com/signalapp/ringrtc>
- signal-cli — <https://github.com/AsamK/signal-cli>
- gurk-rs 409 on link — <https://github.com/boxdot/gurk-rs/issues/556>
