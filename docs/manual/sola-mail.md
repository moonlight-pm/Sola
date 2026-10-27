# Mail

Mail is Sola's native IMAP/SMTP mail client. Accounts and rules live in
**Settings → Mail**; Mail itself does not edit them.

## Accounts

Accounts with **IMAP** checked are connected. The sidebar is always the
six boxes (Inbox, Sent, Drafts, Archive, Junk, Trash), **combined**
across those accounts, newest first by date. Gmail’s extra labels stay
hidden. If Gmail has no Archive label, Sola creates one on connect.
Moves (and undo) use **that message’s** account mailbox
(`[Gmail]/Trash` on Gmail, `Trash` on a Wicket server) and never hold up
opening the next letter. Extra accounts may be SMTP-only (IMAP
unchecked) when mail is only forwarded in.

In **Settings → Mail**, typing the email fills IMAP and SMTP for known
providers (Gmail, Google Workspace / Gmail Apps via MX, Outlook,
Fastmail, Yahoo, iCloud, Proton). IMAP and SMTP each have an enable
checkbox; uncheck IMAP for send-only. Gmail and Workspace need an **app
password**. SMTP port 465 uses implicit TLS; other ports use STARTTLS.

**Send from** on the inbox lists the addresses configured on that server
(Wicket `/api/auth/me`); check the ones Mail should offer. Extra SMTP
accounts still take typed aliases. Catch-alls (`*@moonlight.pm`) are
omitted. The picker is sorted A–Z. **Sending → New messages from** is the
address new messages use. Reply picks the identity that appears in the
original To/Cc, or that one.

Editing rules in Settings keeps every connection open; only an account
whose IMAP login changed reconnects.

## Use

A bar across the window, the message list under it, and the letter (or
compose) beside the list. The mailboxes show beside the list when the
window is at least 1040 px wide (a Full zone); in a half tile the bar's
title (**Inbox ⌄**) opens them as a menu, and **Mailbox** in the
titlebar lists them too. The titlebar names the mailbox and its unread
(`Inbox · 12 unread`), or the search.

- **Bar**: the mailbox, its unread count (or what a search found), and a
  check ring while mail is being fetched; at the right the search field,
  **Erase** in Junk and Trash, and **New message**.
- **List**: unread mail has an accent dot and a bolder sender; a flag
  marks flagged mail; a paperclip marks files. Sent and Drafts show who
  the message went to. The date is the time today, `Yesterday`, the
  weekday this week, then `4 Sep` / `4 Sep 2024`. With more than one
  account, each row names the inbox it came to (`gmail`); mail forwarded
  in shows `↳` and the original recipient. Long senders and subjects end
  in `…`.
- **Letter**: the subject, then the sender, their address and the local
  date and time on one line, then who else it went to. Under that:
  **Reply**, **Reply All**, **Forward**; Archive / Junk / Trash (and
  **Move to Inbox** in Junk, Trash and Archive) as icons; Mark as
  read/unread and Flag at the right. In Drafts, **Edit Draft** replaces
  the replies. Links open in Browser. **Open** / **Save** on attached
  files (images open in Paint, `.ics` invites in Calendar as a new event
  you file in a calendar, everything else in Browser). Copy Message is
  in **Edit**; Undo is on the toast after a move (and **u**).
- **HTML letters** are drawn by Mail itself, safely: paragraphs and line
  breaks as the sender wrote them, bold and italic, links, headings,
  quotes, lists, rules, and tables read as rows. Nothing in a letter
  runs, and its styles are not applied, so a newsletter reads as one
  column. Pictures carried in the message show; **pictures from the web
  are hidden** until you press **Load Pictures** (fetching them tells the
  sender you opened the letter). A dark logo made for white mail sits on
  a light plate.
- **Search**: type in the bar's field (**/** or ⌘F goes there) and press
  Enter. Searches Inbox, Sent, Archive and Drafts on every account (plus
  Junk or Trash when that box is open), up to 200 hits. Esc (twice if the
  field has focus) or × clears it.
- Accounts still connecting, or failing, show on a card at the bottom
  right with **Retry Now**.

### Keys

| Key | Action |
|-----|--------|
| **x / i / a / d** | Junk / Inbox / Archive / Trash, and advance |
| **u** | Undo the last move |
| **w / s** or **↑ / ↓** | Previous / next message |
| **/** | Search |

These keys do nothing while you are typing in a field.

Menus: ⌘N new, ⌘S save draft, ⌘⇧A attach, ⌘R reply, ⌘⇧R reply all,
⌘⇧F forward, ⌘⇧U mark read/unread, ⌘⇧L flag, ⌘⇧N get new mail, ⌘F
search.

### Compose

- **Reply** goes to the sender’s Reply-To when there is one (mailing
  lists), else the sender. **Reply all** adds the original To and keeps
  the original Cc, minus your own addresses.
- **Forward** brings the original’s files along (remove any you don’t
  want).
- **Attach**: **Attach** next to Send, or ⌘⇧A. Opens
  the kit file picker. Files go out as `multipart/mixed`.
- **Send** shows *Sending…* until the server takes it; a failure keeps
  the draft open. If the server refuses a recipient, the message names
  the address and the server's reason. `5.1.2 Mailbox does not exist`
  means there is no mailbox for that address there. Failures are also
  written to `/opt/sola/log/app-sola-mail.log`.
- **Save Draft** files it in Drafts and closes compose. Open it from
  Drafts and press **Edit draft** to continue; sending (or saving again)
  replaces the saved copy.
- **Fields**: **Tab** / **Shift+Tab** move To → Cc → Subject → body
  (Tab inside the body types a tab). Enter in To / Cc takes the top
  suggestion, else moves on. A new message or forward starts in To; a
  reply starts in the body.
- **Suggestions**: typing two or more letters in To / Cc lists people
  you have mail from or to (name or address match; no-reply senders
  left out). Click one, or press Enter for the top one.
- **Send** checks each address first and marks the one it can’t read
  under its field.
- **Esc**, **Cancel**, or clicking another message with an edited draft
  shows a **Discard this draft?** card over the compose pane: Keep
  Editing, Save Draft, or Discard. Esc first closes suggestions or the
  From menu.
- The **Message** menu stays live while composing: Archive / Delete /
  Move to Junk act on the selected message (the draft stays open); Reply /
  Forward replace an untouched draft or ask about an edited one.

### Erasing

Trash and Junk have an erase button in the bar (and Mailbox →
Erase…). Both ask before permanently deleting everything in that box on
every account.

### Connecting

Mail opens on the last list immediately. A card at the bottom right
lists accounts still connecting or that failed. Failed accounts retry on
their own (after 15 s, growing to 5 minutes), or now with **Retry Now** /
Get New Mail.

Switching to the Mail window checks the server right away (the open
list and the mailbox counts); new mail also arrives on its own in the
background. While a check is out, a small ring sits beside the message
count; hover it for **Checking mail**.

## Limits

- There is no HTML engine. The letter is converted to prose; CID images
  are attached files, not inline pictures.
- No drag-and-drop onto compose.
- No full offline store.
- IDLE watches Inbox only.
- Edit → Paste goes into the compose body only.
