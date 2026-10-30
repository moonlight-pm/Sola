# Security

Sola's defaults trust whoever is at the keyboard and whoever is on the
network. Each way into a running desk can be closed. This chapter lists them,
what each does by default, and how to close it. The design is in the
[secure access spec](../specs/2026-10-29-secure-access-design.md).

| Way in | Default | Closed by |
|---|---|---|
| The keyboard, at boot | The desk starts signed in | A password, with **Sign in automatically** off |
| The keyboard, while you are away | Nothing locks | Super+L, or **Lock when the windows have faded** |
| The console, after **Exit Sola** | The console opens signed in | A password, with **Sign in automatically** off |
| The network, through the KVM | The peer takes input and clipboard from anyone | [Pairing](sola-kvm-operator.md#pairing) |

The password, the lock and the console login need Oath's login service
(`svc:login`, Oath R42 and later). Without it there is no password: the desk
starts signed in, Super+L says **Locking needs Oath's login service**, and
Settings → Login says the service is not running.

## The password

**Settings → Login** sets, changes or removes the desk's password. Oath keeps
it (hashed), not Sola; `oath passwd` sets it from a console too. Changing or
removing it needs the current one. None is set by default.

**Sign in automatically** (on by default) decides whether a password is
asked for as the desk starts and when the console opens after Exit Sola. With
a password set, changing it needs the current password typed above it.

A password with **Sign in automatically** on still lets you lock by hand:
the desk starts signed in and never locks on its own.

## Locking

Super+L, or **Lock Screen** in the flower menu, locks the screen. With no
password set, a toast says **Set a password in Settings → Login to lock**.

**Settings → Appearance → Idle → Lock when the windows have faded** (off by
default) locks once the idle fade is complete.

With auto-login off and a password set, the desk is locked as it starts.

The lock screen (`sola-lock`) shows the background picture under a dark veil,
the time and the date, and a password field. Return checks the password,
Backspace erases a character, and Escape or Ctrl+U clears the field. Keys
typed while **Checking…** shows are ignored. Oath answers a wrong password
after a delay that doubles each time, up to 30 s. A password typed through a
paired KVM works like the local keyboard.

While locked, the compositor (`ext-session-lock-v1`) shows and sends input
to the lock screen only. If `sola-lock` dies, the screen stays locked and the
shell starts it again, up to five times a minute.

## Exit Sola

**Exit Sola** closes your apps and ends the desk. On Oath it then tells
Oath's login service that the desk ended on purpose, and the console gets
tty1: signed in with auto-login on, asking for the password with it off.
`oath desk` brings the desk back.

## Limits

- Sola's own processes trust each other and anything else running as you:
  the bus and the call socket are only as private as your user account.
- The lock screen draws without the kit (iced cannot make lock surfaces); it
  follows the picture but not the theme's colors.
- The lock does not cover another computer's screen through the KVM; lock
  that computer on its own.
