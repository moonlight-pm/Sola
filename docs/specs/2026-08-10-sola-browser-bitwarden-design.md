# sola-browser · first-party Bitwarden (D7)

> Status: **design locked + login / match / fill dogfoodable** · 2026-08-10  
> Implementation: custom identity login (`newDeviceOtp`); match picker in
> vault panel; page fill via `Cmd::EvaluateJs` / `sola_wpe_evaluate_js`.  
> Gaps: no session persist, idle lock, auto-offer, save-from-page.  
> Locks product: [`open-questions.md` D4 / D7](../open-questions.md)  
> Priority: [`CURRENT.md`](../../CURRENT.md) · hardening:
> [`plans/2026-08-09-sola-browser-hardening.md`](../plans/2026-08-09-sola-browser-hardening.md)  
> Capability: `browser` in [`capabilities.md`](../capabilities.md)

## 1. Goal

Password manager UX **inside** sola-browser at extension-class quality:
unlock vault, match logins for the active tab URL, fill username/password
into the page. Daily-driver bar (D4), not a demo popup.

### In scope (MVP)

| Capability | Notes |
|------------|--------|
| Account login | Email + master password; **official Bitwarden cloud** (self-host later if needed) |
| Sync | Encrypted vault download + periodic refresh while unlocked |
| Lock / unlock | Master password; idle lock timeout |
| URI match | Bitwarden-style domain matching for current tab URL |
| Manual fill | Toolbar / shortcut → pick match → inject into page |
| Single-match autofill | Optional: when vault unlocked and exactly one match, offer or fill |
| Chrome UI | Iced unlock panel + match picker (kit components) |

### Explicitly deferred (post-MVP)

- Save / update login from page (capture form)
- TOTP copy/display, passkeys / WebAuthn, card / identity fill
- Biometric unlock, PIN unlock, org SSO
- Full vault browser UI (folders, edit ciphers, generators)
- Sharing / Sends / attachments
- Multi-account profiles

## 2. Locked decisions

### Product (D7)

| Do | Don't |
|----|--------|
| Vault + unlock + autofill **in** sola-browser | Chrome/Firefox store package |
| Bitwarden client protocol (crypto client-side) | Separate **user-run** system service or Bitwarden **desktop** bridge |
| Page fill via WebKit inject / `evaluate_javascript` | Full WebExtensions host (for now) |
| Sola chrome UI for password UX | Revisit CEF solely for extensions |

### Architecture (2026-08-10)

| Decision | Choice |
|----------|--------|
| **V1 Backend** | **`bitwarden/sdk-internal` Password Manager crates** (`PasswordManagerClient` and friends) — not Secrets Manager, not `bw` CLI |
| **V2 Process model** | **In-process** inside `sola-browser` (`src/vault/`); async vault worker thread for network/crypto so iced/WPE paint stays responsive |
| **V3 Server** | **Official Bitwarden cloud only** for now (no base-URL / Vaultwarden field) |
| **License** | **Ignored for architecture.** Optimize for dogfood and code quality. Revisit only if/when public distribution requires it. |

Still open for implement time: **V4** autofill default (offer vs single-match auto vs manual-only — default **offer**), **V5** shortcut chord.

## 3. Research summary (what exists)

### 3.1 Wrong tool: Secrets Manager SDK

Public crates (`bitwarden` on crates.io, `bitwarden/sdk-sm`) are **Secrets
Manager** — org machine secrets / API keys. **Not** personal password vault
logins for websites. Do not use for D7.

### 3.2 Right tool: Password Manager client (Rust)

