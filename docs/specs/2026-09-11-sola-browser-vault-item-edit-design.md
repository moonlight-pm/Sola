# sola-browser vault — edit item

**Date:** 2026-09-11  
**Status:** **Frozen** — implemented; **installed** `browser` release 2026-09-11; desk smoke pending  
**Related:** [unified panel](2026-08-28-sola-browser-vault-panel-design.md); [create login](2026-08-13-sola-browser-vault-create-login-design.md); [manual](../manual/sola-browser.md)

## Intent

A vault item you can open, you can correct. Signup leftovers, rotated
passwords, a new website URI, and notes live in the same hanging card —
not a trip to bitwarden.com.

## Product rules

| Rule | Choice |
|------|--------|
| Entry | **Edit** on the item record (ghost, next to **Fill**). Hidden when the cipher is not writable (`edit: false`). |
| Surface | Same hanging card. Back / Cancel returns to the record; nothing is written. **Save** or Enter persists, then the record. |
| Types | Every kind the record already shows: login, card, identity, note, SSH, bank, license, passport. |
| Fields | Name, type-specific rows (empty allowed so you can fill blanks), websites on logins (**Add website** / **Remove**), notes, custom fields (**Add field** / **Remove**). |
| Secrets | Password **Regenerate** (same 16-char generator as create). Fields are visible on the edit form (the record already showed them). Authenticator is the TOTP secret (`otpauth://` or the key). |
| Passkeys | Badge only. FIDO2 credentials are not edited here. |
| Orgs | Writable org items edit in place. Create login stays personal-only. |
| History | Password (and hidden custom) changes prepend Bitwarden password history (cap 5). |
| Dates | License / passport dates are `YYYY-MM-DD`. Bad input stays on the form with the message. |
| Out of this slice | Delete; create card/identity/note; folders/collections; generator options; attachments. |

## Architecture

- Chrome phase `ItemEdit` holds an `ItemDraft` (plain strings).
- `VaultCmd::UpdateItem` → `VaultService::update_item`: load `CipherView`, `apply_draft`, encrypt, `PUT /ciphers/{id}`, sync.
- Apply is in-place so passkeys, attachments, collections, and linked fields survive.
- URI match type is kept per existing website; new URIs default to domain match.

## Gaps

- Desk smoke (personal login, org item, custom field, TOTP secret).
- Delete.
- Create card / identity / note.

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Record **Edit** + save/cancel | **done** |
| Login / card / identity / note / SSH / bank / license / passport | **done** |
| Custom fields add/remove | **done** |
| Password history on change | **done** |
| Desk smoke | pending |
