# Signal client — plan

**Started:** 2026-11-16, on Joshua's word ("All this is good -- go ahead").
**Branch:** `signal` (`.worktrees/signal`), not merged, nothing installed;
merged into `next` (613e7b77) for the overlay.
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

## Direction (Joshua, 2026-11-16, after the first link and a Note to Self)

"lets skip these intermediate tests -- just build out the full client and
we'll test it 'in production' so to speak". So the client is built whole
and tried in real use; what is below says of each piece whether it has run
against Signal or only against libsignal in a unit test.

## Built

One process: the window on the main thread, the engine on its own
(`engine/`), the store between them. The window reads the store and is told
what changed; what is to be sent goes to the store's outbox first and
leaves when there is a connection, across restarts.

- [x] **Build:** `cargo make build sola-signal`, with nothing outside the
      tree but Oath's libclang.
- [x] **Link:** the code to scan in the window (or `sola-signal link` in a
      terminal), the phone's keys, the device registered. Linking again
      (after the phone removed the device) keeps the conversations if it is
      the same account. *Against Signal: linked to Joshua's phone
      2026-11-16 (device 5); the window's code shown from Signal on a
      private desk.*
- [x] **Receive:** every envelope kind (sealed sender, pre-key, double
      ratchet, sender key, plaintext notice), kept before it is
      acknowledged; a replay is dropped; a sync message only from our own
      account; what does not decrypt is asked for again (a
      `DecryptionErrorMessage`). *Against Signal: signs in, uploads
      pre-keys, received and kept a group message. Unit tests against real
      libsignal sessions.*
- [x] **Send:** text, replies, attachments, to a person or a group (one
      send per member; sender keys later), sealed when we hold the
      recipient's profile key and signed in when not; the server's device
      corrections; the transcript to our other devices; a resend when a
      device says it could not read us. *Against Signal: a Note to Self
      reached the phone. Nothing has been sent to another account yet.*
- [x] **Conversations:** messages, edits, deletes for everyone, deletes on
      this device, reactions, quotes, mentions as names, disappearing
      timers (kept, counted from reading, removed when up), delivery and
      read states of what we sent, delivery receipts out, read receipts out
      when the account sends them, reads told to our other devices and
      taken from them, typing shown, calls as a line ("answered on your
      phone"), view-once as a line. *Unit tests of the store logic; none of
      it seen against another account yet.*
- [x] **People and groups:** the account's record on the storage service
      (contacts with profile keys, groups, blocked, read receipts), the
      phone's contacts sync, profiles (names, decrypted), group states
      from the group server (zkgroup credential; name, members, timer).
      *Against Signal: 43 people (33 named), the groups read; a group the
      account has left answers 403 and stays unnamed.*
- [x] **Attachments:** fetched and decrypted (pictures and sound by
      themselves, the rest when asked), encrypted and uploaded (both of
      Signal's upload protocols). *Cipher unit-tested; `sola-signal
      selftest` put a file on Signal's file server (the tus protocol) and
      took the same bytes back. No file has gone in a message yet.*
- [x] **The window:** conversations with unread counts and drafts, search
      that also finds people to write to, the thread (bubbles, day lines,
      names in groups, pictures, files, reactions, replies), the composer
      (Enter sends, Shift+Enter a new line, files), a picked message's
      actions (react, reply, copy, edit, delete), menus (Edit, the
      disappearing timer), a desk notification per conversation, the
      unread count in the window's title. *Looked at on a private desk
      over an invented store (`sola-signal demo`, `SOLA_SIGNAL_OFFLINE=1`).*

## Not built

- The app stops receiving when its window closes (it is an
  `iced::application`, as Mail is). A daemon that stays is next.
- History from before the link (`backup5` and `libsignal-message-backup`).
- Pictures of people and groups (discs with an initial stand in), stickers
  as packs, link previews, voice notes recorded here, polls and pins as
  more than a line of text.
- Sender keys for groups (a group message is one send per member today),
  group changes as lines in the thread, creating or changing a group.
- Typing told to others; signed pre-keys rotated (Desktop does every two
  days; ours stay as registered).
- Phone numbers and usernames looked up (a conversation starts from
  someone the account already knows).
- Stories, payments, calls (RingRTC), key transparency.
- Applied anywhere. The package and the launcher's entry are written (see
  Packaging) and wait for Joshua's word for the apply.
- Where the keys rest: still the sqlite file in the clear, mode 0600.

One `sola-signal` signs in at a time (`signal.lock` beside the store): the
server closes a device's connection when it opens another, and two
processes would write the same sessions.

## Packaging

- `pkg:sola-signal` in `sola-make`'s `PACKAGES`, its build packs `cmake`
  (BoringSSL), `git` (boring-sys patches its copy with `git apply`) and
  `libclang` (bindgen) beside the usual ones. `plan:sola-build` builds
  `sola-protoc` first and sets `PROTOC`, `LIBCLANG_PATH=/lib` and bindgen's
  clang arguments, as `signal_tools` does for a build on the desk;
  `cargo make oath stage` takes the same environment.
- **2026-11-16: `cargo make oath publish --local` passed from the branch
  (8a77a48e)**: every plan built in the sandbox, `pkg:sola-signal`
  6cc2ce03a164 among them (51 MB; it runs, and loads only the C and C++
  runtimes). The vendored crates needed no change to the pruning.
- The launcher's entry ("Signal", `lucide/message-circle`) is in
  `sola-shell`'s `builtins.rs`: it reaches a desk with `pkg:sola`.
- On novus the overlay makes the package (`cargo make oath apply sola-signal
  sola` from `next`; `pkg:sola` restarts the desk). Other machines get it
  only once it is published and the Oath session has put it in the desk
  profile, with the AGPL-3.0 note.

## Following Signal

- libsignal: one tag in `crates/sola-signal/Cargo.toml`, bumped about
  monthly, with `RELEASE_NOTES.md` read.
- Protos: `crates/sola-signal/proto/README.md` names the Signal-Android
  commit they were copied from.
- Signal's server parameters (`engine/server_public_params.b64`) and trust
  roots (`receive.rs`) are Signal-Desktop's `config/production.json`; its
  TLS roots are libsignal's.
- What a linked device must declare (`link.rs` `capabilities`) and the
  link URL follow Signal-Desktop's `getCapabilities` and `Provisioner`.

## Known costs of the merge (when it comes)

- `Cargo.lock`: rustls 0.23.43 → 0.23.46, rustls-webpki, 
  rustls-platform-verifier 0.7.0 → 0.7.1, tokio 1.53.1 → 1.54.1, aes-gcm
  0.11.0 → 0.11.1 for the whole workspace; `[patch.crates-io]` gains
  Signal's `boring` fork.
- A whole-workspace `cargo make build` then builds BoringSSL (cmake) and
  needs libclang: on NixOS, a from-source build needs both.
- Every release build compiles BoringSSL too, and the lock's bumps move
  every package's hash: the first publish after the merge moves them all
  (each app starts again, and the desk).
- `cargo test -p sola-signal` needs the same environment by hand (there is
  no `cargo make test`).
