# sola-browser vault — unified panel

**Date:** 2026-08-28  
**Status:** **Frozen** — implemented; **installed** `browser --release` 2026-08-28; desk smoke pending  
**Related:** [create login](2026-08-13-sola-browser-vault-create-login-design.md); [manual](../manual/sola-browser.md)

## Intent

One toolbar vault, like the official Bitwarden extension: search the
whole vault, open a full item record (notes, identity, card, custom
fields, TOTP), and fill from there. Not three exclusive widgets
(login / authenticator / cards).

## Product rules

| Rule | Choice |
|------|--------|
| Toolbar | **One** vault control. Locked = lock. Unlocked = key. Shield when this page has a TOTP login. Fingerprint during a passkey ceremony. Accent wash while the panel is open. |
| Panel | Same hanging card (top-right under the icon). Unlock, 2FA, passkey, create-login, browse, and item view share it. |
| Browse | Search (name, username, URI, notes, identity names/email, card brand/last4, text custom fields). Type chips: All / Login / Card / Identity / Note. |
| Autofill | URI-matching **logins** at the top when search is empty. **Fill** on the row injects and closes. Clicking the row opens the record. |
| Item view | Whole decrypted record: labelled fields, copy, reveal on secrets, notes, custom fields, live TOTP + remaining seconds. **Fill** for login / card / identity. TOTP **Fill** copies and injects the code. |
| Create | **+** still creates a personal login (existing create-login freeze). |
| Passkeys | Unchanged: get picker / create confirm take over the same panel. |
| Out of this slice | Edit/save item; create card/identity/note; generator tab; Bitwarden Send; folders/collections as first-class nav; always-show-cards/identities-in-autofill setting. |

## Search (not Lunr)

Case-insensitive AND of whitespace words over a haystack built on the
worker. Haystack **never** includes password, PAN, CVC, TOTP secret,
SSN, or SSH private key.

## Surfaces

Kit graphite card, 400px. Search + chips, then a scroller. Item view
is back + name + Fill, then copy-rows (label, value, copy; eye on
hidden). Type is a 14px lucide glyph on the row (key, card, user,
sticky-note, …), not a second toolbar.

## Gaps

- Desk smoke of org-vault list + identity fill + notes.
- No item edit.
- SSH / bank / passport / license are view + copy only (no page fill).

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| One toolbar icon + contextual lock/key/shield/fingerprint | **done** (installed `--release`) |
| Search + type chips + all-item list | **done** |
| Item record (notes, identity, card, custom, TOTP) | **done** |
| Login / card / identity fill from record | **done** (desk smoke pending) |
| Create login **+** | **done** (existing freeze) |
