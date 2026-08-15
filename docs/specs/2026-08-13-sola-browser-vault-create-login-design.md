# sola-browser vault — create login

**Date:** 2026-08-13  
**Status:** **Frozen** — implemented and dogfooded (landed on master 2026-08-13)  
**Related:** vault module (`crates/sola-browser/src/vault/`); [profiles](2026-08-10-sola-browser-profiles-design.md); [manual](../manual/sola-browser.md)

## Intent

Signup pages need a login that **exists in Bitwarden before the site ever sees the password**. Other managers generate first and hope a save prompt fires after submit — the password is often lost. Sola writes the cipher first, then fills.

## Product rule

A generated password never exists only in the page. **Create** (or Enter) persists a personal login on the official Bitwarden cloud, then fills. Abandoning signup leaves a leftover vault item, never a password that only lived in a tab.

## Surface

Same 360px kit modal, top-right under the vault icon. Unlock, 2FA, and passkey pick are unchanged (no Create on those).

### Fill list (unlocked default)

- Title **Fill login** + host subtitle when there are matches.
- Empty: compact card (no reserved 420px scroller). Copy: **No saved login for this site.** Primary action: **Create login**.
- Matches: existing rows (name, username, passkey badge). Click fills and closes. **Create login** stays visible (ghost/secondary) with **Close**. **Refresh** stays a quiet control.
- Size follows content. Tall scroller only when there are matches.

### Create login (replaces the list)

Cancel returns to the list. Nothing is written until Create or Enter.

| Field | Default | Focus |
|---|---|---|
| Username | Last username filled or created (`vault.json`) | Selected on open — typing replaces |
| Password | Fresh generated password (visible, editable) | **Regenerate** on the field row |
| URL | Bare registrable domain of the current page (`accounts.google.com/signup` → `google.com`) | Editable |

No separate name field. Bitwarden item name is that host (`google.com`), or `Login` if the URL is empty. Blank / `sola:` pages leave URL empty.

**Password generator:** 16 characters, at least one upper, lower, digit, and symbol. No options panel.

### After Create / Enter

1. Create the personal login on Bitwarden (name, username, password, one URI, default domain match).
2. Only after that succeeds: fill the best username field and **every visible password field** (including confirm).
3. Remember this username as last used; touch the new cipher in `last_used`.
4. Fields found → close the panel (same as fill). None found → stay on **Saved to vault** with Close. The item is already real.

Save error → stay on the form with the message. Do not fill.

## Architecture

- Chrome phase `CreateLogin` (plus `CreateSaved` when persist succeeded and the page had no fields).
- `VaultCmd::CreateLogin` → `VaultService::create_login`: encrypt a `CipherView` login, `POST /ciphers`, then sync so the in-memory repo sees it.
- Same worker / session as unlock and fill. Vault prefs stay **shared across profiles**.
- Apex via the existing `match_uri` base-domain helper (two-label + `co.uk` / `com.au` hack).
- Fill script reports whether it found fields (`__sola_vault_fill__`) so chrome can choose close vs Saved.

## Out of scope

Edit/delete, lock, folders, orgs, notes, generator options, passkey **create**,
per-profile vault, **create-card**. Cards **fill** is a sibling chrome surface
(separate toolbar button + panel), not this freeze.

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Create login chrome + save-then-fill | **done** (dogfooded 2026-08-13) |
| Generator + apex + fill-all passwords | **done** |
| History / downloads | not this slice |
