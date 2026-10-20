# sola-mail

Kit-native IMAP/SMTP client. **Partial.** Accounts and rules live in
**Settings → Mail**. This app does not edit them.

Accounts with **IMAP** checked are connected. The sidebar is always the
six boxes — Inbox, Sent, Drafts, Archive, Junk, Trash — **combined**
across those accounts, newest first by date. Gmail’s extra labels stay
hidden. If Gmail has no Archive label, Sola creates one on connect.
Moves (and undo) use **that message’s** account mailbox
(`[Gmail]/Trash` on Gmail, `Trash` on Wicket) and never hold up opening
the next letter. Extra accounts may be SMTP-only (IMAP unchecked) when
mail is only forwarded in.

Typing the email fills IMAP and SMTP for known providers (Gmail,
Google Workspace / Gmail Apps via MX, Outlook, Fastmail, Yahoo,
iCloud, Proton). IMAP and SMTP each have an enable checkbox — uncheck
IMAP for send-only. Gmail and Workspace need an **app password**. SMTP
port 465 uses implicit TLS; other ports use STARTTLS. **Send from** on
the inbox is the addresses configured on that server (Wicket
`/api/auth/me`) — check the ones Mail should offer. Extra SMTP accounts
still take typed aliases. Catch-alls (`*@moonlight.pm`) are omitted. The
picker is A–Z. **Default From** is the address new messages use. Reply
picks the identity that appears in the original To/Cc, or Default From.

Editing rules in Settings keeps every connection open; only an account
whose IMAP login changed reconnects.

## Use

Three columns: mailboxes, message list, letter (or compose).

- **List** — unread rows are **bold**; a flag marks flagged mail; a
  paperclip marks files. Sent and Drafts show who the message went to.
  The date is the time today, `Yesterday`, the weekday this week, then
  `4 Sep` / `4 Sep 2024`. With more than one account, each row names the
  inbox it came to (`gmail`); mail forwarded in shows `↳` and the
  original recipient.
- **Letter** — the Date is the local calendar day with the local clock
  (`16:00`) under it. Links open in sola-browser. **Open** / **Save** on
  attached files (images in Paint, everything else in the browser).
- **Toolbar** — New message, Attach, Reply, Reply all, Forward, then
  Archive / Trash / Junk (and **Move to Inbox** in Junk, Trash and
  Archive), then Mark as read/unread, Flag, Copy, Undo. In Drafts,
  **Edit draft** replaces the reply buttons.
- **Search** — type and press Enter. Searches Inbox, Sent, Archive and
  Drafts on every account (plus Junk or Trash when that box is open),
  up to 200 hits. Esc (twice if the field has focus) clears it.

### Keys

**x / i / a / d** — Junk / Inbox / Archive / Trash and advance.
**u** undoes the last move. **w / s** or **↑ / ↓** — previous / next.
These keys do nothing while you are typing in a field.

Menus: ⌘N new, ⌘S save draft, ⌘⇧A attach, ⌘R reply, ⌘⇧R reply all,
⌘⇧F forward, ⌘⇧U mark read/unread, ⌘⇧L flag, ⌘⇧N get new mail.

### Compose

- **Reply** goes to the sender’s Reply-To when there is one (mailing
  lists), else the sender. **Reply all** adds the original To and keeps
  the original Cc, minus your own addresses.
- **Forward** brings the original’s files along (remove any you don’t
  want).
- **Attach** — toolbar paperclip, **Attach** next to Send, or ⌘⇧A. Kit
  file picker. Files go out as `multipart/mixed`.
- **Send** shows *Sending…* until the server takes it; a failure keeps
  the draft open.
- **Save Draft** files it in Drafts and closes compose. Open it from
  Drafts and press **Edit draft** to continue; sending (or saving again)
  replaces the saved copy.
- **Esc**, **Cancel**, or clicking another message with an edited draft
  asks first: Keep Editing, Save Draft, or Discard.

### Erasing

Trash and Junk have an erase button in the list header (and Mailbox →
Erase…). Both ask before permanently deleting everything in that box on
every account.

### Connecting

Mail opens on the last list immediately. A card at the bottom right
lists accounts still connecting or that failed; failed accounts retry on
their own (15 s, growing to 5 minutes) or now with **Retry Now** /
Get New Mail.

## Not in this pass

HTML engine (the letter is converted prose; CID images are files, not
inline pictures), drag-drop onto compose, full offline store. IDLE
watches Inbox only. Edit → Paste goes into the compose body only.
