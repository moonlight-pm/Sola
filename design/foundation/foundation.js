/* Foundation: the parts every board draws, as built on `next`
 * (2026-10-24). Plain globals; a board injects HTML, then calls finish().
 * Sizes are the code's: islands 28 (menubar/mod.rs), titlebar 38 and
 * corners 12 (kit window_frame), the tab bar 240 (shell zoning.rs). */

const FLOWER_D = "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.300-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.600-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.900-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";

const ICONS = {
  mail: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  mouse: '<rect x="5" y="2" width="14" height="20" rx="7"/><path d="M12 6v4"/>',
  calendar: '<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  chevL: '<path d="m15 18-6-6 6-6"/>',
  chevR: '<path d="m9 18 6-6-6-6"/>',
  chevD: '<path d="m6 9 6 6 6-6"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.7 9a1 1 0 0 1-.6 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.2-2.7a1.2 1.2 0 0 1 1.6 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z"/>',
  download: '<path d="M12 15V3M7 10l5 5 5-5M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  plus: '<path d="M5 12h14M12 5v14"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20"/>',
  terminal: '<path d="m7 11 2-2-2-2M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2"/>',
  prompt: '<path d="m4 17 6-6-6-6M12 19h8"/>',
  image: '<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>',
  settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  cube: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5M12 22V12"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  palette: '<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/>',
  calculator: '<rect width="16" height="20" x="4" y="2" rx="2"/><path d="M8 6h8M16 10h.01M12 10h.01M8 10h.01M12 14h.01M8 14h.01M12 18h.01M8 18h.01M16 14v4"/>',
  gamepad: '<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258A4 4 0 0 0 17.32 5z"/>',
  capture: '<path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2"/><rect x="7" y="8" width="10" height="8" rx="1"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
};

