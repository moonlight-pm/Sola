/* Next: shared parts. Boards call these to draw the islands, windows,
 * titlebars, and app bodies, so every board shows the same desk. */
const FLOWER_D = "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.300-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.600-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.900-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";

const ICONS = {
  mail: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  calendar: '<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  chevL: '<path d="m15 18-6-6 6-6"/>',
  chevR: '<path d="m9 18 6-6-6-6"/>',
  chevD: '<path d="m6 9 6 6 6-6"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  lock: '<rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.7 9a1 1 0 0 1-.6 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.2-2.7a1.2 1.2 0 0 1 1.6 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z"/>',
  download: '<path d="M12 15V3M7 10l5 5 5-5M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20"/>',
  terminal: '<path d="m7 11 2-2-2-2M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2"/>',
  image: '<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>',
  bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2M20 14h2M15 13v2M9 13v2"/>',
  activity: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
  layers: '<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 12.5-8.58 3.91a2 2 0 0 1-1.66 0L2 12.5"/>',
  plus: '<path d="M5 12h14M12 5v14"/>',
  calculator: '<rect width="16" height="20" x="4" y="2" rx="2"/><path d="M8 6h8M16 10h.01M12 10h.01M8 10h.01M12 14h.01M8 14h.01M12 18h.01M8 18h.01M16 14v4"/>',
  gamepad: '<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258A4 4 0 0 0 17.32 5z"/>',
  capture: '<path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2"/><rect x="7" y="8" width="10" height="8" rx="1"/>',
  palette: '<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
};

function icon(name, size = 14, color = "currentColor", sw = 1.75) {
  return `<svg class="ic" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
}

function flower(size = 14) {
  return `<svg class="flower" width="${size}" height="${size}" viewBox="-25 -32 561 561"><path d="${FLOWER_D}" fill="var(--sol)"/></svg>`;
}

/* Where the window sits: three columns with its own filled, or a float. */
function zoneGlyph(i, n = 3) {
  if (i === "float")
    return `<svg class="tb__zone" width="16" height="12" viewBox="0 0 16 12"><rect x="0.5" y="0.5" width="15" height="11" rx="2" fill="none" stroke="currentColor" stroke-opacity=".5"/><rect x="4" y="3" width="8" height="6" rx="1.2" fill="currentColor"/></svg>`;
  const pitch = 15.75 / n, cw = pitch - 0.75;
  const cols = [...Array(n)].map((_, c) => {
    const x = 0.5 + c * pitch;
    return c === i
      ? `<rect x="${x}" y="0.5" width="${cw}" height="11" rx="1.2" fill="currentColor"/>`
      : `<rect x="${x + 0.5}" y="1" width="${cw - 1}" height="10" rx="1" fill="none" stroke="currentColor" stroke-opacity=".5"/>`;
  });
  return `<svg class="tb__zone" width="16" height="12" viewBox="0 0 16 12">${cols.join("")}</svg>`;
}

/* Shape first: working is a ring, waiting a diamond, done a check. */
const mark = {
  ring: (size = 12, color = "var(--accent)") =>
    `<svg class="mark" width="${size}" height="${size}" viewBox="0 0 12 12"><path d="M6 1.6A4.4 4.4 0 1 1 1.6 6" fill="none" stroke="${color}" stroke-width="1.7" stroke-linecap="round"/></svg>`,
  diamond: (size = 12, color = "var(--warning)") =>
    `<svg class="mark" width="${size}" height="${size}" viewBox="0 0 12 12"><rect x="3" y="3" width="6" height="6" rx="1" transform="rotate(45 6 6)" fill="${color}"/></svg>`,
  check: (size = 12, color = "var(--success)") =>
    `<svg class="mark" width="${size}" height="${size}" viewBox="0 0 12 12"><path d="M2.6 6.3 5 8.6 9.6 3.6" fill="none" stroke="${color}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  dot: (size = 12, color = "var(--ink-3)") =>
    `<svg class="mark" width="${size}" height="${size}" viewBox="0 0 12 12"><circle cx="6" cy="6" r="2" fill="${color}"/></svg>`,
};

