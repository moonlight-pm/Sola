# sola-bots

**Date:** 2026-09-15  
**Status:** **Frozen** — daemon + iced + HTTP + iOS client (partial)  
**Related:** [call plane](2026-08-13-sola-call-plane-design.md); [browser agent control](2026-09-11-sola-browser-agent-control-design.md); Workspaces is a **different** product ([workspaces freeze](2026-08-13-sola-agent-terminal-design.md))

| | |
|--|--|
| **Implementation** | `crates/sola-bots` (`sola-botsd` + iced HTTP/SSE client); HTTP `:27419` + SSE `GET /events`; iOS `~/Workspace/SolaBot` on Ember (scheme **SolaBot-Release** default); `solactl bots` on sola-call; seat guards |
| **Dogfood** | desk + phone used; ACP dies on `cargo make install bots` |
| **Gaps** | session-roll; Grok not tmux-backed (child of daemon); iOS SSH codesign flaky |

---

## Intent

Named, always-on **informational** LLM sessions that live on this computer.
The operator talks to them from a kit app and from a native iPhone app.
The UI is a **long dialog**, not a PTY.

This is **not coding**. Bots do not sit under `~/Workspace`, do not use git
worktrees, and do not overlap Workspaces. Workspaces remains the coding
agent product (Grok/Codex in PTYs). `crates/sola-agent` stays retired;
this crate is new and is not that GUI.

The machine **is** the sandbox. Official Grok bots get a rented VM; these
bots get **this desk** — browser (`solactl browser`), files, shell — with
`--always-approve`. Soft law (foundation prompt + per-bot home) is the
write fence, not OS sandboxing.

**Example:** a Suno bot that logs into the operator’s Suno account in
sola-browser, generates songs, publishes, and keeps notes in its own
directory.

---

## Locked

| Topic | Choice |
|---|---|
| Name | Crate / daemon **`sola-bots`**. Window **Bots**. Call owner **`bots`**. iOS app **Bots**. |
| Job | Informational / account / research / ops bots. **Not** a coding agent. |
| vs Workspaces | No overlap as the **product**. Default write fence is the bot home; Joshua can order an escape (whole computer / a path outside home). Workspaces never hosts these sessions. `solactl workspaces` stays off-limits unless he asks for the coding rail. |
| vs old sola-agent | Do **not** resurrect `crates/sola-agent`. Daemon owns ACP; UIs are viewers. |
| Runtime (v1) | One long-lived **`grok agent stdio`** (ACP) per bot. Vendor/model selectable later; Grok is the proof. |
| Tools | **Yolo.** `--always-approve` / equivalent. No per-tool prompt in UI or on the phone. |
| Write fence | **Prompt only.** Default: mutate files only under the bot’s home. Joshua can **order an escape** (whole computer / drop the fence / a path outside home) for that task or the session. Seat steal stays forbidden. Hard sandbox is out of v1. |
| Bot home | **`~/Bots/<slug>/`**. Created on bot create. ACP `cwd` is that directory. Not hidden XDG for the working tree. |
| Catalog | `~/.config/sola/bots/catalog.json` |
| Dialog | One long conversation in the UI. Compaction, session roll, and Grok internals are **never** shown. |
| Session roll | TBD (see Open). Likely: persist a `CURRENT.md` (and siblings) in the bot home, start a fresh Grok session when the transcript is too heavy, seed the new session from those files — same spirit as Sola’s progress docs. |
| Process | **Daemon** is the manager (supervisor `MANAGED`, restartable). Iced app and iOS are clients. Closing the window does not kill bots. |
| Desk UI | sola-kit iced. Sidebar of names + transcript + composer. Status mark (idle / working / done) only. No compaction `×N`, no tool-trace chrome as the product. |
| Phone | **Native iOS**, built on the Mac, sideload. Not a PWA first. Same dialog. |
| Remote | Daemon HTTP + stream (WebSocket or SSE) + bearer token. LAN first; WAN is bind + the same token (operator opens a port). Never listen without auth. |
| Call plane | `solactl bots …` (list / new / send / transcript / rm / cancel). Fail if daemon down. Same first-class rule as workspaces/browser. |
| Create | Seed `~/Bots/<slug>/`, start ACP, send a hidden orientation prompt. Assistant intro is the first visible turn. |
| Delete | `rm` drops ACP, catalog row, home directory, and `grok sessions delete` when known. |
| Browser | Bots **call** `solactl browser` on the human profile (agent-control freeze). Skill: prefer a tab group named after the bot; do not steal the seat unless asked. |
| Privilege | Desk-equivalent **except** OS pointer/keyboard **and** compositor screenshots. Bots spawn with `SOLA_BOT=1`. Refused: compositor `screenshot`/`sample`/`input`, `tab.focus`, `tab.open --select`. Browser verbs require `--tab`. Page description is `solactl browser snapshot` (background tabs work). |