function icon(name, size = 14, color = "currentColor", sw = 1.75) {
  return `<svg class="ic" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
}

function flower(size = 14) {
  return `<svg class="flower" width="${size}" height="${size}" viewBox="-25 -32 561 561"><path d="${FLOWER_D}" fill="var(--sol)"/></svg>`;
}

/* Shape first (kit status_mark): working a spinning accent ring, waiting
 * a warning diamond, done a success check; idle a quiet dot. */
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

/* ── LED meters (stats/pixel.rs, audio/wave.rs) ─────────────────
 * 12 × 5 cells of 2 px dots on a 3 px pitch, 35 × 14. A column is one
 * sample, lit from the bottom, its top cell dithered (2×2 Bayer). Levels
 * (CPU, GPU, MEM) print the ink, then the warning past 75%, the danger past 90%,
 * per column; RX takes the accent, TX success. Unlit cells stay at 16%. */
const LED = {
  cpu: [12, 18, 30, 22, 46, 34, 20, 18, 41, 77, 52, 38],
  cpuHot: [55, 62, 71, 78, 84, 88, 92, 86, 79, 83, 91, 80],
  gpu: [3, 8, 2, 4, 12, 3, 2, 9, 4, 2, 11, 6],
  mem: [61, 61, 62, 62, 62, 63, 63, 63, 64, 64, 64, 64],
  rx: [5, 22, 8, 46, 20, 6, 18, 64, 25, 9, 40, 18],
  tx: [2, 4, 16, 3, 2, 22, 4, 18, 3, 2, 26, 6],
};
const SPECTRUM = [0.62, 0.8, 1, 0.82, 0.6, 0.78, 0.42, 0.58, 0.4, 0.38, 0.22, 0.2];
const PHOSPHOR = ["--phosphor-1", "--phosphor-2", "--phosphor-3", "--phosphor-4", "--phosphor-5"];
const BAYER = [[0.125, 0.625], [0.875, 0.375]];

function leds(kind, o = {}) {
  const data = o.vals ? ` data-vals="${o.vals.join(" ")}"` : "";
  const cols = o.cols ? ` data-cols="${o.cols}"` : "";
  const rows = o.rows ? ` data-rows="${o.rows}"` : "";
  const scale = o.scale ? ` data-scale="${o.scale}"` : "";
  return `<canvas class="leds" data-kind="${kind}"${data}${cols}${rows}${scale}></canvas>`;
}

function paintLeds(root = document) {
  const dpr = window.devicePixelRatio || 1;
  root.querySelectorAll("canvas.leds").forEach((cv) => {
    const css = getComputedStyle(cv);
    const v = (n) => css.getPropertyValue(n).trim();
    const kind = cv.dataset.kind;
    const k = Number(cv.dataset.scale || 1);
    const dot = 2 * k, pitch = 3 * k;
    const rows = Number(cv.dataset.rows || 5);
    const spectrum = kind === "spectrum";
    const vals = cv.dataset.vals ? cv.dataset.vals.split(" ").map(Number) : spectrum ? SPECTRUM : LED[kind];
    const cols = spectrum ? 36 : Number(cv.dataset.cols || vals.length);
    const band = 2 * k;
    const w = spectrum ? 12 * 3 * pitch + 11 * band - (pitch - dot) : cols * pitch - (pitch - dot);
    const h = rows * pitch - (pitch - dot);
    cv.style.width = w + "px";
    cv.style.height = h + "px";
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    const g = cv.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const off = v("--led-off");
    const level = (pct) => (pct >= 90 ? v("--danger") : pct >= 75 ? v("--warning") : v("--text-primary"));
    const tint = kind === "rx" ? v("--accent") : kind === "tx" ? v("--success") : null;
    const max = kind === "rx" || kind === "tx" ? Math.max(...vals, 1) : 100;
    for (let c = 0; c < cols; c++) {
      const b = spectrum ? Math.floor(c / 3) : c;
      const x = spectrum ? b * (3 * pitch + band) + (c % 3) * pitch : c * pitch;
      const sample = spectrum ? vals[b] : vals[c - (cols - vals.length)] ?? 0;
      const fill = spectrum ? sample : Math.min(1, sample / max);
      const cells = fill * rows;
      for (let r = 0; r < rows; r++) {
        const cf = Math.max(0, Math.min(1, cells - r));
        const lit = spectrum ? v(PHOSPHOR[Math.min(r, 4)]) : tint || level(sample);
        // 2×2 sub-pixels per cell, dithered like pixel.rs.
        for (let dy = 0; dy < 2; dy++)
          for (let dx = 0; dx < 2; dx++) {
            const on = cf >= 1 || (cf > 0 && cf > BAYER[dy][dx]);
            g.fillStyle = on ? lit : off;
            g.fillRect(x + dx * k, h - dot - r * pitch + dy * k, k, k);
          }
      }
    }
  });
}

/* ── The top row ────────────────────────────────────────────── */
const I = {
  flower: (open) => `<div class="isl isl--flower${open ? " is-open" : ""}">${flower(14)}</div>`,
  /* One per Projects session that asks or has finished: its mark, its
   * project in medium, its tab in parentheses, and what a waiting one
   * wants ("✓ Sola (root)", "◆ Sola (root) wants to run …"). */
  agent: (m, project, tab, want = "", open) =>
    `<div class="isl${open ? " is-open" : ""}" style="gap:6px">${m}<span class="agent__name">${project}</span>${tab ? `<span class="agent__title">(${tab})</span>` : ""}${want ? `<span class="agent__title">${want}</span>` : ""}</div>`,
  more: (n) => `<div class="isl isl--more">+${n}</div>`,
  extras: (o = {}) => {
    const parts = [];
    if (o.mail) parts.push(`<span class="count">${icon("mail", 14, "var(--accent)")}${o.mail}</span>`);
    if (o.bell) parts.push(`<span class="count"${o.seen ? ' style="color:var(--ink)"' : ""}>${icon("bell", 14)}${o.bell}</span>`);
    if (o.mouse) parts.push(`<span class="stat" style="font-size:12px">${icon("mouse", 13)}${o.mouse}</span>`);
    if (o.bt !== false) parts.push(icon("bluetooth", 14, o.bt === "off" ? "var(--stat-label)" : "currentColor"));
    return `<div class="isl${o.open ? " is-open" : ""}">${parts.join("")}</div>`;
  },
  spectrum: (open) => `<div class="isl isl--spectrum${open ? " is-open" : ""}">${leds("spectrum")}</div>`,
  meters: (o = {}) =>
    `<div class="isl${o.open ? " is-open" : ""}"><span class="stat">CPU ${leds(o.hot ? "cpuHot" : "cpu")}</span>${o.gpu === false ? "" : `<span class="stat">GPU ${leds("gpu")}</span>`}<span class="stat">MEM ${leds("mem")}</span></div>`,
  net: (open) => `<div class="isl${open ? " is-open" : ""}"><span class="stat">RX ${leds("rx")}</span><span class="stat">TX ${leds("tx")}</span></div>`,
  clock: (t = "14:32 Sat 2026-10-24", open) => `<div class="isl${open ? " is-open" : ""}"><span class="clock">${t}</span></div>`,
  toast: (t) => `<div class="isl">${t}</div>`,
};

/* The row: { l: [...], c: html, r: [...] }. Defaults to canto's. */
function topRow(o = {}) {
  const l = o.l || [I.flower()];
  const r = o.r || [I.extras({ mail: 12 }), I.spectrum(), I.meters({ gpu: o.gpu }), I.net(), I.clock(o.clock)];
  return `<div class="top"><div class="top__l">${l.join("")}</div>${o.c ? `<div class="top__c">${o.c}</div>` : ""}<div class="top__r">${r.join("")}</div></div>`;
}

/* ── Apps: the menus each publishes (first is its name), the Window
 * menu the kit appends, and a title. ── */
const APPS = {
  projects: { menus: ["Projects", "Project", "Edit", "Window"], title: "Sola", ic: "prompt", body: () => projectsBody() },
  terminal: { menus: ["Terminal", "Shell", "Pane", "Edit", "Window"], title: "home", ic: "terminal", body: () => `<div class="term">${termHome()}</div>` },
  browser: { menus: ["Browser", "Edit", "Profiles", "Window"], title: "ext-background-effect-v1", ic: "globe", body: () => browserBody() },
  mail: { menus: ["Mail", "Edit", "Mailbox", "Message", "Window"], title: "Inbox", ic: "mail", body: () => mailBody() },
  paint: { menus: ["Paint", "File", "Edit", "View", "Window"], title: "invoice-0412.png", ic: "image", body: () => previewBody() },
  settings: { menus: ["Settings", "Edit", "Window"], title: "Appearance", ic: "settings", body: () => "" },
  steam: { menus: [], title: "Steam", ic: "gamepad", body: () => `<div style="background:linear-gradient(180deg,#1b2838,#171a21)"></div>` },
};

const SHEETS = {
  Projects: [["New Tab…", "⌘T"], ["Split Down", "⇧⌘↓"], ["Split Right", "⇧⌘→"], ["Close Pane", "⌘W"], "-", ["About Projects"], "-", ["Quit Projects", "⌘Q", true]],
  Window: [["Hide", "⌘H"], ["Cycle Windows", "⌘`"], "-", ["Float", "⌘ Pad *"], "-", ["Left", "⌘ Pad 4", true], ["Right", "⌘ Pad 6"]],
  Flower: [["Launch Application…"], ["Keyboard Shortcuts", "⌘K"], "-", ["Restart Shell"], "-", ["Quit Sola", "", true], ["Restart Computer"], ["Shut Down"]],
};

