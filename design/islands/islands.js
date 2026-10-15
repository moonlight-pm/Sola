/* Islands: shared parts. Pages call these to build the band, islands,
 * marks, LED meters, and desk windows, so every board draws the same thing. */
const FLOWER_D = "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.300-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.600-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.900-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";

const ICONS = {
  mail: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  calendar: '<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
  calculator: '<rect width="16" height="20" x="4" y="2" rx="2"/><path d="M8 6h8M16 10h.01M12 10h.01M8 10h.01M12 14h.01M8 14h.01M12 18h.01M8 18h.01M16 14v4"/>',
  gamepad: '<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258A4 4 0 0 0 17.32 5z"/>',
  scan: '<path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3"/><path d="m16 16-1.9-1.9"/>',
  capture: '<path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2"/><rect x="7" y="8" width="10" height="8" rx="1"/>',
  image: '<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>',
  terminal: '<path d="m7 11 2-2-2-2M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20"/>',
  settings: '<path d="M20 7h-9M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>',
  bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2M20 14h2M15 13v2M9 13v2"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  pause: '<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>',
  next: '<path d="M5 4l10 8-10 8z"/><path d="M19 5v14"/>',
  prev: '<path d="M19 20 9 12l10-8z"/><path d="M5 19V5"/>',
  volume: '<path d="M11 4.7a.7.7 0 0 0-1.2-.5L6.4 7.6A1.4 1.4 0 0 1 5.4 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.4a1.4 1.4 0 0 1 1 .4l3.4 3.4a.7.7 0 0 0 1.2-.5z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.4 18.4a9 9 0 0 0 0-12.8"/>',
  headphones: '<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>',
  speaker: '<rect width="16" height="20" x="4" y="2" rx="2"/><circle cx="12" cy="14" r="4"/><path d="M12 6h.01"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  chevL: '<path d="m15 18-6-6 6-6"/>',
  chevR: '<path d="m9 18 6-6-6-6"/>',
  palette: '<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/>',
  lock: '<rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  star: '<path d="M12 2l3.1 6.3 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21l1.2-6.8-5-4.9 6.9-1z"/>',
};

function icon(name, size = 14, color = "currentColor", sw = 1.75) {
  return `<svg class="ic" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
}

function flower(size = 14) {
  return `<svg class="flower" width="${size}" height="${size}" viewBox="-25 -32 561 561"><path d="${FLOWER_D}" fill="var(--sol)"/></svg>`;
}

/* Shape first: working is a ring, waiting a diamond, done a check. */
const mark = {
  ring: (size = 12, color = "var(--accent)") =>
    `<svg class="mark mark--ring" width="${size}" height="${size}" viewBox="0 0 12 12"><path d="M6 1.6A4.4 4.4 0 1 1 1.6 6" fill="none" stroke="${color}" stroke-width="1.7" stroke-linecap="round"/></svg>`,
  diamond: (size = 12, color = "var(--warning)") =>
    `<svg class="mark mark--diamond" width="${size}" height="${size}" viewBox="0 0 12 12"><rect x="3" y="3" width="6" height="6" rx="1" transform="rotate(45 6 6)" fill="${color}"/></svg>`,
  check: (size = 12, color = "var(--success)") =>
    `<svg class="mark mark--check" width="${size}" height="${size}" viewBox="0 0 12 12"><path d="M2.6 6.3 5 8.6 9.6 3.6" fill="none" stroke="${color}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  dot: (size = 12, color = "var(--ink-3)") =>
    `<svg class="mark mark--dot" width="${size}" height="${size}" viewBox="0 0 12 12"><circle cx="6" cy="6" r="2" fill="${color}"/></svg>`,
};

/* ── LED meters (2px dots on a 3px pitch, painted at device pixels) ── */
const LED = {
  cpu: { vals: [1, 1, 2, 1, 3, 2, 1, 1, 2, 4, 2, 2], color: "#e9ecf2" },
  gpu: { vals: [0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1], color: "#e9ecf2" },
  mem: { vals: [3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4], color: "#e9ecf2", warn: 4 },
  rx: { vals: [0, 1, 0, 2, 1, 0, 1, 3, 1, 0, 2, 1], color: "#3dd6f5" },
  tx: { vals: [0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0], color: "#3ecf8e" },
};
const SPECTRUM = [3, 4, 5, 4, 3, 4, 2, 3, 2, 2, 1, 1];
const PHOSPHOR = ["#38fa6b", "#1febe0", "#6185ff", "#c752ff", "#ff942e"];

function leds(kind, scale = 1) {
  return `<canvas class="leds" data-kind="${kind}" data-scale="${scale}"></canvas>`;
}

