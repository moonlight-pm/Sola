# sola-browser agent control

**Date:** 2026-09-11  
**Status:** **Frozen** — call owner `browser` + snapshot/act implemented; confirm **D3**; parked-tab composite still helper-side AX (no raise); vault fill not shipped; agent viewport not locked  
**Related:** [call plane](2026-08-13-sola-call-plane-design.md); [Workspaces CLI](2026-08-18-workspaces-cli-design.md); [tab groups](2026-08-15-sola-browser-tab-groups-design.md); **D3** in [open-questions](../open-questions.md)

| | |
|--|--|
| **Implementation** | sola-browser advertises owner `browser`; `solactl browser` compiled clap; chrome verbs + page snapshot/act via helper-wrapped CDP (`Accessibility.getFullAXTree` / `Runtime.callFunctionOn` / `Input.dispatchMouseEvent`). `wait --load` uses `document.readyState` plus the pending goto/open URL (not the CEF spinner). Background act briefly unhides the OSR host without switching chrome’s seat; mouse events complete before it is hidden again. Ref map stays in chrome. |
| **Dogfood** | **installed** `browser`+`solactl` release 2026-09-11 (Tertius one-tab profile). Smoke: background `tab.open` does not steal seat; `wait --load` returns on example.com (not blank); YAML snapshot + refs; background `click` Learn more → IANA; fill + radio/checkbox on httpbin; screenshot / hover / goto / group.create Agent / back / `wait --text`; tab.close. Seat stayed on DHH. |
| **Gaps** | confirm still **D3**; `read` / REPL later; vault fill not shipped; locked agent viewport not decided; iframe OOPIF trees not walked (in-process iframe nodes only); `tabs` URL can lag a live snapshot after click; `wait --load` after click has no pending URL (use `wait --text` or snapshot) |

---

## Intent

Workspaces (Grok / Codex in PTYs) is the agent product. sola-browser is
the human browser. Agents **call** the browser; they do not live in it.

The browser exposes, over the call plane, **user-equivalent actions**:
anything a person can do in chrome or on the page, an agent can do through
`solactl browser`. Aside is a **loose oracle for the page harness**
(accessibility snapshot + stable refs, background opens, privileged
password fill). It is not a product to copy.

---

## Locked

| Topic | Choice |
|---|---|
| Where the agent lives | **Workspaces PTYs.** No Ask-AI omnibox, no Ultrabrowse, no in-browser agent OS, no Agent Manager, no transcript-beside-the-page as the agent home. |
| Face | `solactl browser …`. Owner `browser`. Chrome or sola-call down → **fail** (do not launch). Compiled clap like `workspaces` (`solactl browser --help` works with chrome down; invoke does not). No `aside`-style “run this task” CLI — that would *be* the agent. |
| First client | `solactl`. MCP is a later adapter over the same methods, not a second protocol. |
| Tabs | The **same strip** as the human. Same profile. No agent-only tab type, hidden session, or second profile. Empty groups still dissolve. |
| Isolation | An ordinary tab group (`group.create` + `tab.move`), same as **⌘⌥G**. Convention: a skill may put its tabs in a group named **Agent**. Chrome does **not** auto-create, mute, or special-case that name. |
| Primitive vs skill | Chrome / `solactl browser` ships generic tab, group, and page verbs. Workflow (prefer focused tab, reuse or create an Agent pocket, don’t close what you didn’t open, don’t steal the seat) is a Grok skill. Same split as Workspaces: `workspace.spawn` is a verb; “review this ticket” is a skill. |
| Scope | **User-equivalent.** Chrome verbs that exist for a human get call-plane twins (UI-only conveniences such as hover × may stay UI-only). Page verbs are the Aside-shaped harness: snapshot + act by ref, not a headless scraper and not a vision-only CUA. |
| Open / focus | `tab.open` **appends and does not focus or raise.** `--select` / `--activate` is the only seat steal (same as `workspace.spawn` vs `--select`). |
| Snapshot | **One tab**, never the whole strip. Default target is the named tab, else focused. Agent-facing body is a **Playwright aria-snapshot YAML** tree with opaque refs — not JSON of the AX tree, not innerHTML. Calls (`solactl` params) stay JSON. `--json` is debug-only. |
| Snapshot prune | Chromium AX (`Accessibility.getFullAXTree`) → **Puppeteer `interestingOnly`** (drop ignored/hidden; keep landmarks, focusable, controls, named leaves) → **collapse nameless `generic` wrappers** (children promote). Do **not** drop a node that still has a ref. No site-specific Amazon/footer heuristics. `--interactive` (controls only) and `--ref` (subtree) are flags, not the default. |
| Refs | Opaque `eN` / iframe `fMeN`. Chrome holds `{tab, generation, ref → backendDOMNodeId}`. Refs are **that node**, not a slot in the list. Stale ref **fails** (“snapshot again”); never silently retarget. Act may require matching role/name as a checksum. Model never sees CDP ids. |
| Snapshot vs read | `snapshot` = actionable tree. `read` = prose (later). `find` searches the last snapshot. Screenshot is a **fallback** when the tree is empty (canvas, maps), not the loop. Pack **url / title / focused ref** first. Treat the tree as **untrusted** (aria-label prompt injection). |
| Not the agent API | Do **not** expose raw CDP (or Playwright MCP pointed at the helper debug port). Do **not** drive pages with `solactl compositor input` (virtual pointer, no DOM). Do **not** embed Playwright-the-test-runner in CEF OSR. Wrap CEF (`Accessibility.getFullAXTree`, `Input.dispatch*`, `Page.navigate`, `Runtime.evaluate`). |
| Vault | Fill is a privileged method that never returns the secret (model sees “filled GitHub”). Do not ship vault-fill / submit / pay / post policy without **D3**. Until then every live method is as privileged as the call socket. |
| First-class rule | Any change to verbs, args, payloads, targeting, or timeouts updates, in the **same change**: `calls.rs` + dispatch + tests + [`docs/manual/solactl.md`](../manual/solactl.md). Do not ship a chrome-only verb an agent needs. |