/* ── LED meters (2 px dots on a 3 px pitch, painted at device pixels) ── */
const LED = {
  cpu: { vals: [1, 1, 2, 1, 3, 2, 1, 1, 2, 4, 2, 2], color: "#e9ecf2" },
  gpu: { vals: [0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1], color: "#e9ecf2" },
  mem: { vals: [3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4], color: "#e9ecf2", warn: 4 },
  rx: { vals: [0, 1, 0, 2, 1, 0, 1, 3, 1, 0, 2, 1], color: "#3dd6f5" },
  tx: { vals: [0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0], color: "#3ecf8e" },
};
const SPECTRUM = [3, 4, 5, 4, 3, 4, 2, 3, 2, 2, 1, 1];
const PHOSPHOR = ["#38fa6b", "#1febe0", "#6185ff", "#c752ff", "#ff942e"];

function leds(kind) {
  return `<canvas class="leds" data-kind="${kind}"></canvas>`;
}

function paintLeds(root = document) {
  const dpr = window.devicePixelRatio || 1;
  root.querySelectorAll("canvas.leds").forEach((cv) => {
    const kind = cv.dataset.kind;
    const css = getComputedStyle(cv);
    const ink = css.getPropertyValue("--ink").trim() || "#e9ecf2";
    const off = css.getPropertyValue("--led-off").trim() || "#e9ecf21f";
    const dot = 2, pitch = 3, rows = 5;
    const spectrum = kind === "spectrum";
    const cols = spectrum ? 36 : 12;
    const bandGap = spectrum ? 2 : 0;
    const w = spectrum ? 12 * (3 * pitch) + 11 * bandGap - (pitch - dot) : cols * pitch - (pitch - dot);
    const h = rows * pitch - (pitch - dot);
    cv.style.width = w + "px";
    cv.style.height = h + "px";
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    const g = cv.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    for (let c = 0; c < cols; c++) {
      const band = spectrum ? Math.floor(c / 3) : c;
      const x = spectrum ? band * (3 * pitch + bandGap) + (c % 3) * pitch : c * pitch;
      const v = spectrum ? SPECTRUM[band] : LED[kind].vals[c];
      for (let r = 0; r < rows; r++) {
        let col = off;
        if (r < v) {
          if (spectrum) col = PHOSPHOR[r];
          else if (LED[kind].warn && v >= LED[kind].warn) col = "#e8b84a";
          else col = LED[kind].color === "#e9ecf2" ? ink : LED[kind].color;
        }
        g.fillStyle = col;
        g.fillRect(x, h - dot - r * pitch, dot, dot);
      }
    }
  });
}

/* ── Islands: the top row ───────────────────────────────────── */
const live = {
  working: (name = "kit-foundation", t = "4m") =>
    `${mark.ring()}<span class="live__name">${name}</span><span class="live__verb">working</span><span class="live__num">${t}</span>`,
  waiting: (name = "kit-foundation") =>
    `${mark.diamond()}<span class="live__name">${name}</span><span class="live__verb">wants to run</span><code class="live__code">cargo make build</code>`,
  waitingOn: (ws = "Sola", name = "kit-foundation") =>
    `${mark.diamond()}<span class="live__name">${name}</span><span class="live__verb">wants to run</span><code class="live__code">cargo make build</code><span class="live__ws">on ${ws}</span>`,
  playing: () =>
    `${icon("music", 13, "var(--ink-2)")}<span class="live__name">Says</span><span class="live__verb">Nils Frahm</span><span class="live__num">3:12</span>`,
  countdown: () =>
    `${icon("calendar", 13, "var(--ink-2)")}<span class="live__name">Design review</span><span class="live__verb">in</span><span class="live__num">25 min</span>`,
};

