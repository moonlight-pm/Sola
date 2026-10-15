// Agent desk — builds the Graphite menubar around each `.mb` and fills the
// LED meters. Deterministic: the same seed draws the same meters.
(() => {
  const FLOWER = "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.3-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.6-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.9-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";
  const ICONS = {
    mail: '<rect x="1.5" y="3" width="11" height="8.5" rx="1.6"/><path d="M1.8 4.2 7 7.6l5.2-3.4"/>',
    bell: '<path d="M3.4 5.6a3.6 3.6 0 0 1 7.2 0c0 3.2 1.3 4.3 1.3 4.3H2.1s1.3-1.1 1.3-4.3"/><path d="M5.9 11.8a1.2 1.2 0 0 0 2.2 0"/>',
    bt: '<path d="M4 4.4 9.6 9.4 6.8 12V2l2.8 2.6L4 9.6"/>',
  };
  const PHOS = ["--phosphor-1", "--phosphor-2", "--phosphor-3", "--phosphor-4", "--phosphor-5"];

  function rng(seed) {
    let s = seed >>> 0 || 1;
    return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  }

  // cols × rows of 2px dots, lit from the bottom.
  function grid(cols, rows, level, color) {
    const g = document.createElement("div");
    g.className = "leds";
    g.style.gridTemplateRows = `repeat(${rows}, 2px)`;
    for (let c = 0; c < cols; c++) {
      const h = level(c);
      for (let r = 0; r < rows; r++) {
        const i = document.createElement("i");
        if (rows - r <= h) i.style.background = color(c, rows - r);
        g.appendChild(i);
      }
    }
    return g;
  }

  function meter(kind, seed) {
    const R = rng(seed);
    if (kind === "spectrum") {
      const bands = Array.from({ length: 12 }, (_, b) => Math.max(1, Math.round(5 * (0.35 + 0.65 * R()) * (1 - b / 16))));
      return grid(36, 5, c => bands[Math.floor(c / 3)], (c, h) => `var(${PHOS[Math.min(4, h - 1)]})`);
    }
    if (kind === "rx" || kind === "tx") {
      const col = kind === "rx" ? "var(--accent)" : "var(--success)";
      return grid(12, 5, () => (R() < .45 ? Math.ceil(R() * 3) : 0), () => col);
    }
    return grid(12, 5, () => Math.round(R() * (kind === "mem" ? 5 : 3)), () => "var(--stat-label)");
  }

  function el(tag, cls, html) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html != null) e.innerHTML = html;
    return e;
  }

  function build(mb, n) {
    const strip = mb.querySelector(".strip");
    const whisper = mb.querySelector(".mb__whisper");
    const app = mb.dataset.app || "Workspaces";
    const menus = (mb.dataset.menus || "File Edit View Pane Tabs Window").split(" ");
    const seed = Number(mb.dataset.seed || 7 + n * 13);
    const left = el("div", "mb__left");
    left.appendChild(el("div", "mb__sys", `<svg viewBox="-25 -32 561 561"><path d="${FLOWER}" fill="var(--sol)"/></svg>`));
    left.appendChild(el("div", "mb__item mb__item--app", app));
    if (mb.dataset.menus !== "none") menus.forEach(m => left.appendChild(el("div", "mb__item", m)));
    const right = el("div", "mb__right");
    if (strip) { right.appendChild(strip); right.appendChild(el("div", "mb__sep")); }
    const ex = el("div", "mb__extras");
    ex.appendChild(el("div", "mb__icon mb__icon--on", `<svg viewBox="0 0 14 14">${ICONS.mail}</svg>`));
    ex.appendChild(el("div", "mb__icon", `<svg viewBox="0 0 14 14">${ICONS.bell}</svg>`));
    ex.appendChild(el("div", "mb__icon", `<svg viewBox="0 0 14 14">${ICONS.bt}</svg>`));
    if (mb.dataset.extras !== "none") right.appendChild(ex);
    if (mb.dataset.meters !== "none") {
      if (mb.dataset.meters !== "short") {
      const sp = el("div", "mb__stat"); sp.style.padding = "0 10px 0 6px"; sp.appendChild(meter("spectrum", seed)); right.appendChild(sp);
      }
      const all = [["CPU", "cpu"], ["GPU", "gpu"], ["MEM", "mem"], ["RX", "rx"], ["TX", "tx"]];
      (mb.dataset.meters === "short" ? [all[0], all[2]] : all).forEach(([l, k], i) => {
        const s = el("div", "mb__stat", l); s.appendChild(meter(k, seed + i * 31)); right.appendChild(s);
      });
    }
    right.appendChild(el("div", "mb__clock", mb.dataset.clock || "14:32 Mon 2026-10-13"));
    mb.innerHTML = "";
    mb.appendChild(left);
    mb.appendChild(right);
    if (whisper) mb.appendChild(whisper);
  }

  const GLYPH = {
    ring: '<span class="g g--ring"><svg viewBox="0 0 12 12"><circle cx="6" cy="6" r="4.1" transform="rotate(-90 6 6)"/></svg></span>',
    diamond: '<span class="g g--diamond"></span>',
    check: '<span class="g g--check"><svg viewBox="0 0 12 12"><path d="M2.6 6.4 5 8.6 9.4 3.8"/></svg></span>',
    failed: '<span class="g g--failed"><svg viewBox="0 0 12 12"><path d="M3.6 3.6l4.8 4.8M8.4 3.6 3.6 8.4"/></svg></span>',
    idle: '<span class="g g--idle"></span>',
  };
  window.agentGlyph = k => GLYPH[k];

  function glyphs() {
    document.querySelectorAll("[data-g]").forEach(e => {
      e.insertAdjacentHTML("afterbegin", GLYPH[e.dataset.g] || "");
      e.removeAttribute("data-g");
    });
  }

  function run() {
    glyphs();
    document.querySelectorAll(".mb").forEach(build);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run);
  else run();
})();