---

## Not this

- PTY, tmux, Workspaces tabs, git worktrees, `~/Workspace`.
- Rebuilding the retired ACP chat GUI as the process of record.
- Chat-API toy with a private tool runner (would drop skills / MCP / browser).
- Showing compaction, permissions, or “agent thinking” as first-class UI.
- App Store, push, iCloud.
- Per-tool approval UI (locked yolo).
- OS sandbox / extra user / container in v1.

---

## Processes

```text
  iPhone  ──HTTPS token──┐
                         ▼
                    sola-bots daemon
                    catalog + ACP children + HTTP + call owner
                         │
                         ├── ~/Bots/suno/   grok agent stdio
                         └── ~/Bots/mail/   grok agent stdio

  sola-bots iced  ──HTTP SSE 127.0.0.1:27419──▶  daemon
```

- Supervisor starts the daemon with bus/call, not only with the window.
- Iced app is a launcher row (`app_id=sola-bots`). It does not spawn Grok.
  Live state is HTTP SSE to the daemon, same as the phone.
- Phone never talks Unix sockets.

---

## Bot record

```text
Bot {
  id,              // stable uuid
  name,            // "Suno"
  slug,            // "suno" → ~/Bots/suno
  vendor,          // grok (v1)
  model,           // grok-4.6 default; selectable
  grok_session_id, // current ACP/Grok session; may rotate on roll
  created, last_used
}
```

Foundation files in `~/Bots/<slug>/` (seeded at create, operator-editable):

| File | Role |
|------|------|
| `CURRENT.md` | Living focus for *this* bot (what it is doing) |
| `AGENTS.md` or `RULES.md` | Bot-specific standing instructions (Suno workflow, accounts) |
| (optional later) `docs/` | Longer memory the roll rule can re-seed |

Host-wide foundation (not in the bot dir) is injected every session:
yolo is on; **default** write/edit/delete under `~/Bots/<slug>/` (Joshua
can order an escape); read the rest of the machine; prefer `solactl
browser` for web accounts; not a coding agent by default; seat steal
never lifts.

---

## UI (desk + phone)

One list, one thread.

- List: name + quiet status (working / idle).
- Thread: user and assistant text only. Tool calls fold away or omit.
- Composer: send. No slash-command chrome in v1.
- Create bot: name → slug → directory + catalog row + ACP session.

iOS matches that. No extra phone-only product.

---

## POC order (when implementing)

1. Daemon + catalog + `~/Bots/<slug>` + `grok agent stdio` + yolo + foundation prompt.
2. Iced dialog + `solactl bots`.
3. Local HTTP + token.
4. iOS (Mac) against that HTTP.
5. Session-roll rule once a bot is actually long-lived.

Do not install without express permission.

---

## Open (not blocking the freeze)

1. **Roll trigger** — token/compaction threshold vs a `CURRENT.md` “start fresh”
   convention vs calendar. Implement after a real Suno-class transcript exists.
2. **HTTP bind** — loopback default vs LAN (`0.0.0.0`) behind the token.
3. **iOS auth storage** — Keychain token; how the operator pastes it once.
4. **Second vendor** — after Grok ACP is boring.

---

## Decision log (this freeze)

| Date | Decision |
|------|----------|
| 2026-09-15 | Yolo all tools. Informational, not coding. One long dialog; hide compaction. Per-bot `~/Bots/<slug>/`. No Workspace/worktree overlap. Native iOS, not PWA-first. Crate **Bots**. Soft write fence via foundation prompt. This computer is the sandbox; browser via `solactl browser`. |
| 2026-09-17 | Write fence stays **default home-only**. Joshua can explicitly order an escape (whole computer / drop the fence / a named path outside home). Seat steal still forbidden. |