function sheet(name, o = {}) {
  const rows = (SHEETS[name] || []).map((r) =>
    r === "-" ? `<div class="sep"></div>` : `<div class="row${r[2] ? " is-hot" : ""}">${r[0]}${r[1] ? `<span class="row__key">${r[1]}</span>` : ""}</div>`);
  return `<div class="${o.menu ? "menu" : "sheet"}" data-under="${name}" style="top:${o.top ?? 42}px;${o.left !== undefined ? `left:${o.left}px;` : ""}${o.style || ""}">${rows.join("")}</div>`;
}

/* The titlebar: close dot, the app's menus (its name first, bold), then
 * the window's title in tertiary ink at the right. Menus that do not fit
 * are left off; the title is cut, or dropped under 32 px of room. */
function titlebar(app, o = {}) {
  const a = typeof app === "string" ? APPS[app] : app;
  const title = o.title ?? a.title;
  const menus = (o.menus || a.menus).map((m) => `<span class="tb__menu${o.open === m ? " is-open" : ""}">${m}</span>`);
  return `<div class="tb"><span class="tb__close"></span>${menus.join("")}${title ? `<span class="tb__title">${title}</span>` : ""}</div>`;
}

function win(app, o = {}) {
  const a = typeof app === "string" ? APPS[app] : app;
  const cls = ["win", o.focus ? "is-focus" : "", o.float ? "win--float" : "", o.cls || ""].join(" ");
  const tb = o.tb === false ? "" : titlebar(a, o.tb || {});
  const body = o.body ?? a.body();
  return `<div class="${cls}" style="left:${o.x}px;top:${o.y}px;width:${o.w}px;height:${o.h}px;${o.style || ""}">
    ${tb}<div class="win__body">${body}</div>${o.sheet ? sheet(o.sheet, o.sheetOpts) : ""}
  </div>`;
}