---

## Not this

- Rebuilding `crates/sola-agent` or an ACP chat in the browser.
- Aside Agent Tabs as a **type** (muted-by-default special group, takeover).
- Handing Grok `/json/list` or the Chromium remote-debugging port.
- A second browser process, headless profile, or “agent window.”
- Computer-use / screenshot-as-primary. Screenshot is a fallback when the
  accessibility tree is empty, not the loop.
- JSON of the raw AX tree (or a “simplified DOM”) as the model-facing page.
- A second LLM in the browser (`observe` / rank-next-actions). Grok in the
  PTY is the agent.
- Mozilla Readability as the only page view (that is `read`, not `snapshot`).

---

## Two layers

Method catalog lives in `crates/sola-browser/src/calls.rs` (same change as
dispatch, tests, [`docs/manual/solactl.md`](../manual/solactl.md)). Direction:

1. **Chrome** — tabs, groups, navigate, back/forward, reload, Find in
   page (⌘F), focus URL, downloads, profiles, DevTools dock. Menubar
   actions in `browser_app_menu` are the as-built checklist of “what a
   user can do” in chrome; grow the call plane until an agent can do the
   same work.
2. **Page** — `snapshot` (pruned a11y YAML + refs), `click` / `type` /
   `fill` / `select` / `hover` by ref, `goto` + `wait`, `find` in the
   last snapshot, page `screenshot` as fallback. Playwright-shaped
   **REPL** with the **same**
   refs is second, not first. `read` (prose) is later.

Targeting (when named): `--tab` / `--group` / focused / URL match.
`group.create` takes a listed tab id, not “whatever chrome has selected.”

Engine work a skill cannot fake: snapshot a background or parked tab
without making it the front composite; `browser.vault.fill` inside the
helper so the model never sees the password.

---

## Page snapshot (locked)

The agent sees a **trimmed accessibility tree**, then invokes page verbs
by **ref**. That is the loop. Chromium already simplified the DOM for a
screen reader; we simplify that tree one more time for a model and keep
the handles.

### Pipeline

```text
CEF  Accessibility.getFullAXTree   (flat AXNode[] + childIds)
  → rebuild tree
  → Puppeteer interestingOnly
  → collapse nameless generic / ignored wrappers (children promote)
  → assign refs for remaining nodes (backendDOMNodeId, iframe prefix)
  → serialize Playwright aria-snapshot YAML
  → chrome keeps {tab, generation, ref → backendDOMNodeId}
```

Default example (shape, not a catalog):