/* Workspaces in the OS sense: each one swaps the whole set of windows. */
const SPACES = [
  { n: 1, name: "Sola" },
  { n: 2, name: "Mail" },
  { n: 3, name: "Game" },
];
const wsMark = {
  waiting: () => mark.diamond(10),
  unread: () => `<span class="sp__dot"></span>`,
};

const B = {
  /* style: "marks" (names + attention, the proposal) · "names" · "numbers" · "dots".
   * current: index of the showing workspace. marks: {index: html}. */
  spaces: (o = {}) => {
    const cur = o.current ?? 0, style = o.style || "marks", marks = o.marks || {};
    const items = SPACES.map((s, i) => {
      const on = i === cur ? " is-on" : "";
      if (style === "numbers") return `<span class="sp sp--num${on}">${s.n}${marks[i] ? `<i class="sp__badge">${marks[i]}</i>` : ""}</span>`;
      if (style === "dots") return `<span class="sp sp--dot${on}"></span>`;
      const m = style === "marks" && marks[i] ? marks[i] : "";
      return `<span class="sp${on}">${style === "marks" ? `<span class="sp__n">${s.n}</span>` : ""}${s.name}${m}</span>`;
    });
    return `<div class="bubble bubble--ws${o.open ? " is-open" : ""}">${items.join("")}</div>`;
  },
  flower: (open) => `<div class="bubble bubble--icon${open ? " is-open" : ""}">${flower(14)}</div>`,
  search: () => `<div class="bubble">${flower(14)}<span class="dim">Search</span><span class="key" style="margin-left:18px">Super Space</span></div>`,
  agents: () => `<div class="bubble">
      <span class="agent">${mark.ring()}<span>kit-foundation</span></span>
      <span class="agent">${mark.diamond()}<span>menubar-pass</span></span>
      <span class="agent">${mark.check()}<span class="dim">mail-focus</span></span>
    </div>`,
  live: (html = live.working()) => `<div class="bubble">${html}</div>`,
  extras: () => `<div class="bubble" style="gap:12px">${icon("mail", 14, "var(--accent)")}${icon("bell")}${icon("bluetooth")}</div>`,
  spectrum: (open) => `<div class="bubble${open ? " is-open" : ""}">${leds("spectrum")}</div>`,
  stats: () => `<div class="bubble" style="gap:12px"><span class="stat">CPU ${leds("cpu")}</span><span class="stat">GPU ${leds("gpu")}</span><span class="stat">MEM ${leds("mem")}</span></div>`,
  net: () => `<div class="bubble" style="gap:12px"><span class="stat">RX ${leds("rx")}</span><span class="stat">TX ${leds("tx")}</span></div>`,
  clock: () => `<div class="bubble"><span class="clock">14:32 Wed 2026-10-14</span></div>`,
  /* Everything on the right in one island (the Islands canvas) */
  instruments: () => `<div class="bubble" style="gap:16px">
      <span style="display:flex;gap:12px">${icon("mail", 14, "var(--accent)")}${icon("bell")}${icon("bluetooth")}</span>
      ${leds("spectrum")}
      <span style="display:flex;gap:12px"><span class="stat">CPU ${leds("cpu")}</span><span class="stat">GPU ${leds("gpu")}</span><span class="stat">MEM ${leds("mem")}</span></span>
      <span style="display:flex;gap:12px"><span class="stat">RX ${leds("rx")}</span><span class="stat">TX ${leds("tx")}</span></span>
      <span class="clock">14:32 Wed 2026-10-14</span>
    </div>`,
};