/* ── The tab bar ────────────────────────────────────────────────
 * Head: the showing workspace (its menu switches). Then Full, Left,
 * Right, Floating, each a row per window. Row states: focus (the
 * highlight pill), shown (a zone's front window, lifted 5%), bright (shown
 * while its zone is off screen), behind (70% ink), hover (+7%, the red
 * stoplight closes it). A working or waiting tab shows its mark in the
 * icon's place. */
const TABS = {
  Full: [{ ic: "prompt", t: "Sola", bright: true }],
  Left: [
    { ic: "globe", t: "Primary — Claude" },
    { ic: "prompt", t: "Sola", focus: true },
  ],
  Right: [
    { ic: "cube", t: "Steam" },
    { ic: "settings", t: "Sola Settings" },
    { ic: "terminal", t: "home", shown: true },
    { ic: "terminal", t: "Oath", m: "ring" },
    { ic: "music", t: "Spotify" },
    { ic: "folder", t: "Oath" },
  ],
  Floating: [],
};

function tabRows(rows) {
  return rows.map((r) => {
    const cls = ["tabbar__row", r.focus ? "is-focus" : "", r.shown ? "is-shown" : "", r.bright ? "is-bright" : "", r.hover ? "is-hover" : ""].join(" ");
    const lead = r.m ? mark[r.m](12) : icon(r.ic, 14);
    return `<div class="${cls}"><span class="lead">${lead}</span><span>${r.t}</span>${r.hover ? `<span class="stop"></span>` : r.sub ? `<span class="sub">${r.sub}</span>` : ""}</div>`;
  }).join("");
}

function tabbar(o = {}) {
  const zones = o.zones || TABS;
  const ws = o.ws || "Main";
  const marks = o.wsMarks || "";
  const head = `<div class="tabbar__head${o.headOpen ? " is-open" : ""}">${ws}${icon("chevD", 12, "currentColor", 2).replace('class="ic"', 'class="ic chev"')}${marks}</div>`;
  const body = Object.entries(zones).map(([z, rows]) => `<div class="tabbar__zone">${z}</div>${tabRows(rows)}`).join("");
  return `<div class="tabbar" style="left:${o.x ?? 10}px;top:${o.y ?? 48}px;width:${o.w ?? 240}px;${o.h ? `height:${o.h}px` : `bottom:${o.bottom ?? 10}px`}">${head}${body}</div>`;
}

