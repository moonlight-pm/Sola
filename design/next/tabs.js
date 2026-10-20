/* Board 06: one tab list. The master bar is the only tab strip; apps lose
 * their own. A zone holds tabs of any kind and shows one; its titlebar
 * belongs to whatever it is showing. The workspace switcher is back in the
 * top row, and the bar lists only the workspace showing. */
{
  const set = (id, html) => { const el = document.getElementById(id); if (el) el.innerHTML = html; };
  const S = 1296 / 2560;
  const view = (html, c) => {
    const s = c.s || 1;
    return `<div class="crop" style="width:${c.w}px;height:${c.h}px"><div style="position:absolute;left:${-c.x * s}px;top:${-c.y * s}px;transform:scale(${s});transform-origin:0 0">${html}</div></div>`;
  };

  /* Shell bodies for terminal tabs */
  const SHELLS = {
    "~/Workspace/Sola": [
      `<span class="c">~/Workspace/Sola</span> <span class="m">master</span>`,
      `❯ git status --short`,
      ` M CURRENT.md`,
      `<span class="g">??</span> design/next/`,
      `❯ ls design`,
      `agent-desk  foundation  instrument  islands  next  solar  tokens.css`,
      `❯ <span class="cur"></span>`,
    ],
    "cargo make build sola-kit": [
      `<span class="c">~/Workspace/Sola</span> <span class="m">master</span>`,
      `❯ cargo make build sola-kit`,
      `   <span class="g">Compiling</span> sola-core v0.1.0`,
      `   <span class="g">Compiling</span> sola-bus v0.1.0`,
      `   <span class="g">Compiling</span> sola-kit v0.1.0`,
      `    <span class="g">Finished</span> \`release\` profile [optimized] in 41.2s`,
      `❯ <span class="cur"></span>`,
    ],
  };
  const LOG = [
    `<span class="d">17:42:08</span> <span class="i">INFO</span> sola_river: output connected connector=DP-1 mode=5120x2160@120`,
    `<span class="d">17:42:08</span> <span class="i">INFO</span> sola_river: composition windows=4 hidden=6`,
    `<span class="d">17:42:11</span> <span class="i">INFO</span> sola_river: zone assign app=sola-browser zone=center`,
    `<span class="d">17:42:19</span> <span class="w">WARN</span> sola_river: shadow commit late frame=8812`,
    `<span class="d">17:42:20</span> <span class="i">INFO</span> sola_river: composition windows=4 hidden=6`,
    `<span class="d">17:43:02</span> <span class="i">INFO</span> sola_river: focus app=sola-projects`,
  ];

  /* What each kind of tab is, and the app whose titlebar it wears */
  const KIND = {
    agent: { ic: "bot", app: "Projects", menus: ["Project", "Edit", "Window"], body: () => `<div class="term">${termLines().join("\n")}</div>` },
    browser: { ic: "globe", app: "Browser", menus: ["Edit", "Profiles", "Window"], body: () => browserBody({ noSide: true }) },
    terminal: { ic: "terminal", app: "Terminal", menus: ["Edit", "View", "Window"], body: (t) => `<div class="term">${(SHELLS[t.t] || []).join("\n")}</div>` },
    log: { ic: "terminal", app: "Terminal", menus: ["Edit", "View", "Window"], body: () => `<div class="log">${LOG.join("\n")}</div>` },
    mail: { ic: "mail", app: "Mail", menus: APPS.mail.menus, body: () => mailBody() },
    calendar: { ic: "calendar", app: "Calendar", menus: APPS.calendar.menus, body: () => calendarBody() },
    preview: { ic: "image", app: "Preview", menus: ["Window"], body: () => previewBody() },
  };
  const appOf = (t) => {
    const k = KIND[t.kind];
    return { name: k.app, menus: k.menus, title: t.t, body: () => k.body(t) };
  };

  const SOLA = {
    idx: 0, name: "Sola", zones: [
      { k: "Left", z: 0, tabs: [
        { kind: "agent", t: "kit-foundation · claude", m: mark.ring(11), show: true, focus: true },
        { kind: "agent", t: "menubar-pass · codex", m: mark.diamond(11) },
        { kind: "agent", t: "mail-focus · claude", m: mark.check(11) },
      ] },
      { k: "Center", z: 1, tabs: [
        { kind: "browser", t: "ext-background-effect-v1", show: true },
        { kind: "browser", t: "river-window-management-v1" },
        { kind: "terminal", t: "~/Workspace/Sola" },
        { kind: "terminal", t: "cargo make build sola-kit" },
      ] },
      { k: "Right", z: 2, tabs: [
        { kind: "log", t: "tail -f sola-river.log", show: true },
        { kind: "browser", t: "sola · GitHub" },
      ] },
      { k: "Floating", z: "float", tabs: [{ kind: "preview", t: "invoice-0412.png", show: true }] },
    ],
  };
  const MAIL = {
    idx: 1, name: "Mail", zones: [
      { k: "Left", z: 0, tabs: [{ kind: "mail", t: "Inbox · 12 unread", show: true, focus: true }] },
      { k: "Center", z: 1, tabs: [
        { kind: "calendar", t: "October 2026", show: true },
        { kind: "browser", t: "Hetzner · Invoice 0412" },
      ] },
      { k: "Right", z: 2, tabs: [] },
      { k: "Floating", z: "float", tabs: [] },
    ],
  };

  /* A copy of ws with `title` showing (and focused) in its zone. */
  const pick = (ws, title) => ({
    ...ws, zones: ws.zones.map((z) => {
      const has = z.tabs.some((t) => t.t === title);
      return { ...z, tabs: z.tabs.map((t) => ({ ...t, focus: t.t === title, show: has ? t.t === title : t.show })) };
    }),
  });

  /* The bar: this workspace's zones and their tabs. */
  function tabBar(ws) {
    const n = ws.zones.reduce((a, z) => a + z.tabs.length, 0);
    const rows = [`<div class="mb__head">${ws.name}<span class="meta">${n} tabs</span></div>`];
    for (const z of ws.zones) {
      rows.push(`<div class="mb__zone">${zoneGlyph(z.z)}${z.k}<span class="c">${z.tabs.length}</span><span class="add">${icon("plus", 11)}</span></div>`);
      if (!z.tabs.length) rows.push(`<div class="mb__empty">Drop a tab here</div>`);
      for (const t of z.tabs) {
        const cls = ["mb__win", t.show ? "is-show" : "", t.focus ? "is-focus" : ""].join(" ");
        rows.push(`<div class="${cls}">${icon(KIND[t.kind].ic, 13)}<span>${t.t}</span>${t.m || ""}</div>`);
      }
    }
    return rows.join("");
  }

  /* The desk: top row with the workspace bubble, the bar, the zones that hold tabs. */
  function tabDesk(ws, o = {}) {
    const W = 2560, H = 1080, g = 10, bw = 240, x0 = g + bw + g;
    const tiled = ws.zones.filter((z) => z.z !== "float" && z.tabs.length);
    const zw = (W - x0 - g - (tiled.length - 1) * g) / tiled.length;
    const wins = tiled.map((z, i) => {
      const x = Math.round(x0 + i * (zw + g)), x2 = Math.round(x0 + i * (zw + g) + zw);
      const t = z.tabs.find((t) => t.show);
      return win(appOf(t), { x, y: 48, w: x2 - x, h: H - 48 - g, zone: z.z, focus: t.focus, tb: { stack: z.tabs.length > 1 ? z.tabs.length : 0 } });
    });
    const fl = ws.zones.find((z) => z.z === "float");
    const ft = fl && fl.tabs.find((t) => t.show);
    if (ft) wins.push(win(appOf(ft), { x: 1560, y: 600, w: 520, h: 380, float: true, focus: ft.focus }));
    const spaces = ws.idx === 0 ? { current: 0, marks: { 1: wsMark.unread() } } : { current: 1, marks: { 0: wsMark.waiting() } };
    const lv = ws.idx === 0 ? live.waiting("menubar-pass") : live.waitingOn("Sola", "menubar-pass");
    return `<div class="desk" style="width:${W}px;height:${H}px">
      ${topRow("spaces", { spaces, live: lv })}
      <div class="mb" style="left:${g}px;top:48px;width:${bw}px;bottom:${g}px">${tabBar(ws)}</div>
      ${wins.join("")}
    </div>`;
  }

  /* ── Page 1 ── */
  set("desk", view(tabDesk(SOLA), { x: 0, y: 0, w: 1296, h: 547, s: S }));
  set("anat", view(tabDesk(SOLA), { x: 0, y: 0, w: 700, h: 700 }));

  const PAIRS = [
    ["ext-background-effect-v1", "<b>A browser tab is showing.</b> Center's titlebar is the browser's: its name, Edit, Profiles, Window, and the page title. The page has the zone's full width; its location bar stays, because it belongs to the page."],
    ["~/Workspace/Sola", "<b>Pick a terminal tab in the same zone.</b> The zone swaps what it shows and the titlebar becomes Terminal's. The browser tab stays open behind it; its row drops back to tertiary ink."],
  ];
  set("pairs", PAIRS.map(([title, cap]) => {
    const d = tabDesk(pick(SOLA, title));
    return `<div class="pair">${view(d, { x: 0, y: 40, w: 260, h: 400 })}${view(d, { x: 1017, y: 40, w: 776, h: 400 })}</div><p class="pair__cap">${cap}</p>`;
  }).join(""));

  /* ── Page 2 ── */
  set("mail", view(tabDesk(MAIL), { x: 0, y: 0, w: 1296, h: 547, s: S }));

  const BEFORE = [
    ["Browser, as it is", "The vertical tab strip, groups and all, inside the window.", win("browser", { x: 10, y: 10, w: 614, h: 600, focus: true, zone: 1 })],
    ["Browser, as a tab", "Location bar and page. The tabs are rows in the bar now.", win(appOf({ kind: "browser", t: "ext-background-effect-v1" }), { x: 10, y: 10, w: 614, h: 600, focus: true, zone: 1, tb: { stack: 4 } })],
    ["Projects, as it is", "The rail of projects and worktree tabs, then the agent's pane.", win("projects", { x: 10, y: 10, w: 614, h: 600, focus: true, zone: 0 })],
    ["Projects, as a tab", "Just the agent's pane. Each agent session is its own tab, with its mark on its row.", win(appOf({ kind: "agent", t: "kit-foundation · claude" }), { x: 10, y: 10, w: 614, h: 600, focus: true, zone: 0, tb: { stack: 3 } })],
  ];
  set("before", BEFORE.map(([h, p, w]) =>
    `<div class="sample">${view(`<div class="desk" style="width:860px;height:620px">${w}</div>`, { x: 0, y: 0, w: 634, h: 330 })}<div class="sample__head"><h4>${h}</h4></div><p>${p}</p></div>`).join(""));

  finish();
}
