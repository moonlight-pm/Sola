# Vault — parked

**Status:** parked (Joshua, 2026-10-31, answering the gaps after Vault
shipped: "Put this in ideas docs", "Ideas", "Ideas / deferred"). Vault
itself is the [freeze](../specs/2026-10-31-sola-vault-design.md); do not
implement these from this file — promote one into a slice first.

**Declined:** vault search in the launcher ("Don't want this").

## An SSH agent from the vault

Bitwarden's SSH-key items served on `SSH_AUTH_SOCK` by `sola-vaultd`
(`bitwarden-ssh` is in the SDK). Every signature asks in a shell prompt
naming the host and the key, so an agent in a terminal cannot sign
silently; allow once, or for a while for one host. Open: what the prompt
looks like, whether `ssh-add -L` lists keys while locked, how a Projects
session on another computer reaches it (forwarding the socket).

## Mail passwords from the vault

A Mail account names a vault item instead of keeping its own password
(Settings → Mail: "Password from Vault"). The grant is made once, by you,
for that account; Mail redeems it from the service at connect time without
a prompt. Open: how the grant is stored and revoked, and what Mail shows
while the vault is locked (connect waits, or the account says why).

## The rest of "everything"

- **Import** — the SDK's importers (`bitwarden-importers`: Bitwarden JSON,
  CSV, KeePass, 1Password, LastPass, …) behind File → Import… in Vault; a
  new `Import` request.
- **Attachments** — download (save through the kit file panel) and add;
  records list them today. New requests; the SDK's attachment encryption.
- **Send** — Bitwarden Send (text and files with an expiry and a link).
  Deferred in V7.

## Other applications' windows (V4)

A key that opens the right-click menu at the pointer over any window —
Chromium, Steam, a foreign terminal — whose Vault values are typed through
River's virtual keyboard (no clipboard). Open: the key, typing non-ASCII
through a keymap, refusing secure-input contexts we cannot see.

## Small ones

- **The username generator** has no options of its own (it borrows the
  passphrase's). Bitwarden offers random word, plus-addressed email,
  catch-all, and forwarded-email aliases.
- **The shell's own fields** (the launcher's query and the like) have no
  right-click menu: the bus never sends an app its own message, so the
  shell would open its menu for its own fields directly.
- **Memory locking** — `sola-vaultd` locks its memory only when the
  memlock limit is unlimited, and Oath's service entries have no way to
  raise it (an Oath feature: an rlimit field on `svc:`).