/* Arrangements of the top row. "bubbles" is the proposal. */
const TOPS = {
  three: (o) => ({ l: [B.flower()], c: [B.live(o.live)], r: [B.instruments()] }),
  bubbles: (o) => ({ l: [B.flower()], c: [B.live(o.live)], r: [B.extras(), B.spectrum(), B.stats(), B.net(), B.clock()] }),
  agents: (o) => ({ l: [B.flower(), B.agents()], c: [B.live(o.live || live.playing())], r: [B.extras(), B.spectrum(), B.stats(), B.net(), B.clock()] }),
  search: (o) => ({ l: [B.search()], c: [B.live(o.live)], r: [B.extras(), B.spectrum(), B.stats(), B.net(), B.clock()] }),
  hero: (o) => ({ l: [B.flower(), B.agents()], c: [], r: [B.extras(), B.clock()] }),
  corner: () => ({ l: [B.flower()], c: [], r: [] }),
  spaces: (o) => ({ l: [B.flower(), B.spaces(o.spaces)], c: o.live === null ? [] : [B.live(o.live)], r: [B.extras(), B.spectrum(), B.stats(), B.net(), B.clock()] }),
  spacesHero: (o) => ({ l: [B.flower(), B.spaces(o.spaces)], c: o.live === null ? [] : [B.live(o.live)], r: [B.extras(), B.clock()] }),
  none: () => ({ l: [], c: [], r: [] }),
};

function topRow(kind = "bubbles", o = {}) {
  const t = (TOPS[kind] || TOPS.bubbles)(o);
  return `<div class="top">
    <div class="top__l">${t.l.join("")}</div>
    ${t.c.length ? `<div class="top__c">${t.c.join("")}</div>` : ""}
    <div class="top__r">${t.r.join("")}</div>
  </div>`;
}

/* ── Apps: the menus each one publishes today, plus the Window menu
 * the shell injects. The first menu is the app's own name. ── */
const APPS = {
  workspaces: { name: "Workspaces", menus: ["Project", "Edit", "Window"], title: "kit-foundation · claude", body: () => workspacesBody() },
  browser: { name: "Browser", menus: ["Edit", "Profiles", "Window"], title: "ext-background-effect-v1", body: () => browserBody() },
  mail: { name: "Mail", menus: ["Edit", "Mailbox", "Message", "View", "Window"], title: "Inbox · 12 unread", body: () => mailBody() },
  preview: { name: "Preview", menus: ["Window"], title: "invoice-0412.png", body: () => previewBody() },
  projects: { name: "Projects", menus: ["Project", "Edit", "Window"], title: "kit-foundation · claude", body: () => workspacesBody() },
  calendar: { name: "Calendar", menus: ["Edit", "View", "Window"], title: "October 2026", body: () => calendarBody() },
  arcade: { name: "Arcade", menus: ["Window"], title: "Hollow Lantern", body: () => arcadeBody() },
  terminal: { name: "Terminal", menus: ["Edit", "View", "Window"], title: "~/Workspace/Sola", body: () => `<div class="term">${termLines().slice(0, 8).join("\n")}</div>` },
};

/* The app menu items the sheets show (Workspaces' and Mail's, as published). */
const SHEETS = {
  Workspaces: [["Spawn Sibling", "⌘T"], ["Split Down", "⇧⌘↓"], ["Split Right", "⇧⌘→"], ["Close Pane", "⌘W"], "-", ["About Workspaces"], "-", ["Quit Workspaces", "⌘Q"]],
  Message: [["New Message", "⌘N"], ["Attach Files…", "⇧⌘A"], "-", ["Reply", "⌘R"], ["Reply All", "⇧⌘R", true], "-", ["Archive", "A"], ["Move to Inbox", "I"], ["Move to Junk", "X"], ["Delete", "D"], "-", ["Undo Move", "U"]],
  Window: [["Hide", "⌘H"], ["Cycle Windows", "⌘`"], "-", ["Float", "⌘ Pad *"], ["Left", "⌘ Pad 4"], ["Full Middle", "⌘ Pad 5", true], ["Right", "⌘ Pad 6"], "-", ["Fullscreen"]],
};