/* ── The desk ───────────────────────────────────────────────────
 * Tiles and the tab bar start at y 48 (gap, island, gap). The content
 * area is right of the bar, a gap from every edge. Zones: Full, or Left
 * and Right as halves; Full and the halves take turns on screen. */
function zoneRects(W, H, o = {}) {
  const g = 10, top = 48;
  const x0 = o.noBar ? g : g + 240 + g;
  const cw = W - x0 - g, h = H - top - g;
  const half = (cw - g) / 2;
  return {
    full: { x: x0, y: top, w: cw, h },
    left: { x: x0, y: top, w: Math.floor(half), h },
    right: { x: x0 + Math.floor(half) + g, y: top, w: Math.ceil(half), h },
  };
}

/* o: { w, h, picture, scheme, top, bar, wins: [{app, zone|rect, focus, …}], extra } */
function desk(o = {}) {
  const W = o.w || 1920, H = o.h || 1080;
  const z = zoneRects(W, H, o);
  const wins = (o.wins || [
    { app: "projects", zone: "left", focus: true },
    { app: "terminal", zone: "right" },
  ]).map((w) => win(w.app, { ...(w.rect || z[w.zone]), ...w }));
  const bar = o.noBar ? "" : tabbar(o.bar || {});
  const cls = ["desk", o.picture ? "desk--picture" : "", o.scheme ? `scheme-${o.scheme}` : "", o.cls || ""].join(" ");
  return `<div class="${cls}" style="width:${W}px;height:${H}px;${o.style || ""}">${topRow(o.top || {})}${bar}${wins.join("")}${o.extra || ""}</div>`;
}

/* A crop of a desk, scaled: view(html, {x, y, w, h, s}). */
function view(html, c) {
  const s = c.s || 1;
  return `<div class="stage${c.picture ? " stage--picture" : ""}" style="width:${c.w}px;height:${c.h}px;${c.style || ""}"><div style="position:absolute;left:${-(c.x || 0) * s}px;top:${-(c.y || 0) * s}px;transform:scale(${s});transform-origin:0 0">${html}</div></div>`;
}

/* ── App bodies ─────────────────────────────────────────────── */
function projectsBody(o = {}) {
  const chips = (o.chips || [["root", "ring", true]]).map(([t, m, on, n]) =>
    `<span class="chip${on ? " is-on" : ""}">${m ? mark[m](11) : ""}${t}${n ? `<span class="n">×${n}</span>` : ""}</span>`).join("");
  return `<div class="proj"><div class="proj__bar">${chips}<span class="proj__add">${icon("plus", 14)}</span></div><div class="term">${(o.lines || termClaude()).join("\n")}</div></div>`;
}

function termClaude() {
  return [
    `<span class="q">❯ Alright, update foundation to match what we built on next.</span>`,
    ``,
    `<span class="g">●</span> Read design/foundation/canvas.toml`,
    `<span class="g">●</span> Read crates/sola-kit/src/components/window_frame/paint.rs`,
    `  <span class="m">The rim is the highlight a step lighter, in its hue.</span>`,
    ``,
    `<span class="g">●</span> Update(design/tokens.css)`,
    `  <span class="r">-  --hairline: #222222;</span>`,
    `  <span class="g">+  --hairline: #252525;   /* white 7% over raised */</span>`,
    ``,
    `<span class="c">*</span> <span class="m">Drawing the islands… (2m 14s · ↓ 9.9k tokens)</span>`,
    ``,
    `❯ <span class="cur"></span>`,
  ];
}

function termHome() {
  return `<span class="m">17:46:21</span> <span class="p">home@canto</span> ~ § <span class="cur"></span>`;
}