function paintLeds(root = document) {
  const dpr = window.devicePixelRatio || 1;
  root.querySelectorAll("canvas.leds").forEach((cv) => {
    const kind = cv.dataset.kind;
    const s = parseFloat(cv.dataset.scale || "1");
    const dot = 2 * s, pitch = 3 * s, rows = 5;
    const spectrum = kind === "spectrum";
    const cols = spectrum ? 12 * 3 : 12;
    const bandGap = spectrum ? 2 * s : 0;
    const w = spectrum ? 12 * (3 * pitch) + 11 * bandGap - (pitch - dot) : cols * pitch - (pitch - dot);
    const h = rows * pitch - (pitch - dot);
    cv.style.width = w + "px";
    cv.style.height = h + "px";
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    const g = cv.getContext("2d");
    g.scale(dpr, dpr);
    const off = "rgba(233,236,242,0.12)";
    for (let c = 0; c < cols; c++) {
      const band = spectrum ? Math.floor(c / 3) : c;
      const x = spectrum ? band * (3 * pitch + bandGap) + (c % 3) * pitch : c * pitch;
      const v = spectrum ? SPECTRUM[band] : LED[kind].vals[c];
      for (let r = 0; r < rows; r++) {
        const y = h - dot - r * pitch;
        let col = off;
        if (r < v) {
          if (spectrum) col = PHOSPHOR[r];
          else if (LED[kind].warn && v >= LED[kind].warn) col = "#e8b84a";
          else col = LED[kind].color;
        }
        g.fillStyle = col;
        g.fillRect(x, y, dot, dot);
      }
    }
  });
}

/* ── Live (center island) content ── */
const live = {
  working: (name = "kit-foundation", t = "4m") =>
    `${mark.ring()}<span class="live__name">${name}</span><span class="live__verb">working</span><span class="live__num">${t}</span>`,
  building: (name = "kit-foundation", t = "0:03") =>
    `${mark.ring()}<span class="live__name">${name}</span><span class="live__verb">building</span><span class="live__num">${t}</span>`,
  waiting: (name = "kit-foundation") =>
    `${mark.diamond()}<span class="live__name">${name}</span><span class="live__verb">wants to run</span><code class="live__code">cargo make build</code>`,
  done: (name = "mail-focus") =>
    `${mark.check()}<span class="live__name">${name}</span><span class="live__verb">done</span><span class="live__num">12m</span>`,
  playing: () =>
    `${icon("music", 13, "var(--ink-2)")}<span class="live__name">Says</span><span class="live__verb">Nils Frahm</span><span class="live__num">3:12</span>`,
  countdown: () =>
    `${icon("calendar", 13, "var(--ink-2)")}<span class="live__name">Design review</span><span class="live__verb">in</span><span class="live__num">25 min</span>`,
  whisper: () => `<span class="live__verb" style="color: var(--ink)">Saved Screenshot 14.32.07.png</span>`,
};

/* ── The band ── */
const APP_MENUS = {
  Workspaces: ["File", "Edit", "View", "Pane", "Tabs", "Window"],
  Mail: ["File", "Edit", "Message", "View", "Window"],
  Browser: ["File", "Edit", "View", "History", "Tabs", "Window"],
};

function leftIsland(o = {}) {
  const app = o.app || "Workspaces";
  const menus = o.menus || APP_MENUS[app] || APP_MENUS.Workspaces;
  return `<div class="island island--left glass" ${o.leftStyle ? `style="${o.leftStyle}"` : ""}>
    <span class="sys${o.open === "sys" ? " is-open" : ""}">${flower(14)}</span>
    <span class="app">${app}</span>
    ${menus.map((m) => `<span class="menu${o.open === m ? " is-open" : ""}">${m}</span>`).join("")}
  </div>`;
}

function rightIsland(o = {}) {
  const open = (k) => (o.open === k ? " is-open" : "");
  return `<div class="island island--right glass" ${o.rightStyle ? `style="${o.rightStyle}"` : ""}>
    <span class="extras">${icon("mail", 14, "var(--accent)")}${icon("bell")}${icon("bluetooth")}</span>
    <span class="grp${open("spectrum")}">${leds("spectrum")}</span>
    <span class="grp stats">
      <span class="stat">CPU ${leds("cpu")}</span>
      <span class="stat">GPU ${leds("gpu")}</span>
      <span class="stat">MEM ${leds("mem")}</span>
    </span>
    <span class="grp net">
      <span class="stat">RX ${leds("rx")}</span>
      <span class="stat">TX ${leds("tx")}</span>
    </span>
    <span class="grp clock${open("clock")}">14:32 Mon 2026-10-12</span>
  </div>`;
}

function centerIsland(o = {}) {
  const html = o.center || live.working();
  return `<div class="island island--center glass" ${o.centerStyle ? `style="${o.centerStyle}"` : ""}>${html}</div>`;
}