function sheet(name, o = {}) {
  const rows = (SHEETS[name] || []).map((r) =>
    r === "-" ? `<div class="sep"></div>` : `<div class="row${r[2] ? " is-hot" : ""}">${r[0]}${r[1] ? `<span class="row__key">${r[1]}</span>` : ""}</div>`
  );
  return `<div class="sheet" data-under="${name}" style="top:${o.top ?? 42}px;${o.style || ""}">${rows.join("")}</div>`;
}

/* A titlebar.
 *   layout: "row" (menus in the titlebar, the proposal) · "two" (title row,
 *   then a menu row) · "name" (menus fold into the app name) · "title"
 *   (close and title only) · "ssd" (drawn by sola-river for foreign apps)
 *   fold: how many menus fold into "···" (from the right)
 *   open: which menu (or the app name) is open */
function titlebar(app, o = {}) {
  const a = typeof app === "string" ? APPS[app] : app;
  const layout = o.layout || "row";
  const name = o.name || a.name;
  const title = o.title ?? a.title;
  const isOpen = (m) => (o.open === m ? " is-open" : "");
  const zone = (o.stack ? `<span class="tb__stack">${icon("layers", 12)}${o.stack}</span>` : "") +
    (o.zone === undefined || o.zone === null ? "" : zoneGlyph(o.zone, o.zones || 3));
  const close = `<span class="tb__close"></span>`;

  if (layout === "two") {
    return `<div class="tb">${close}<span class="tb__title tb__title--center">${title}</span><span style="margin-left:auto">${zone}</span></div>
      <div class="mrow"><span class="tb__app${isOpen(name)}">${name}</span>${a.menus.map((m) => `<span class="tb__menu${isOpen(m)}">${m}</span>`).join("")}</div>`;
  }
  if (layout === "name") {
    return `<div class="tb">${close}<span class="tb__app${isOpen(name)}">${name}${icon("chevD", 12, "var(--ink-3)", 2)}</span><span class="tb__title">${title}</span>${zone}</div>`;
  }
  if (layout === "title") {
    return `<div class="tb">${close}<span class="tb__title" style="margin-left:0;padding-left:2px;color:var(--ink-2)">${title}</span>${zone}</div>`;
  }
  if (layout === "ssd") {
    return `<div class="tb tb--ssd">${close}<span class="tb__app">${name}</span><span class="ssd-tag">${o.tag || "XWAYLAND"}</span><span class="tb__title">${title}</span>${zone}</div>`;
  }
  if (layout === "omni") {
    return `<div class="tb">${close}<span class="tb__app${isOpen(name)}">${name}${icon("chevD", 12, "var(--ink-3)", 2)}</span>
      <span class="tb__tools" style="margin-left:8px">${icon("chevL", 16)}${icon("chevR", 16)}${icon("refresh", 14)}</span>
      <span class="tb__omni">${icon("lock", 12)}<span>wayland.app/<b>protocols/ext-background-effect-v1</b></span></span>
      <span class="tb__tools">${icon("shield", 14)}${icon("download", 14)}</span>${zone}</div>`;
  }
  const fold = o.fold || 0;
  const shown = a.menus.slice(0, a.menus.length - fold);
  return `<div class="tb${o.hideMenus ? " tb--hide-menus" : ""}">${close}<span class="tb__app${isOpen(name)}">${name}</span>${shown
    .map((m) => `<span class="tb__menu${isOpen(m)}">${m}</span>`)
    .join("")}${fold ? `<span class="tb__more">···</span>` : ""}<span class="tb__title">${title}</span>${zone}</div>`;
}

/* A window: titlebar on top, the app below. */
function win(app, o = {}) {
  const a = typeof app === "string" ? APPS[app] : app;
  const cls = ["win", o.focus ? "is-focus" : "", o.float ? "win--float" : "", o.cls || ""].join(" ");
  const tb = o.tb === false ? "" : titlebar(a, { zone: o.float ? "float" : o.zone, ...(o.tb || {}) });
  const body = o.body || a.body();
  return `<div class="${cls}" style="left:${o.x}px;top:${o.y}px;width:${o.w}px;height:${o.h}px;${o.style || ""}">
    ${tb}<div class="win__body">${body}</div>${o.sheet ? sheet(o.sheet, o.sheetOpts) : ""}
  </div>`;
}

