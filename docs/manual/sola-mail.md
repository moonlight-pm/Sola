# sola-mail

Kit-native IMAP/SMTP client. **Partial.** Accounts and rules live in
**Settings → Mail**. This app does not edit them.

Accounts with **IMAP** checked are connected. The sidebar is always the
six boxes — Inbox, Sent, Drafts, Archive, Junk, Trash — **combined**
across those accounts, newest first by date. Gmail’s extra labels stay
hidden. If Gmail has no Archive label, Sola creates one on connect.
Move to Trash (and undo) uses **that message’s** account mailbox
(`[Gmail]/Trash` on Gmail, `Trash` on Wicket) and does not block opening
the next letter. Extra accounts may be SMTP-only (IMAP unchecked) when
mail is only forwarded in.
Typing the email fills IMAP and SMTP for known providers (Gmail,
Google Workspace / Gmail Apps via MX, Outlook, Fastmail, Yahoo,
iCloud, Proton). IMAP and SMTP each have an enable checkbox — uncheck
IMAP for send-only. Gmail and Workspace need an **app password**. **Send from** on the inbox is the addresses configured on that server
(Wicket `/api/auth/me`) — check the ones Mail should offer. Extra SMTP
accounts still take typed aliases. Catch-alls (`*@moonlight.pm`) are
omitted. The picker is A–Z. **Default From** is the address new messages
use. Reply picks the identity that appears in the original To/Cc, or
Default From.

## Use

Three columns: mailboxes, message list, letter (or compose). Unread rows
are **bold**. The toolbar stays on; message actions mute until a row is
selected.

- **Compose** — toolbar pen, or Message → New Message (⌘N).
- **Attach** — while composing: toolbar paperclip, **Attach** next to
  Send, or Message → Attach Files… (⌘⇧A). Kit file picker. Files go out
  as `multipart/mixed`.
- **Received files** — paperclip on the list row. In the letter: **Open**
  (images in Paint, everything else in the browser) or **Save** (picker,
  starts in Downloads).
- **j / i / a / d** — Junk / Inbox / Archive / Trash and advance.
  **u** undoes the last move.
- On open, the last inbox list shows immediately. A card at the bottom
  right lists accounts still connecting (or that failed).

Links in the letter open in sola-browser.

## Not in this pass

HTML engine (the letter is converted prose; CID images are files, not
inline pictures), drag-drop onto compose, forward-with-attachments,
full offline store. Mail opens on the last inbox snapshot while
accounts connect (status at the bottom right). IDLE watches Inbox only.