/* o.parts: which islands to draw (default all three). */
function band(o = {}) {
  const parts = o.parts || ["left", "center", "right"];
  return `<div class="band" style="${o.width ? `width:${o.width}px;` : ""}${o.style || ""}">
    ${parts.includes("left") ? leftIsland(o) : ""}
    ${parts.includes("center") ? centerIsland(o) : ""}
    ${parts.includes("right") ? rightIsland(o) : ""}
  </div>`;
}

/* ── Desk windows ── */
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

function terminal(o = {}) {
  const lines = [
    `<span class="c">~/Workspace/Sola/.worktrees/kit-foundation</span>`,
    `❯ claude`,
    ``,
    `● Read crates/sola-kit/src/theme.rs`,
    `● Read crates/sola-kit/src/components/style.rs`,
    `  <span class="m">Hairlines are opaque white mixes on the surface,</span>`,
    `  <span class="m">so a 1px edge does not bloom under iced's</span>`,
    `  <span class="m">linear blend.</span>`,
    ``,
    `● Update(crates/sola-kit/src/fonts.rs)`,
    `  <span class="r">-  display: medium(ui_fam),</span>`,
    `  <span class="g">+  display: medium(display_fam),</span>`,
    ``,
    `  Build it?  cargo make build sola-kit`,
    `  <span class="c">❯ 1. Yes</span>`,
    `    <span class="m">2. No, and tell Claude what to do</span>`,
    `❯ <span class="sel">cargo make build</span> sola-kit <span class="cur"></span>`,
  ];
  return `<div class="term" style="${o.style || ""}">${lines.join("\n")}</div>`;
}

document.addEventListener("DOMContentLoaded", () => paintLeds());

function browser(o = {}) {
  const tabs = o.tabs || [
    { t: "ext-background-effect-v1", c: "#e8b84a", l: "W", on: true },
    { t: "niri 26.04 — LWN.net", c: "#f07178", l: "L" },
    { t: "iced 0.14 — docs.rs", c: "#3dd6f5", l: "D" },
  ];
  const url = o.url || 'wayland.app/<b>protocols/ext-background-effect-v1</b>';
  const page = o.page || `
    <h1>Background effect</h1>
    <div class="sub">ext-background-effect-v1 · staging</div>
    <div class="bars"><i style="width:96%"></i><i style="width:88%"></i><i style="width:92%"></i><i style="width:54%"></i></div>
    <h2>Interfaces</h2>
    <div class="code">ext_background_effect_manager_v1
ext_background_effect_surface_v1</div>
    <h2>Compositor support</h2>
    <div class="bars"><i style="width:72%"></i><i style="width:84%"></i><i style="width:40%"></i></div>`;
  return `<div class="browser">
    <div class="browser__tabs">${tabs.map((t) => `<div class="tab${t.on ? " is-on" : ""}"><span class="fav" style="background:${t.c}">${t.l}</span>${t.t}</div>`).join("")}</div>
    <div class="browser__bar">${icon("chevL", 16)}${icon("chevR", 16)}${icon("refresh", 14)}<div class="omni">${url}</div>${icon("lock", 14)}</div>
    <div class="page">${page}</div>
  </div>`;
}

function mail() {
  const msg = (from, time, subj, prev, unread, sel) => `<div class="msg${sel ? " is-sel" : ""}">
    <div class="msg__top">${unread ? '<span class="msg__dot"></span>' : ""}${from}<time>${time}</time></div>
    <div class="msg__subj">${subj}</div><div class="msg__prev">${prev}</div></div>`;
  return `<div class="mail">
    <div class="mail__list">
      <div class="mail__head">Inbox <span>${mark.ring(11)} 12 unread</span></div>
      ${msg("Hetzner Online", "14:02", "Invoice 0412 for October", "Your invoice is attached as a PDF.", true)}
      ${msg("Cal.com", "13:40", "Design review moved to Thursday", "The Sola foundation review is now…", true, true)}
      ${msg("GitHub", "12:15", "[sola] CI passed on master", "All 4 jobs succeeded for 1d82db1.")}
      ${msg("Fastmail", "Yesterday", "Your export is ready", "Download it within 7 days.")}
      ${msg("Grapevine", "Mon", "Weekly digest", "Three new threads in #desk.")}
    </div>
    <div class="mail__read">
      <h3>Design review moved to Thursday</h3>
      <div class="from">Cal.com · notifications@cal.com · 13:40</div>
      <div class="acts"><span class="btn btn--quiet">Reply</span><span class="btn" style="color:var(--ink-2)">Forward</span><span class="btn" style="color:var(--ink-2)">Junk</span></div>
      <p>Hi —</p>
      <p>The Sola foundation review moved to Thursday at 10:00. Same room, same link.</p>
      <p>On the agenda: the graphite sheet, the role scale, and whether the flower should be gold.</p>
      <blockquote>Can we keep the switcher pill neutral? The count marks already carry the cyan.</blockquote>
    </div>
  </div>`;
}

