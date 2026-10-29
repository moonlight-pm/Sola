# Mail

Mail (`sola-mail`, in Oath's `pkg:sola`) is Sola's own IMAP and SMTP
client, in one window. Accounts and rules are set in **Settings → Mail**;
Mail itself does not edit them.

## Accounts

Accounts with **IMAP** checked are connected. The mailboxes are always
the same six (Inbox, Sent, Drafts, Archive, Junk, Trash), **combined**
across those accounts, newest first. Gmail's other labels stay hidden; if
Gmail has no Archive label, Mail makes one when it connects. A move (and
its undo) goes to that message's own account's mailbox (`[Gmail]/Trash`
on Gmail, `Trash` elsewhere) and never holds up opening the next letter.
An account can be SMTP-only (IMAP unchecked), for mail that is only
forwarded in.

In **Settings → Mail**, typing the email fills IMAP and SMTP for known
providers: Gmail, Google Workspace (found through the domain's MX
records), Outlook, Fastmail, Yahoo, iCloud, and Proton. IMAP and SMTP each
have an enable box; uncheck IMAP for send-only. Gmail, Google Workspace,
Yahoo, and iCloud need an **app password**, and Settings says so under the
password. SMTP port 465 uses implicit TLS; other ports use STARTTLS.

**Send from** on the inbox lists the addresses its server reports
(`GET /api/auth/me` on the IMAP host, as a Wicket server answers); check
the ones Mail should offer. Extra SMTP accounts take typed addresses.
Catch-alls (`*@example.com`) are left out, and the list is sorted A–Z.
**Sending → New messages from** is the address a new message uses. A
reply uses the address of yours that the original was sent to (To or
Cc), else that one.

Editing rules in Settings keeps every connection open; only an account
whose IMAP login changed reconnects.

## Rules

**Settings → Mail → Rules** files incoming mail. A rule has conditions on
From, To, or Subject (contains, is, address is, domain is; every
condition must match) and one action:

- **Move to mailbox** (Archive, Junk, Trash, Sent, Drafts, or another):
  Mail runs these rules over the newest 500 Inbox messages when an
  account connects, and over new mail as it arrives.
- **Smart mailbox**: the Inbox messages that match show under a **Smart**
  heading beside the mailboxes, with their unread count, instead of in
  Inbox. The server's search does the matching, so "address is" and
  "domain is" are close but not exact there.

## Use

A bar across the window, the message list under it, and the letter (or
compose) beside the list. The mailboxes show beside the list when the
window is at least 1040 px wide (a Full zone); narrower (a half tile),
the bar's title (**Inbox ⌄**) opens them as a menu, and the **Mailbox**
menu in the titlebar lists them too. The titlebar's title names the
mailbox and its unread (`Inbox · 12 unread`), or the search.

- **Bar**: the mailbox, its unread count (or what a search found), and a
  ring while mail is being checked; at the right the search field,
  **Erase** in Junk and Trash, and **New message**.
- **List**: unread mail has an accent dot and a bolder sender; a flag
  marks flagged mail; a paperclip marks attached files. Sent and Drafts
  show who the message went to. The date is the time today, `Yesterday`,
  the weekday this week, then `4 Sep` / `4 Sep 2024`. With more than one
  account, each row names the inbox it came to (`gmail`); mail forwarded
  in shows `↳` and the address it was sent to. Long senders and subjects
  end in `…`. Scrolling to the end loads older mail.
- **Letter**: the subject, then the sender, their address, and the local
  date and time on one line, then who else it went to. Under that:
  **Reply**, **Reply All**, **Forward**; Archive, Junk, and Trash (and
  **Move to Inbox** in Junk, Trash, and Archive) as icons; Mark as read or
  unread and Flag at the right. In Drafts, **Edit Draft** takes the
  replies' place. Links open in Browser. Attached files have **Open** and
  **Save**: pictures open in Paint, `.ics` invites in Calendar (as a new
  event you file in one of your calendars), everything else in Browser.
- **HTML letters** are drawn by Mail itself, safely: paragraphs and line
  breaks as the sender wrote them; bold, italic, and struck text; links,
  headings, quotes, lists, rules; tables as rows. Nothing in a letter
  runs and its styles are not applied, so a newsletter reads as one
  column. Pictures carried in the message show; **pictures from the web
  are hidden** until you press **Load Pictures** (fetching them tells the
  sender you opened the letter). A dark logo made for white mail sits on
  a light plate.
- **After a move** the message above is selected and, a moment later,
  opened and marked read. A toast, **Moved to** the mailbox, with **Undo**
  and **Dismiss**, floats over the foot of the list for 5 seconds; the
  list does not move for it. A check of the server already under way
  does not undo your changes: Mail keeps its own read, unread, and flag
  marks until the server's list shows them too.
- **Search**: type in the bar's field (**/** or ⌘F goes there) and press
  Enter. It searches Inbox, Sent, Archive, and Drafts on every account
  (plus Junk or Trash when that mailbox is open), up to 200 matches.
  Escape (twice if the field has focus) or **×** clears it.
- **Copy Message** (Edit menu) copies the open letter.

While Mail runs, the top row shows a mail badge with Inbox's unread
count; click it to bring Mail forward (see [the shell](sola-shell.md)).

### Keys

| Key | Action |
|-----|--------|
| **x** / **i** / **a** / **d** | Move to Junk / Inbox / Archive / Trash |
| **u** | Undo the last move |
| **w** / **s**, or **↑** / **↓** | Previous / next message |
| **/** | Search |

These keys do nothing while you are typing in a field.

| Menu | Items |
|---|---|
| Mail | Get New Mail (⌘⇧N), Quit Mail (⌘Q) |
| Edit | Cut, Copy, Paste, Select All (⌘X / ⌘C / ⌘V / ⌘A), Copy Message, Search Mail (⌘F) |
| Mailbox | Inbox, Sent, Drafts, Archive, Junk, Trash, Erase Junk Mail…, Erase Deleted Items… |
| Message | New Message (⌘N), Save Draft (⌘S), Attach Files… (⌘⇧A), Reply (⌘R), Reply All (⌘⇧R), Forward (⌘⇧F), Mark as Read / Unread (⌘⇧U), Flag / Unflag (⌘⇧L), Archive, Move to Inbox, Move to Junk, Delete, Undo Move |
| View | Next Message, Previous Message |
| Window | The Window menu every Sola application has |

**Edit** works in the field you are typing in (To, Cc, Subject, Search)
at its caret or selection. With no field focused it works on the draft's
body, or on the open letter (Copy with nothing selected copies the whole
letter).

### Compose

- **Reply** goes to the sender's Reply-To when there is one (mailing
  lists), else the sender. **Reply All** adds the original To and keeps
  the original Cc, minus your own addresses.
- **Forward** brings the original's files along (remove any you don't
  want).
- **Attach** (next to Send) or ⌘⇧A opens Sola's file panel; choose one
  file or several (Shift, ⌘). Files go out as `multipart/mixed`. **Save**
  on a received file asks before it replaces one already there.
- **Send** shows *Sending…* until the server takes the message; a failure
  keeps the draft open. If the server refuses a recipient, the message
  names the address and the server's reason;
  `5.1.2 Mailbox does not exist` means there is no mailbox for that
  address there. Failures are
  also logged, in `sola.log` and `app-sola-mail.log` under
  `~/.local/state/sola/`.
- **Save Draft** files the message in Drafts and closes compose. Open it
  from Drafts and press **Edit Draft** to go on; sending (or saving again)
  replaces the saved copy.
- **Fields**: **Tab** and **Shift+Tab** move To → Cc → Subject → body
  (Tab in the body types a tab). Enter in To or Cc takes the top
  suggestion, else moves on. A new message or a forward starts in To; a
  reply starts in the body.
- **Suggestions**: typing two or more letters in To or Cc lists people
  you have mail from or to (name or address match; no-reply senders left
  out). Click one, or press Enter for the top one.
- **Send** checks each address first and marks the one it can't read
  under its field.
- **Escape**, **Cancel**, or clicking another message with an edited
  draft shows **Discard this draft?** over the compose pane: **Keep
  Editing**, **Save Draft**, or **Discard**. Escape first closes the
  suggestions or the From menu.
- The **Message** menu stays live while you compose: Archive, Delete, and
  Move to Junk act on the selected message (the draft stays open); Reply
  and Forward replace an untouched draft or ask about an edited one.

### Erasing

Junk and Trash have **Erase** in the bar (and Mailbox → **Erase Junk
Mail…** / **Erase Deleted Items…**). Each asks first (**Cancel** /
**Erase**), then permanently deletes everything in that mailbox on every
account.

### Connecting

Mail opens on the last list it showed, at once. A card at the bottom
right lists accounts still connecting or that failed. Failed accounts try
again on their own (after 15 seconds, growing to 5 minutes), or now with
**Retry Now** or Get New Mail.

Switching to the Mail window checks the server right away (the open list
and the mailbox counts). New mail also comes in on its own: Mail watches
each Inbox (IMAP IDLE) and checks every 45 seconds. While a check is out,
a ring sits beside the count; hover it for **Checking mail**.

## Files

| Path | What |
|------|------|
| `~/.config/sola/state.yaml` (`MailConfig`) | Accounts, Send from addresses, and rules, set in Settings; passwords age-encrypted with `~/.config/sola/key` |
| `~/.local/state/sola/mail/snapshot.json` | The last list, so Mail opens on it |

## Limits

- No web engine: a letter's stylesheets are not applied, and nothing in
  it runs.
- No pictures from the web without **Load Pictures**, and no "always load
  pictures from this sender".
- No dragging files onto compose.
- No offline store: only the last list is kept.
- Only Inbox is watched for new mail; other mailboxes update when you
  open them or Mail checks.