/* Columns of tiles under the top row, every gap the same. */
function tiles(W, H, n, g = 10) {
  const top = 2 * g + 28;
  const w = (W - 2 * g - (n - 1) * g) / n;
  return [...Array(n)].map((_, i) => {
    const x = Math.round(g + i * (w + g));
    const x2 = Math.round(g + i * (w + g) + w);
    return { x, y: top, w: x2 - x, h: H - top - g };
  });
}

/* The desk: top row, three tiles (Workspaces, Browser, Mail), an optional
 * float. Options: w, h, gap, radius, top (TOPS key), focus (tile index),
 * float ({x,y,w,h} or false), cls, tb (titlebar options for all tiles),
 * open ({i, menu}) to hang one menu. */
function desk(o = {}) {
  const W = o.w || 2560, H = o.h || 1080, g = o.gap ?? 10;
  const apps = o.apps || ["workspaces", "browser", "mail"];
  const r = tiles(W, H, apps.length, g);
  const focus = o.focus ?? 0;
  const wins = apps.map((app, i) => {
    const open = o.open && o.open.i === i ? o.open.menu : undefined;
    return win(app, {
      ...r[i],
      zone: i,
      focus: focus === i,
      tb: { zones: apps.length, ...(o.tb || {}), ...(o.tbs?.[i] || {}), open },
      sheet: open,
      sheetOpts: { top: 42 },
    });
  });
  const fl = o.float === false ? "" : win("preview", { ...(o.float || { x: 1500, y: 560, w: 560, h: 400 }), float: true, focus: focus === "float" });
  const vars = [`--gap:${g}px`, o.radius !== undefined ? `--r-window:${o.radius}px` : "", o.style || ""].join(";");
  return `<div class="desk ${o.cls || ""}" style="width:${W}px;height:${H}px;${vars}">
    ${topRow(o.top || "bubbles", o)}
    ${wins.join("")}
    ${fl}
  </div>`;
}

/* ── App bodies ─────────────────────────────────────────────── */
function rail() {
  const row = (m, name, extra = "", sel = false) =>
    `<div class="rail__row${sel ? " is-sel" : ""}">${m}<span>${name}</span>${extra}</div>`;
  return `<div class="rail">
    <div class="rail__proj"><span>SOLA</span><span>+</span></div>
    ${row(mark.ring(), "kit-foundation", '<span class="count">×2</span>', true)}
    ${row(mark.diamond(), "menubar-pass")}
    ${row(mark.check(), "mail-focus")}
    ${row(mark.dot(), "master")}
    <div class="rail__proj" style="margin-top:10px"><span>WICKET</span><span>+</span></div>
    ${row(mark.diamond(), "vault-panel")}
    ${row(mark.dot(), "master")}
    <div class="rail__proj" style="margin-top:10px"><span>▸ EXIT</span><span>+</span></div>
    <div class="rail__proj"><span>▸ GENEOLOGY</span><span>+</span></div>
  </div>`;
}

function termLines() {
  return [
    `<span class="c">~/Workspace/Sola/.worktrees/kit-foundation</span>`,
    `❯ claude`,
    ``,
    `● Read crates/sola-kit/src/components/titlebar.rs`,
    `● Read crates/sola-kit/src/menu.rs`,
    `  <span class="m">The menu titles move into the titlebar row;</span>`,
    `  <span class="m">the shell keeps drawing the sheet under them.</span>`,
    ``,
    `● Update(crates/sola-kit/src/components/titlebar.rs)`,
    `  <span class="r">-  pub fn titlebar(title, on_drag, on_close)</span>`,
    `  <span class="g">+  pub fn titlebar(title, menus, on_drag, on_close)</span>`,
    ``,
    `  Build it?  cargo make build sola-kit`,
    `  <span class="c">❯ 1. Yes</span>`,
    `    <span class="m">2. No, and tell Claude what to do</span>`,
    ``,
    `❯ <span class="sel">cargo make build</span> sola-kit <span class="cur"></span>`,
  ];
}