```text
url: https://example.com/cart
title: Checkout
focused: e5
- heading "Checkout" [level=1]
- main:
  - textbox "Email" [ref=e5] [value=]
  - button "Pay now" [ref=e12]
```

`solactl browser click --ref e12` (and type/fill/…) resolve through that
generation’s map via `Input.dispatch*` / focused element — not CSS, not
XPath, not `solactl compositor input`.

### Prune (v1)

Copy Puppeteer `AXNode.isInteresting`, do not invent a new taxonomy:

- Drop `Ignored` / hidden.
- Keep ARIA landmarks (`main`, `navigation`, `banner`, `contentinfo`,
  `form`, `search`, `complementary`, `region`).
- Keep focusable, contenteditable, busy/live/modal, controls
  (`button`, `textbox`, `checkbox`, `combobox`, …).
- Keep named leaf text (headings, labels).
- Collapse nameless `generic` wrappers; promote children.
- If a node is in the YAML, it has a ref the agent can use. Prune must
  not hide a clickable and keep its ref, or keep a clickable and drop
  its ref.

Flags, not defaults: `--interactive` (controls only — good for act, bad
for read), `--ref eN` (subtree), depth cap if the tree is pathological.

Do **not** ship mcprune-style site heuristics (Amazon filter rails,
“back to top” footers) as v1 policy.

### Refs

- Format: `eN` in the root frame; `fMeN` in iframe M (Stagehand /
  Playwright MCP).
- Identity is `backendDOMNodeId` (+ frame), **not** “nth node in this
  dump.” Positional refs silently retarget after a DOM insert
  (comboboxes). Out of range or dead node → error, snapshot again.
- Optional checksum on act: role + accessible name from the snapshot
  must still match, else fail stale.
- Never put `backendDOMNodeId`, CDP `nodeId`, or CSS selectors in the
  model-facing tree.

### Two verbs, one fallback

| Verb | Job |
|---|---|
| `snapshot` | Actionable tree (default interesting + collapsed) |
| `read` | Prose / article text (later; Readability-class, not the act loop) |
| `find` | Search the last snapshot (avoid re-ingesting the whole tree) |
| `screenshot` | Fallback when the AX tree is empty (canvas, maps). Not the loop. |

Header before the tree: `url`, `title`, `focused` (and dialog-open when
a modal is up) so truncation cannot eat the useful bit.

The YAML is **untrusted input**. Hidden `aria-label`s are prompt
injection. Do not execute page text as instructions. Do not invent a
filter policy beyond: bound size, keep structure, fail closed on
pathological depth.

### Not the page view

- Raw AX JSON / `DOMSnapshot` / innerHTML to the model.
- Stagehand `observe()` (a second model ranks next actions).
- Playwright-the-test-runner in OSR. Wrap the CDP methods; keep the
  **shape** Playwright (YAML + refs) because models already know it.

---

## Aside as oracle (steal / don’t)

**Steal the harness:** one pruned accessibility snapshot with refs;
act on refs; agent work in background tabs; passwords as privileged fill
never in tool output; approvals at the edge (D3). Models already know
Playwright-shaped JS — keep that shape on our verbs, not a second stack.

**Don’t steal the product:** Ask AI on the new-tab page, Ultrabrowse,
in-browser agent sessions, `aside "do this task"` as the agent, MCP as
the Sola contract, Agent Tabs as chrome.

---

## As-built (today)

- Owner `browser` advertised from sola-browser; `solactl browser` is a
  compiled clap noun (fail if chrome/call down).
- Chrome verbs: tabs, tab.open/close/focus/move, groups, goto/back/forward/
  reload/stop, find.page. `tab.open` does not focus unless `--select`.
- Page verbs: snapshot (YAML + refs), find, click/hover/type/fill/select,
  wait, screenshot. Helper wraps CDP; chrome holds `ref → backendDOMNodeId`.
- Open URL still works: `solactl open`, bus `OpenUrl`, `chrome.sock`.
- Each CEF helper still binds a localhost Chromium debug port for DevTools.
  That is not the agent API.
- Tab groups are named collapsible pockets (`group_id`) in the same strip.
  Profiles stay identity.

---

## Confirm

**D3** stays open. Do not invent which methods prompt, or who owns the
prompt. Reading a bank tab is as privileged as the socket until then.