Bitwarden’s SDK monorepo
[`bitwarden/sdk-internal`](https://github.com/bitwarden/sdk-internal) contains
a real PM surface used by their own clients:

| Crate / type | Role |
|--------------|------|
| `bitwarden-pm` · `PasswordManagerClient` | Facade: `auth`, `unlock`, `sync`, `vault`, `generator`, … |
| `bitwarden-core` | `Client`, tokens, API configs, key store |
| `bitwarden-crypto` | Master-key derivation, cipher crypto |
| `bitwarden-unlock` | Unlock methods / session key |
| `bitwarden-sync` | Vault sync |
| `bitwarden-vault` | Cipher/folder models, decrypt views, TOTP helpers |
| `bitwarden-auth` | Identity / login |

Upstream marks this API **internal / unstable** — pin a git rev (or crates
versions when published) and wrap behind a thin `sola-browser` vault façade
so chrome does not thrash on SDK churn.

### 3.3 Rejected for product architecture (not “license”)

| Path | Why not |
|------|---------|
| Secrets Manager SDK | Wrong product |
| `bw serve` / CLI | Extra Node process; user-facing tool dependency; D7 “system service” smell |
| Clean-room reimplementation | Far more cost for same protocol; only if SDK becomes unusable technically |
| Separate `sola-vault` binary | Unneeded complexity once license is out of scope; split later **only** if crash isolation is proven necessary |

## 4. Architecture

### 4.1 Process model (in-process vault module)

```text
┌──────────────────────────── sola-browser ────────────────────────────┐
│                                                                      │
│  iced chrome (main)                                                  │
│   · toolbar key / lock state                                         │
│   · unlock panel + match picker                                      │
│   · Msg::Vault* from worker replies                                  │
│         │ mpsc / oneshot (never secrets on Sola Bus)                 │
│         ▼                                                            │
│  src/vault/  (async worker thread + PasswordManagerClient)           │
│   · login · unlock · lock · sync · matches · fill_fields             │
│   · encrypted state under ~/.local/share/sola/vault/                 │
│         │                                                            │
│         │ FillCredentials { username, password }                     │
│         ▼                                                            │
│  wpe-engine thread ── evaluate_javascript / user-script ──► page     │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

**Why in-process (optimal without license constraints):**

1. One binary, one lifecycle — no socket protocol, no spawn/supervise.
2. Same process as chrome already holds trust for fill; no IPC secret hop.
3. `PasswordManagerClient` is designed to live inside a host app (mobile/web
   clients do the same via UniFFI/WASM).
4. Network + KDF still run off the iced UI thread (dedicated vault worker +
   `tokio` or blocking pool) so unlock/sync cannot freeze paint.

**Optional later split:** if vault panics or memory pressure becomes a
dogfood problem, extract the same `VaultService` trait to a child process
without changing chrome call sites.

### 4.2 Vault façade API (chrome-facing)

Keep chrome ignorant of SDK types. Internal module owns the client.

```text
VaultCmd
  Status
  Login { email, password, server_url? }
  Unlock { password }
  Lock
  Logout
  Sync
  Matches { url }           → Vec<MatchSummary>  // id, name, username, uri — no password
  FillFields { id }         → FillMaterial       // username + password, zeroize after send to engine

VaultEvent / reply
  State { logged_in, unlocked, email?, server_url?, error? }
  Matches(…)
  Filled / FillFailed
```

Rules:

- Master password only lives long enough for Login/Unlock on the vault
  worker; never written to disk or config.
- Match lists **never** include decrypted passwords.
- Idle auto-lock (default 15 min) on the vault worker.
- No vault payloads on Sola Bus or other processes.

### 4.3 Threading

| Thread | Owns |
|--------|------|
| iced main | UI state, toolbar, overlays; sends `VaultCmd`, applies replies |
| vault worker | `PasswordManagerClient`, disk state, HTTP, crypto, match index |
| wpe-engine | WebKit views; runs fill JS on active tab |

Unlock/sync are async relative to UI: show spinner / disable button; never
block the iced update loop on network.

### 4.4 Page inject path (WPE)

As-built: `webkit_web_view_evaluate_javascript` used for copy-selection
(`sola_wpe.c`). No UserContentManager / WebExtension yet.

**Phase I (MVP fill):**

1. Chrome gets `FillMaterial` from vault for chosen cipher.
2. Engine `Cmd::FillCredentials { username, password }` on the **active** tab.
3. Trusted JS snippet finds visible password + username fields
   (`autocomplete`, name/id heuristics), sets `.value`, dispatches
   `input`/`change`.
4. Prefer isolated JS world if available without a full extension; else
   main world with no residual globals.

**Phase II (detection + polish):**

- `WebKitUserContentManager` content script on `document-end` detects login
  forms and notifies chrome (script message / custom handler).
- Badge “N logins”; fill still user-confirmed unless pref says otherwise.

**Phase III (only if needed):** WebKitWebExtension process — packaging cost;
not required for MVP.

### 4.5 Code integration points

| Layer | Change |
|-------|--------|
| New `src/vault/` | Client wrap, worker, match helpers, prefs load/save |
| `Cargo.toml` | Path/git deps on `bitwarden-pm` (+ transitive) pinned |
| `engine.rs` `Cmd` | `FillCredentials { username, password }` |
| `wpe/engine.rs` + `sola_wpe.c` | Eval fill script on active view |
| `app.rs` | Toolbar lock icon, unlock overlay, match popover, shortcut |
| Config | `~/.config/sola/browser-vault.toml` (non-secrets) |

### 4.6 Storage layout

```text
~/.local/share/sola/vault/          # encrypted client state (tokens, sync blob)
~/.config/sola/browser-vault.toml   # server_url, email, lock_timeout, autofill_mode
```

Prefer SDK `save_to_state` / `load_from_state` where they fit; otherwise a
thin adapter around the same directory.

## 5. UX (MVP)

1. **Toolbar** — key icon: locked / unlocked / logged-out.
2. **Logged out** — server URL (default cloud), email, master password →
   Login + Unlock + Sync.
3. **Locked** — master password; Escape closes; error stays open.
4. **Unlocked + shortcut / key click** — match popover for active tab URL;
   Enter fills selection.
5. **No matches** — short empty state (not a vault dump).
6. **Shortcut** — TBD at implement (Bitwarden-ish Ctrl/Cmd+Shift+L if free
   of shell conflicts).

## 6. Security properties (engineering, not license)

| Property | Approach |
|----------|----------|
| Zero-knowledge to server | Client-side crypto; server stores ciphertext |
| Master password | Never on disk; only on vault worker for login/unlock |
| Decrypted vault | In-process memory while unlocked only |
| Fill path | Credentials → engine once; no clipboard by default |
| Logging | No secrets in `tracing` fields |
| Network | TLS; configurable base URL for self-host |

## 7. Implementation phases

| Phase | Deliverable | Exit criteria |
|-------|-------------|----------------|
| **1** | `src/vault/` spike: login, unlock, sync, matches-for-URL (dev harness / logs OK) | Real account vs cloud **or** Vaultwarden |
| **2** | Wire vault worker to chrome state machine (no fill yet) | Status + unlock UI dogfood |
| **3** | Match picker UI | Pick cipher for active tab |
| **4** | `FillCredentials` inject | Fill works on real login pages |
| **5** | Form detection + badge + prefs | Extension-class convenience |
| **6** | Lock timeout, errors, capability/manual docs | D4 Bitwarden subset shippable |

Phases 1–4 = D4 Bitwarden minimum; 5–6 = polish.

### Phase 1 as-built (2026-08-10)

| Piece | Location |
|-------|----------|
| Façade | `crates/sola-browser/src/vault/` (`VaultService`) |
| Worker | `vault/worker.rs` — dedicated thread + tokio; cmds/events |
| Cipher sync handler | `vault/sync_cipher.rs` (upstream PM only registers folder+crypto) |
| URI match | `vault/match_uri.rs` (domain/host/exact/starts_with; no PSL/global domains yet) |
| Chrome | toolbar 🔒/🔓 + modal: email, master password, Sign in (official cloud) |
| SDK pin | `bitwarden/sdk-internal` rev `2c940917…` via Cargo git |

**Dogfood:** open Browser → toolbar lock → sign in with Bitwarden account.

**Still open:** 2FA login; session restore across restarts; match picker + page fill.

## 8. Remaining open (non-blocking)

| ID | Question | Default |
|----|----------|---------|
| **V4** | Autofill: offer / single-match auto / manual only? | **Offer** (badge + picker) |
| **V5** | Keyboard shortcut | Defer; avoid shell Meta chords |
| MFA | How 2FA/login flows surface in UI | Handle when spike hits a 2FA account |

## 9. Non-goals / anti-patterns

- Embedding Bitwarden **web vault** in a WebView as the “integration”
- Driving Bitwarden **Desktop** via IPC
- Requiring user to install `bw` CLI
- Secrets Manager SDK for website passwords
- Shipping a full WebExtensions runtime just for Bitwarden
- Putting vault secrets on Sola Bus
- Designing around license isolation (deferred until public distribution)

## 10. Doc / capability follow-through

When implementing:

1. Update `docs/capabilities.md` `browser` gaps (Bitwarden subset).
2. Update `CURRENT.md` build order as phases land.
3. Operator notes in `docs/manual/` only for **shipped** behavior.
4. Decision log in `open-questions.md` for V1–V3 (done with this freeze).

## 11. References

- D4 / D7: `docs/open-questions.md`
- WPE eval path: `crates/sola-browser/src/wpe/sola_wpe.c` (`sola_wpe_copy_selection`)
- Engine cmds: `crates/sola-browser/src/engine.rs`
- Bitwarden PM SDK: <https://github.com/bitwarden/sdk-internal>
- PM client docs: <https://sdk-api-docs.bitwarden.com/bitwarden_pm/>
- SM SDK (not for this): <https://github.com/bitwarden/sdk-sm>
