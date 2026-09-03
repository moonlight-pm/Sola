# IPC contract compatibility (mixed worktree installs)

**Status:** idea (parked 2026-08-30). Do not implement from this file. Promote
into a freeze + plan + `CURRENT.md` **Now** when work starts.  
**Trigger:** dogfood installs from several `.worktrees/` into `/opt/sola/bin`.
Bus, sola-river, and apps can drift; the desktop looks up and does the wrong
thing.  
**As-built pain:** `TopicKind` is postcard-encoded in `$subscribe`. Inserting a
variant above existing ones **silently remaps** later discriminants (this broke
Super+Tab). Subscribe decode failure is swallowed (`if let Ok(kinds)`). Clients
stay “connected” and deaf. The process manager then restarts every non-zero
exit, so a hard failure would flap instead of explaining itself.

Do **not** try to make mixed contracts work. Mixed *binaries* are fine when the
contracts match (UI-only `install shell` against an older bus). Contract drift
must fail in one place with a dedicated exit code and a sentence.

---

## The graph (client → host)

Checks are not app ↔ app.

```text
                    sola-bus          sola-call         river (compositor)
                    bus contract      call contract     Wayland XML
                         ▲                 ▲                  ▲
         ┌───────────────┼─────────────────┼──────────────────┘
         │               │                 │
    sola (PM)      sola-river         sola-shell / session / kvm / kit apps / solactl
                   (bus+call+wayland)
```

| Contract | What must match | Who |
|---|---|---|
| **bus** | socket framing + `Message` + `TopicKind` + topic payloads | every Sola process except the bus host |
| **call** | JSON `Wire` framing | river, session, kit providers, monitor, solactl |

`cargo make install river` is the **bridge** crate (`sola-river`), not the
compositor. `/opt/sola/bin/river` comes from Nix. Treat compositor XML as a
follow-up: if required `river_*` globals are missing, exit the same way. Do not
hash the River ELF in v1.

Cargo `0.1.0` and git SHA are the wrong IDs. SHA is too strict (a chrome-only
shell would refuse an older bus). A forgotten manual generation number is too
loose.

---

## Path: preamble + content hash, then die

### 1. Compile-time contract hashes (build.rs, no bump ritual)

- **bus** = hash of `message.rs`, `transport.rs`, `topic.rs`, `topics.rs`, plus
  the sola-core types that ride payloads (`theme/types.rs`, `applications.rs`,
  `keys.rs`). Declare the file list in `sola-bus/build.rs`; if a payload moves,
  add the file.
- **call** = hash of `protocol.rs` + `transport.rs`. Method arg JSON is
  advertised live; drift there is already a remote error.

Same tree ⇒ same hashes ⇒ mixed install is allowed. Any IPC file change ⇒ new
hash ⇒ old binaries fail on connect.

### 2. Fixed binary preamble, before postcard/JSON

Identify/subscribe cannot carry this: they *are* the fragile encoding.

Something like 32 bytes: magic `SOLA`, plane (`B`/`C`), preamble version,
16-byte hash. Client writes, host replies ok or mismatch + a UTF-8 reason, then
closes.

Old binaries that do not speak the preamble fail the same way instead of
hanging on a postcard parse.

### 3. Exit 76 (`EX_PROTOCOL`) with a TTY/log line, not a reconnect loop

```text
incompatible bus contract
  this   sola-shell  bus=9f3a…c1  git abc1234-dirty
  host   sola-bus    bus=12ab…d0  git def5678
  hint   cargo make install shell     # same tree as the running bus
         or cargo make install bus    # from this tree
```

Bake git identity only for that message (`--compat` on every binary via
`sola-core`).

Handshake mismatch must **not** look like “socket not up.” Today
`connect_blocking` and the kit poller retry forever. Mismatch is fatal: log,
`_exit(76)`. Same on mid-session reconnect (`install bus` from another tree).

### 4. Supervisors must not flap

`sola` currently restarts every managed exit. **76 = do not restart**; log once.
Core chrome can stay down (no menubar) rather than a crash loop.

`sola-session` should reap 76, emit `UserAppExited`, optionally toast, and not
relaunch.

### 5. Install can warn; runtime is the source of truth

`cargo make install shell` from a tree whose bus hash ≠ the installed
`sola-bus` should print the same mismatch. Do not block a combined
`install bus shell …` (restart order already puts bus first). Do not require a
full-tree install when the hash is unchanged.

---

## What this does *not* try

- Per-topic versions, protobuf, or making postcard forward-compatible.
- Requiring every binary from the same git commit.
- Versioning call *methods* (catalog already fails cleanly).
- Hashing iced/kit UI, CEF, or tmux.
- Teaching the bus to serve two `TopicKind` layouts at once (that is how
  Super+Tab went silent).

---

## Failure modes after this

| You install | Contract changed? | Result |
|---|---|---|
| `shell` only, UI | no | works (today’s happy path) |
| `shell` only, `topics.rs` | yes | shell exits 76; rest stay |
| `bus` only, `topics.rs` | yes | clients reconnect, all exit 76, PM does not flap |
| `bus shell river …` from one tree | yes | settle order brings a matching set up |

---

## First slice (when promoted)

`sola-core` preamble + exit helper; bus/call handshake; fatal (not retry)
connect; PM/session 76 policy; kit/`solactl` wiring; a unit test that a
`topics.rs` byte change flips the hash. River-global missing → 76 can land in
the same change or immediately after.

Product: mixed trees stay legal; mixed **contracts** die on the socket with a
code and a sentence, instead of a desktop that looks up and does the wrong
thing.