function browserBody() {
  return `<div class="br"><div class="br__bar">${icon("chevL", 16)}${icon("chevR", 16)}${icon("refresh", 14)}<div class="omni">wayland.app/<b>protocols/ext-background-effect-v1</b></div>${icon("shield", 14)}${icon("download", 14)}</div>
    <div class="page"><h1>Background effect</h1><div class="sub">ext-background-effect-v1 · staging</div>
      <div class="bars"><i style="width:96%"></i><i style="width:88%"></i><i style="width:54%"></i></div>
      <h2>Interfaces</h2><div class="code">ext_background_effect_manager_v1
ext_background_effect_surface_v1</div></div></div>`;
}

function mailBody() {
  const msg = (from, time, subj, prev, unread, sel) => `<div class="msg${sel ? " is-sel" : ""}">
    <div class="msg__top">${unread ? '<span class="msg__dot"></span>' : ""}${from}<time>${time}</time></div>
    <div>${subj}</div><div class="msg__prev">${prev}</div></div>`;
  return `<div class="mail"><div class="mail__list"><div class="mail__head">Inbox <span>12 unread</span></div>
      ${msg("Hetzner Online", "14:02", "Invoice 0412 for October", "Your invoice is attached as a PDF.", true)}
      ${msg("Cal.com", "13:40", "Design review moved to Monday", "The Sola foundation review is now…", true, true)}
      ${msg("GitHub", "12:15", "[sola] CI passed on next", "All 4 jobs succeeded for 423b9ea.")}
    </div><div class="mail__read"><h3>Design review moved to Monday</h3><div class="from">Cal.com · notifications@cal.com · 13:40</div>
      <p>The Sola foundation review moved to Monday at 10:00.</p><p>On the agenda: the foundation as built, and what Instrument would add to the panels.</p></div></div>`;
}

function previewBody() {
  return `<div class="pv"><div class="paper"><i class="k" style="width:45%"></i><i style="width:92%"></i><i style="width:80%"></i><i style="width:88%"></i><i style="width:60%"></i><i class="k" style="width:30%;margin-top:10px"></i><i style="width:70%"></i></div></div>`;
}

/* ── The stat panels as built (shell stats/view.rs) ──────────────
 * 320 wide: label, a 30 px mono value tinted by level, identity on the
 * right; a 58 px graph box with a #00d4ff line and 25% area; sections
 * below. The colors are the code's own, not the theme's. */
function statGraph(s, max) {
  const w = 296, h = 58, n = s.length, x = (i) => (i / (n - 1)) * w, y = (v) => h - Math.min(1, v / max) * h;
  const pts = s.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  return `<svg width="100%" height="58" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none"><polygon points="0,${h} ${pts} ${w},${h}" fill="#00d4ff40"/><polyline points="${pts}" fill="none" stroke="#00d4ff" stroke-width="1.5" vector-effect="non-scaling-stroke"/></svg>`;
}
const statProcs = (rows, fmt) => rows.map(([n, v]) => `<div class="st__proc"><div class="n"><span>${n}</span><span>${fmt(v)}</span></div><div class="bar"><i style="width:${(v / rows[0][1]) * 100}%"></i></div></div>`).join("");
const statFoot = (a, b, c, d) => `<div class="st__foot"><div><span>${a}</span><span>${b}</span></div><div><span>${c}</span><span>${d}</span></div></div>`;
const statPanel = (o) => `<div class="st" data-anchor="${o.anchor}" style="top:38px">
    <div class="st__head"><div><div class="st__l">${o.label}</div><div class="st__v" style="color:${o.color}">${o.value}</div></div><div class="st__id"><span class="st__model">${o.model}</span><span class="st__sub">${o.sub}</span></div></div>
    <div class="st__sec"><div class="st__cap"><span>Last 60 seconds</span><span>${o.peak}</span></div><div class="st__graph">${statGraph(o.hist, o.max)}</div></div>
    ${o.body}</div>`;

