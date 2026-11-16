# Signal client — plan

**Started:** 2026-11-16, on Joshua's word ("All this is good -- go ahead").
**Branch:** `signal` (`.worktrees/signal`), not merged, nothing installed.
**Research and why this shape:** [`../ideas/2026-10-31-signal-client.md`](../ideas/2026-10-31-signal-client.md).

One binary, `sola-signal` (`crates/sola-signal`, AGPL-3.0-only): a linked
device of the phone, its service layer ours, on Signal's libsignal at one
pinned tag (v0.108.0). No Java, no second process, no presage.

## Decided (Joshua, 2026-11-16)

- AGPL-3.0 for this crate and binary.
- A linked device; the phone stays the primary.
- References, in order: Signal-Desktop (it is the linked desktop device),
  Signal-Android (the protos come from it), mautrix-signal's `signalmeow`.
- The build tools: Oath's `pkg:libclang` (Oath R91) for bindgen; `protoc` is
  ours (`crates/sola-protoc`, on protox). `cargo make build` sets both up
  (`crates/sola-make/src/signal_tools.rs`).

Open: where the keys rest (the sqlite file as it is, or sealed by Vault).
Today the file holds them in the clear, mode 0600.

## Slice 1 — spike, no window

- [x] Builds on novus from `cargo make build sola-signal`, with nothing
      outside the tree but Oath's libclang.
- [x] `sola-signal link [name]`: the provisioning socket through
      libsignal-net, the QR code in the terminal, the phone's keys decrypted
      (libsignal-provisioning), the device registered (`PUT /v1/devices/link`)
      with a signed pre-key and a last-resort Kyber pre-key per identity, all
      kept in `~/.local/state/sola/signal/signal.db`. **Linked to Joshua's
      phone 2026-11-16 (device 5).**
- [x] `sola-signal receive`: signs in, tops up one-time pre-keys, decrypts
      each envelope (sealed sender, pre-key, double ratchet, sender key,
      plaintext notice) and prints one line for it. **Against Signal: signs
      in, uploads pre-keys, drains the queue (empty). Decrypting is tested
      against a real libsignal session in a unit test; no message from
      Signal has come through it yet.**
- [x] The first real link (Joshua scanned the code, 2026-11-16).
- [ ] The first message from Signal read by `receive`.
- [x] `sola-signal send <me|account id> <text>`: the recipient's pre-keys
      (`GET /v2/keys/<id>/*`, signed in), a session per device, the server's
      corrections (missing, gone, registered again) and a retry, then the
      transcript to our other devices; through libsignal-net-chat's typed
      `send_message` / `send_sync_message`. To `me` it is a Note to Self:
      the transcript alone. **A Note to Self was accepted by Signal's
      server; tested against real libsignal sessions in a unit test. Not
      yet sent to another account.**
- [ ] Sealed sends: keep each contact's profile key (it arrives on their
      messages), derive the access key, fetch our sender certificate.
- [ ] A conversation's disappearing timer on what we send (needs the
      conversation kept; today a send carries none).
- [ ] Delivery and read receipts out; a retry request when a message does
      not decrypt (today it is logged and dropped).
- [ ] Decrypt and keep a message in one transaction, acknowledged after.
- [ ] Ask the phone for contacts, blocked, configuration and keys.

One `sola-signal` signs in at a time (`signal.lock` beside the store): the
server closes a device's connection when it opens another, and two
processes would write the same sessions. `send` leaves what the server
delivers on its connection unanswered, so it comes again to `receive`.
The app is one process with one connection for both.

## Slice 2 — daily driver (the app)

A kit app drawn as Mail is: conversations, a thread, a composer. It keeps
running with its window closed (`iced::daemon`). Messages and
conversations in the store; names and avatars (profiles, contacts sync,
storage service); groups (state by zkgroup, sending with sender keys);
attachments; reactions, replies, edits, deletes, typing, disappearing
timers; the unread count and notifications on the bus; `sgnl://`.

## Slice 3 — history on link

Advertise `backup5` in the link URL, take the transfer archive the phone
offers, parse it with `libsignal-message-backup`.

## Later

Create and change groups, stories, usernames, stickers, link previews, key
transparency; calls (RingRTC) last. Packaging: `pkg:sola-signal` (a new
name: the Oath session), the plan build in `oath build`'s sandbox.

## Following Signal

- libsignal: one tag in `crates/sola-signal/Cargo.toml`, bumped about
  monthly, with `RELEASE_NOTES.md` read.
- Protos: `crates/sola-signal/proto/README.md` names the Signal-Android
  commit they were copied from.
- What a linked device must declare (`link.rs` `capabilities`) and the
  link URL follow Signal-Desktop's `getCapabilities` and `Provisioner`.

## Known costs of the merge (when it comes)

- `Cargo.lock`: rustls 0.23.43 → 0.23.46, rustls-webpki, 
  rustls-platform-verifier 0.7.0 → 0.7.1, tokio 1.53.1 → 1.54.1, aes-gcm
  0.11.0 → 0.11.1 for the whole workspace; `[patch.crates-io]` gains
  Signal's `boring` fork.
- A whole-workspace `cargo make build` then builds BoringSSL (cmake) and
  needs libclang: on NixOS, a from-source build needs both.
- `cargo test -p sola-signal` needs the same environment by hand (there is
  no `cargo make test`).