function workspacesBody() {
  return `<div class="ws">${rail()}<div class="term">${termLines().join("\n")}</div></div>`;
}

function browserBody(o = {}) {
  const tab = (l, c, t, on) => `<div class="br__tab${on ? " is-on" : ""}"><span class="fav" style="background:${c}">${l}</span><span>${t}</span></div>`;
  const bar = o.noBar ? "" : `<div class="br__bar">${icon("chevL", 16)}${icon("chevR", 16)}${icon("refresh", 14)}<div class="omni">wayland.app/<b>protocols/ext-background-effect-v1</b></div>${icon("shield", 14)}${icon("download", 14)}</div>`;
  const side = o.noSide ? "" : `<div class="br__side">
      <div class="br__grp">
        <div class="br__grp-h">${icon("folder", 12)}WAYLAND</div>
        ${tab("W", "#e8b84a", "ext-background-effect-v1", true)}
        ${tab("W", "#e8b84a", "river-window-management-v1")}
        ${tab("W", "#e8b84a", "xdg-decoration-unstable-v1")}
      </div>
      ${tab("L", "#f07178", "niri 26.04 — LWN.net")}
      ${tab("D", "#3dd6f5", "iced 0.14 — docs.rs")}
      ${tab("G", "#3ecf8e", "sola · GitHub")}
      ${tab("O", "#7a8ac7", "Omarchy manual")}
    </div>`;
  return `<div class="br">${side}
    <div class="br__main">${bar}
      <div class="page">
        <h1>Background effect</h1>
        <div class="sub">ext-background-effect-v1 · staging</div>
        <div class="bars"><i style="width:96%"></i><i style="width:88%"></i><i style="width:92%"></i><i style="width:54%"></i></div>
        <h2>Interfaces</h2>
        <div class="code">ext_background_effect_manager_v1
ext_background_effect_surface_v1</div>
        <h2>Compositor support</h2>
        <div class="bars"><i style="width:72%"></i><i style="width:84%"></i><i style="width:40%"></i></div>
      </div>
    </div>
  </div>`;
}

function mailBody() {
  const msg = (from, time, subj, prev, unread, sel) => `<div class="msg${sel ? " is-sel" : ""}">
    <div class="msg__top">${unread ? '<span class="msg__dot"></span>' : ""}${from}<time>${time}</time></div>
    <div>${subj}</div><div class="msg__prev">${prev}</div></div>`;
  return `<div class="mail">
    <div class="mail__list">
      <div class="mail__head">Inbox <span>${mark.ring(11)} 12 unread</span></div>
      ${msg("Hetzner Online", "14:02", "Invoice 0412 for October", "Your invoice is attached as a PDF.", true)}
      ${msg("Cal.com", "13:40", "Design review moved to Thursday", "The Sola foundation review is now…", true, true)}
      ${msg("GitHub", "12:15", "[sola] CI passed on master", "All 4 jobs succeeded for 69d74ca.")}
      ${msg("Fastmail", "Yesterday", "Your export is ready", "Download it within 7 days.")}
      ${msg("Grapevine", "Mon", "Weekly digest", "Three new threads in #desk.")}
    </div>
    <div class="mail__read">
      <h3>Design review moved to Thursday</h3>
      <div class="from">Cal.com · notifications@cal.com · 13:40</div>
      <div class="acts"><span class="btn btn--quiet">Reply</span><span class="btn">Forward</span><span class="btn">Junk</span></div>
      <p>Hi —</p>
      <p>The Sola foundation review moved to Thursday at 10:00. Same room, same link.</p>
      <p>On the agenda: menus on the window, the gaps, and what the top row carries now.</p>
      <blockquote>Can the islands line up with the window edges? Then the gap is the grid.</blockquote>
    </div>
  </div>`;
}