function previewWindow(o = {}) {
  return `<div class="float glass glass--float" style="left:${o.x}px;top:${o.y}px;width:${o.w}px;height:${o.h}px">
    <div class="float__bar"><span class="float__close"></span>Preview — invoice-0412.png</div>
    <div class="float__body" style="height:${o.h - 36}px"><div class="paper"><i class="k" style="width:45%"></i><i style="width:92%"></i><i style="width:80%"></i><i style="width:88%"></i><i style="width:60%"></i><i style="width:84%"></i><i class="k" style="width:30%;margin-top:10px"></i><i style="width:70%"></i></div></div>
  </div>`;
}

function waitingCard(o = {}) {
  return `<div class="wcard">
    <div class="wcard__top">${mark.diamond()}<span>kit-foundation is waiting</span><span class="wcard__meta">Workspaces · now</span></div>
    <div class="wcard__body">Claude asks to run <code>cargo make build sola-kit</code></div>
    ${o.expanded ? `<div class="wcard__term"><span class="m">● Update(crates/sola-kit/src/fonts.rs)</span>
  <span class="r">-  display: medium(ui_fam),</span>
  <span class="g">+  display: medium(display_fam),</span>
❯ cargo make build sola-kit</div>` : ""}
    <div class="wcard__acts"><span class="btn btn--primary">Run build</span><span class="btn btn--quiet">Show pane</span>${o.expanded ? '<span class="kbd">↵ run · esc later</span>' : ""}</div>
  </div>`;
}

/* A single island on a fitted piece of band, for 1:1 specimens. */
function chip(kind, o = {}) {
  const inner = kind === "left" ? leftIsland(o) : kind === "right" ? rightIsland(o) : centerIsland(o);
  return `<div class="band band--fit">${inner}</div>`;
}

/* The top of the 5120 desk: the bar (Graphite strip or Islands band) over
 * the three zones, at real size. Callers scale it. */
function deskTop(o = {}) {
  const strip = o.strip;
  const top = strip ? 28 : 40;
  const h = o.height || 320;
  const b = strip
    ? band({ width: 5120, parts: ["left", "right"] }).replace('class="band"', 'class="band band--strip"')
    : band({ width: 5120, center: o.center });
  return `<div style="position:relative;width:5120px;height:${h}px;background:var(--bg-primary);overflow:hidden">
    ${b}
    <div class="zone" style="left:0;top:${top}px;width:1706px;bottom:0;display:flex">
      <div style="width:220px;flex-shrink:0">${rail()}</div><div style="flex:1">${terminal()}</div>
    </div>
    <div class="zone" style="left:1706px;top:${top}px;width:1707px;bottom:0">${browser()}</div>
    <div class="zone" style="left:3413px;top:${top}px;width:1707px;bottom:0">${mail()}</div>
  </div>`;
}

/* The whole 5120 × 2160 desk in one of three zone treatments:
 * "flush" (Graphite), "gapped" (8 px gutters, 12 px corners), "islands"
 * (band + islands, flush zones). Callers scale or crop it. */
function fullDesk(kind) {
  const g = kind === "gapped" ? 8 : 0;
  const top = kind === "flush" ? 28 : 40 + (g ? 2 : 0); // gapped: 8 px under the islands too
  const zh = 2160 - top - g;
  // [left, width] per zone: flush zones split 5120 in thirds; gapped ones leave 8 px gutters.
  const zones = kind === "gapped"
    ? [0, 1, 2].map((i) => [g + i * (1696 + g), 1696])
    : [[0, 1706], [1706, 1707], [3413, 1707]];
  const r = kind === "gapped" ? "border-radius:12px;" : "";
  const z = ([x, w], html, flex) =>
    `<div class="zone" style="left:${x}px;top:${top}px;width:${w}px;height:${zh}px;${r}${flex ? "display:flex;" : ""}">${html}</div>`;
  const b = kind === "flush"
    ? band({ width: 5120, parts: ["left", "right"] }).replace('class="band"', 'class="band band--strip"')
    : band({ width: 5120 });
  return `<div style="position:relative;width:5120px;height:2160px;background:var(--band-bg);overflow:hidden">
    ${b}
    ${z(zones[0], `<div style="width:220px;flex-shrink:0">${rail()}</div><div style="flex:1">${terminal()}</div>`, true)}
    ${z(zones[1], browser())}
    ${z(zones[2], mail())}
    ${previewWindow({ x: 1480, y: 600, w: 560, h: 400 })}
  </div>`;
}
