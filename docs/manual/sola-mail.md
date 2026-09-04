# sola-mail

Kit-native IMAP/SMTP client. **Partial.** Account and rules live in
**Settings → Mail**. This app does not edit them.

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

Links in the letter open in sola-browser.

## Not in this pass

HTML engine (the letter is converted prose; CID images are files, not
inline pictures), drag-drop onto compose, forward-with-attachments,
offline store. IDLE watches Inbox only.