/* ── The panels as built since 2026-10-24 (stats/view.rs, dots.rs) ──
 * The island's LEDs at 4 px on a 5 px pitch: the value in 5 × 7 dots in a
 * well (four cells, right-aligned), the last minute as 60 columns tinted
 * by their own sample, per-thread columns (a dark one between cores), LED
 * bars for processes and memory. Colors are the theme's. */
const F57 = {
  "0": [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  "1": ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
  "2": [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
  "3": ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
  "4": ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
  "5": ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
  "6": ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
  "7": ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
  "8": [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
  "9": [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
  B: ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
  G: [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
  K: ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
  M: ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
  "%": ["##...", "##..#", "...#.", "..#..", ".#...", "#..##", "...##"],
  ".": [".", ".", ".", ".", ".", ".", "#"],
  " ": ["..", "..", "..", "..", "..", "..", ".."],
};
const PANEL = { dot: 4, pitch: 5 };
const span = (n, g = PANEL) => n * g.pitch - (g.pitch - g.dot);
const levelInk = (v) => (v >= 90 ? "var(--danger)" : v >= 75 ? "var(--warning)" : "var(--text-primary)");

/* `text` right-aligned in `cells` cells, lit in `color`, the field unlit. */
function readoutSvg(text, cells, color) {
  const rows = Array.from({ length: 7 }, () => []);
  [...text.toUpperCase()].forEach((ch, i) => (F57[ch] || F57[" "]).forEach((line, y) => {
    if (i) rows[y].push(0);
    for (const c of line) rows[y].push(c === "#" ? 1 : 0);
  }));
  const cols = cells * 6 - 1, lead = Math.max(0, cols - rows[0].length);
  const W = span(cols), H = span(7);
  let r = "";
  for (let y = 0; y < 7; y++)
    for (let x = 0; x < cols; x++) {
      const on = x >= lead && rows[y][x - lead];
      r += `<rect x="${x * 5}" y="${y * 5}" width="4" height="4" style="fill:${on ? color : "var(--led-off)"}"/>`;
    }
  return `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">${r}</svg>`;
}

/* A cell `cf` lit (0..1): whole, or dithered over 2 × 2 quarters. */
function cellSvg(x, y, cf, lit, rest = "var(--led-off)") {
  if (cf <= 0 || cf >= 1) return `<rect x="${x}" y="${y}" width="4" height="4" style="fill:${cf >= 1 ? lit : rest}"/>`;
  let r = "";
  for (let dy = 0; dy < 2; dy++)
    for (let dx = 0; dx < 2; dx++)
      r += `<rect x="${x + dx * 2}" y="${y + dy * 2}" width="2" height="2" style="fill:${cf > BAYER[dy][dx] ? lit : rest}"/>`;
  return r;
}

/* Columns lit from the bottom: `vals` against `max`, each in `ink(v)`; null is a dark column. */
function columnsSvg(vals, { rows = 8, max = 100, ink = levelInk } = {}) {
  const W = span(vals.length), H = span(rows);
  let r = "";
  vals.forEach((v, x) => {
    const cells = v == null ? 0 : Math.min(1, v / max) * rows;
    for (let fb = 0; fb < rows; fb++) r += cellSvg(x * 5, (rows - 1 - fb) * 5, Math.max(0, Math.min(1, cells - fb)), v == null ? "" : ink(v));
  });
  return `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">${r}</svg>`;
}

/* One row of `cells` lit in runs [[fraction, color], …]; a run's last cell dithers into the next. */
function barSvg(runs, cells) {
  const spans = [];
  let at = 0;
  for (const [f, c] of runs) { const end = at + Math.max(0, Math.min(1, f)) * cells; if (end > at) spans.push([at, end, c]); at = end; }
  let r = "";
  for (let i = 0; i < cells; i++) {
    const k = spans.findIndex(([a, b]) => i >= a && i < b);
    r += k < 0 ? cellSvg(i * 5, 0, 0, "") : cellSvg(i * 5, 0, Math.min(1, spans[k][1] - i), spans[k][2], spans[k + 1] ? spans[k + 1][2] : "var(--led-off)");
  }
  return `<svg width="${span(cells)}" height="4" viewBox="0 0 ${span(cells)} 4" shape-rendering="crispEdges">${r}</svg>`;
}

/* A panel: o = { anchor, label, value, ink, model, sub, peak, hist, max, histInk, sections: [html] } */
const dp = {
  cap: (l, r = "") => `<div class="dp__cap"><span class="dp__caps">${l}</span><span class="dp__fig">${r}</span></div>`,
  sec: (l, r, body) => `<div class="dp__sec">${dp.cap(l, r)}${body}</div>`,
  rule: () => `<div class="dp__rule"></div>`,
  procs: (rows, kind, fmt) => dp.sec("Top processes", `by ${kind}`, `<div class="dp__procs">${rows.map(([n, v]) =>
    `<div class="dp__proc"><span>${n}</span>${barSvg([[v / rows[0][1], "var(--text-primary)"]], 16)}<span class="dp__fig">${fmt(v)}</span></div>`).join("")}</div>`),
  foot: (pairs) => `<div class="dp__foot">${pairs.map(([l, v]) => `<span><span class="dp__caps">${l}</span><span class="dp__fig">${v}</span></span>`).join("")}</div>`,
};
const dottedPanel = (o) => `<div class="dp" data-anchor="${o.anchor}" style="top:38px">
    <div class="dp__head"><div><div class="dp__caps">${o.label}</div><div class="dp__well">${readoutSvg(o.value, 4, o.ink)}</div></div>
      <div class="dp__id"><span class="dp__model">${o.model}</span><span class="dp__fig dp__dim">${o.sub}</span></div></div>
    ${dp.sec("Last 60 seconds", o.peak, columnsSvg(o.hist.slice(-60), { max: o.max || 100, ink: o.histInk || levelInk }))}
    ${(o.sections || []).join("")}</div>`;

/* ── Tokens, read live so printed values cannot go stale ────── */
function tokenValue(name, el = document.documentElement) {
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  el.appendChild(probe);
  const rgb = getComputedStyle(probe).color;
  probe.remove();
  return toHex(rgb);
}

function toHex(c) {
  // Computed colors come back as rgb()/rgba() or, from color-mix, as
  // color(srgb r g b / a) with 0–1 channels.
  const s = c.match(/color\(srgb ([\d.e-]+) ([\d.e-]+) ([\d.e-]+)(?: \/ ([\d.e-]+))?\)/);
  const n = s ? [...s.slice(1, 4).map((x) => Number(x) * 255), s[4] === undefined ? 1 : Number(s[4])] : (c.match(/[\d.]+/g) || []).map(Number);
  if (n.length < 3) return c;
  const h = (v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0");
  return `#${h(n[0])}${h(n[1])}${h(n[2])}${n[3] === undefined || n[3] >= 1 ? "" : h(n[3] * 255)}`;
}

/* WCAG contrast of two hex colors (opaque). */
function contrast(fg, bg) {
  const lum = (hex) => {
    const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
      .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  };
  const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
  return (a + 0.05) / (b + 0.05);
}

/* Fill every [data-token] with its live value. */
function fillTokens(root = document) {
  root.querySelectorAll("[data-token]").forEach((el) => {
    el.textContent = tokenValue(el.dataset.token, el.parentElement || root.documentElement);
  });
}

function hangSheets(root = document) {
  root.querySelectorAll(".win").forEach((w) => {
    const s = w.querySelector(".sheet[data-under]");
    const p = w.querySelector(".tb .is-open");
    if (!s || !p || s.style.left) return;
    s.style.left = p.offsetLeft - 5 + "px";
  });
}

function finish(root = document) {
  paintLeds(root);
  fillTokens(root);
  hangSheets(root);
  document.fonts.ready.then(() => { hangSheets(root); paintLeds(root); });
}
