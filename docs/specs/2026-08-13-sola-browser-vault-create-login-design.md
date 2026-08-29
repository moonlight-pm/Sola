# sola-browser vault — create login

**Date:** 2026-08-13  
**Status:** **Frozen** — implemented and dogfooded (landed on master 2026-08-13)  
**Related:** vault module (`crates/sola-browser/src/vault/`); [unified panel](2026-08-28-sola-browser-vault-panel-design.md); [profiles](2026-08-10-sola-browser-profiles-design.md); [manual](../manual/sola-browser.md)

## Intent

Signup pages need a login that **exists in Bitwarden before the site ever sees the password**. Other managers generate first and hope a save prompt fires after submit — the password is often lost. Sola writes the cipher first, then fills.

## Product rule

A generated password never exists only in the page. **Create** (or Enter) persists a personal login on the official Bitwarden cloud, then fills. Abandoning signup leaves a leftover vault item, never a password that only lived in a tab.

## Surface

Same hanging kit modal, top-right under the vault icon. Unlock, 2FA, and passkey pick are unchanged (no Create on those). The unlocked default is the [unified vault panel](2026-08-28-sola-browser-vault-panel-design.md) (search + records). **+** opens this create-login form.

### Unlocked vault (browse)

Create is the **+** on the unified panel (primary empty state is still this form via **+**). Autofill **Fill** on a URI match injects and closes; a row click opens the item record.

### Create login (replaces the list)

Cancel returns to vault browse. Nothing is written until Create or Enter.

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
per-profile vault, **create-card**. Card/identity fill and notes live on the
[unified vault panel](2026-08-28-sola-browser-vault-panel-design.md).

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Create login chrome + save-then-fill | **done** (dogfooded 2026-08-13) |
| Generator + apex + fill-all passwords | **done** |
| History / downloads | not this slice |
