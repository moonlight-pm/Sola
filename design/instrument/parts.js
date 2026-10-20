/* Instrument parts: the front-panel menubar, built from a state.
 * Load before instrument.js; both fill on DOMContentLoaded in that order.
 * <div data-part="bar" data-preset="idle" data-set='{"app":"Mail"}'></div> */
(() => {
  const MENUS = {
    Workspaces: ["File", "Edit", "View", "Pane", "Tabs", "Window"],
    Mail: ["File", "Edit", "Message", "View", "Window"],
    Browser: ["File", "Edit", "View", "History", "Profiles", "Window"],
  };

  const SILENT = "0 0 0 0 0 0 0 0 0 0 0 0";
  const BASE = {
    app: "Workspaces",
    open: null,
    mail: "",
    bell: "",
    bt: true,
    mic: false,
    cam: false,
    working: "",
    waiting: "",
    spectrum: SILENT,
    peaks: "",
    cpu: "8 12 9 14 22 11 9 7 10 13 9 8",
    gpu: "4 6 5 9 12 7 5 6 8 5 4 6",
    mem: "61 61 62 62 62 62 63 63 63 63 64 64",
    rx: "2 0 5 1 0 8 30 12 3 1 0 2",
    tx: "1 0 2 0 0 3 6 2 1 0 0 1",
    clock: "14:32",
    date: "TUE 13",
    whisper: null,
  };

  const PRESETS = {
    idle: {},
    warm: {
      cpu: "34 41 52 61 70 78 83 88 91 86 79 82",
      gpu: "12 18 22 31 40 44 52 49 47 51 46 44",
      mem: "71 72 74 75 77 78 78 79 80 80 79 80",
      working: "2",
    },
    unread: {
      app: "Mail",
      mail: "12",
      bell: "3",
      working: "2",
      waiting: "1",
      spectrum: "0.62 0.82 0.74 0.58 0.63 0.5 0.42 0.37 0.31 0.24 0.16 0.08",
      peaks: "0.8 0.95 0.86 0.7 0.74 0.62 0.55 0.5 0.42 0.34 0.26 0.16",
    },
    live: {
      app: "Browser",
      mic: true,
      cam: true,
      working: "1",
      spectrum: "0.34 0.52 0.46 0.4 0.3 0.22 0.16 0.12 0.08 0.05 0.03 0.0",
      peaks: "0.5 0.66 0.6 0.5 0.42 0.32 0.26 0.2 0.14 0.1 0.06 0.02",
      rx: "22 31 28 35 30 33 29 36 31 34 30 32",
      tx: "18 26 24 29 25 27 24 30 26 28 25 27",
    },
    whisper: {
      whisper: "Screenshot copied",
      elide: { menus: 6, right: ["stats", "net", "clock"] },
    },
  };

  const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;");

  const ring = (c) =>
    `<svg width="9" height="9" viewBox="0 0 9 9" style="flex-shrink:0"><circle cx="4.5" cy="4.5" r="3.4" fill="none" stroke="${c}" stroke-width="1.6"/></svg>`;
  const diamond = (c) =>
    `<svg width="9" height="9" viewBox="0 0 9 9" style="flex-shrink:0"><rect x="2" y="2" width="5" height="5" transform="rotate(45 4.5 4.5)" fill="${c}"/></svg>`;

  /** A fixed-cell readout in the bar: `cells` glyph cells, right-aligned, dark when empty. */
  function count(value, cells, color) {
    const cols = cells * 4 - 1;
    return `<span data-dots="${esc(value)}" data-font="35" data-cols="${cols}" data-align="right" style="color:${color}"></span>`;
  }

  function bar(s) {
    let menus = s.menus || MENUS[s.app] || MENUS.Workspaces;
    const live = (on) => (on ? "led led--on" : "led");
    const lampLbl = (on) => (on ? "lbl lbl--9 lbl--ink" : "lbl lbl--9 lbl--lo");
    const mailGlyph = s.mail ? "bar__glyph" : "bar__glyph bar__glyph--dim";
    const bellGlyph = s.bell ? "bar__glyph" : "bar__glyph bar__glyph--dim";
    const mods = {
      extras: `
    <div class="bar__mod" data-anchor="extras">
      <div class="bar__pair"><span class="${mailGlyph}" data-icon="mail"></span>${count(s.mail, 2, "var(--fn-focus)")}</div>
      <div class="bar__pair"><span class="${bellGlyph}" data-icon="bell"></span>${count(s.bell, 1, "var(--fn-focus)")}</div>
      <div class="bar__lamps" data-anchor="lamps">
        <div class="bar__pair"><span class="${lampLbl(s.bt)}">BT</span><span class="${live(s.bt)}"></span></div>
        <div class="bar__pair"><span class="${lampLbl(s.mic)}">MIC</span><span class="${live(s.mic)}" style="--c:var(--lamp-live)"></span></div>
        <div class="bar__pair"><span class="${lampLbl(s.cam)}">CAM</span><span class="${live(s.cam)}" style="--c:var(--lamp-live)"></span></div>
      </div>
    </div>`,
      agents: `
    <div class="bar__mod" data-anchor="agents">
      <span class="lbl lbl--9">AGT</span>
      <div class="bar__pair">${ring(s.working ? "var(--fn-focus)" : "var(--engrave-lo)")}${count(s.working, 1, "var(--fn-focus)")}</div>
      <div class="bar__pair">${diamond(s.waiting ? "var(--lamp-look)" : "var(--engrave-lo)")}${count(s.waiting, 1, "var(--lamp-look)")}</div>
    </div>`,
      audio: `
    <div class="bar__mod" data-anchor="audio">
      <span data-spectrum="${s.spectrum}" data-peaks="${s.peaks}"></span>
    </div>`,
      stats: `
    <div class="bar__mod" data-anchor="stats">
      <div class="bar__pair${s.openStat === "cpu" ? " bar__pair--open" : ""}" data-anchor="cpu"><span class="lbl lbl--9">CPU</span><span data-graph="${s.cpu}"></span></div>
      <div class="bar__pair"><span class="lbl lbl--9">GPU</span><span data-graph="${s.gpu}"></span></div>
      <div class="bar__pair"><span class="lbl lbl--9">MEM</span><span data-graph="${s.mem}"></span></div>
    </div>`,
      net: `
    <div class="bar__mod" data-anchor="net">
      <div class="bar__pair"><span class="lbl lbl--9">RX</span><span data-graph="${s.rx}" data-tint="value"></span></div>
      <div class="bar__pair"><span class="lbl lbl--9">TX</span><span data-graph="${s.tx}" data-tint="value"></span></div>
    </div>`,
      clock: `
    <div class="bar__mod" data-anchor="clock">
      <span data-dots="${esc(s.clock)}" data-font="35" data-cols="17" data-align="right"></span>
      <span class="lbl lbl--9 lbl--ink">${esc(s.date)}</span>
    </div>`,
    };
    let order = ["extras", "agents", "audio", "stats", "net", "clock"];
    let mid = "";
    if (s.elide) {
      // A slice of the 5120 px bar: both ends, the middle shown between breaks.
      menus = menus.slice(0, s.elide.menus ?? 2);
      order = s.elide.right || ["stats", "net", "clock"];
      mid = `<div class="bar__mid"><span class="bar__break"></span><span class="bar__mid-text">${esc(s.whisper || "")}</span><span class="bar__break"></span></div>`;
    } else if (s.whisper) {
      mid = `<div class="bar__whisper">${esc(s.whisper)}</div>`;
    }
    return `
<div class="bar" style="${s.width ? `width:${s.width}px` : ""}">
  <div class="bar__left">
    <div class="bar__sys" data-anchor="sys"><span data-flower="14"></span></div>
    <div class="bar__app" data-anchor="app">${esc(s.app)}</div>
    ${menus.map((m, i) => `<div class="bar__menu${m === s.open ? " bar__menu--open" : ""}"${i === 0 ? ' data-anchor="menus"' : ""}>${esc(m)}</div>`).join("")}
  </div>
  ${mid}
  <div class="bar__right">${order.map((k) => mods[k]).join("")}
  </div>
</div>`;
  }

  function state(el) {
    const preset = PRESETS[el.dataset.preset || "idle"] || {};
    const extra = el.dataset.set ? JSON.parse(el.dataset.set) : {};
    return { ...BASE, ...preset, ...extra };
  }

  function render() {
    document.querySelectorAll('[data-part="bar"]').forEach((el) => {
      el.innerHTML = bar(state(el));
    });
  }

  window.Parts = { bar, PRESETS, BASE };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", render);
  else render();
})();