function calendarBody() {
  // October 2026, Sunday first: Sep 27–30, then 1–31 (Oct 1 is a Thursday).
  const ev = {
    9: [["#7a8ac7", "Dentist 15:30"]],
    14: [["#3dd6f5", "Kit overhaul sketch"]],
    15: [["#e8b84a", "10:00 Design review"]],
    20: [["#f07178", "Hetzner invoice due"]],
    23: [["#75a380", "Hike, Tiger Mtn"]],
    28: [["#7a8ac7", "1:1 · 11:00"]],
  };
  const cells = [];
  for (let d = 27; d <= 30; d++) cells.push(`<div class="cal__c is-out"><span class="cal__d">${d}</span></div>`);
  for (let d = 1; d <= 31; d++) {
    const today = d === 14;
    cells.push(`<div class="cal__c${today ? " is-today" : ""}"><span class="cal__d">${d}</span>${(ev[d] || [])
      .map(([c, t]) => `<span class="cal__ev" style="--ev:${c}">${t}</span>`).join("")}</div>`);
  }
  return `<div class="cal">
    <div class="cal__bar"><b>October 2026</b><span class="cal__nav">${icon("chevL", 16)}${icon("chevR", 16)}</span><span class="btn btn--quiet">Today</span></div>
    <div class="cal__wd">${["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].map((w) => `<span>${w}</span>`).join("")}</div>
    <div class="cal__grid">${cells.join("")}</div>
  </div>`;
}

function arcadeBody() {
  return `<div class="game"><div class="game__sky"></div><div class="game__hills"></div><div class="game__title">HOLLOW LANTERN</div><div class="game__hud">gamescope · 2560 × 1080</div></div>`;
}

function previewBody() {
  return `<div class="pv"><div class="paper"><i class="k" style="width:45%"></i><i style="width:92%"></i><i style="width:80%"></i><i style="width:88%"></i><i style="width:60%"></i><i style="width:84%"></i><i class="k" style="width:30%;margin-top:10px"></i><i style="width:70%"></i></div></div>`;
}

/* Place each hanging sheet under its open title (needs layout, so after fonts). */
function hangSheets(root = document) {
  root.querySelectorAll(".win").forEach((w) => {
    const s = w.querySelector(".sheet[data-under]");
    const p = w.querySelector(".tb .is-open, .mrow .is-open");
    if (!s || !p) return;
    s.style.left = p.offsetLeft - 7 + "px";
    if (p.closest(".mrow")) s.style.top = p.closest(".mrow").offsetTop + 28 + "px";
  });
}

function finish(root = document) {
  paintLeds(root);
  hangSheets(root);
  document.fonts.ready.then(() => hangSheets(root));
}

/* Today, as built: one 28 px bar with the focused app's menus, tiles
 * flush below it with no titles. For before / after only. */
function todayDesk(o = {}) {
  const W = o.w || 2560, H = o.h || 1080, top = 28;
  const apps = ["workspaces", "browser", "mail"];
  const cut = [0, Math.round(W / 3), Math.round((2 * W) / 3), W];
  const wins = apps.map((app, i) =>
    win(app, { x: cut[i], y: top, w: cut[i + 1] - cut[i], h: H - top, tb: false, cls: "win--flush" }));
  const a = APPS.workspaces;
  return `<div class="desk" style="width:${W}px;height:${H}px">
    <div class="gbar">
      <span class="gbar__l">${flower(14)}<b>${a.name}</b>${a.menus.map((m) => `<span>${m}</span>`).join("")}</span>
      <span class="gbar__r">${B.instruments().replace('class="bubble"', 'class="bubble bubble--bare"')}</span>
    </div>
    ${wins.join("")}
  </div>`;
}
