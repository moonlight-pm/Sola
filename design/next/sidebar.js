/* Board 05: the master sidebar. Draws whichever of its sections a page has. */
{
  const set = (id, html) => { const el = document.getElementById(id); if (el) el.innerHTML = html; };
  const S = 1296 / 2560;
  const view = (html, c) => {
    const s = c.s || 1;
    return `<div class="crop" style="width:${c.w}px;height:${c.h}px"><div style="position:absolute;left:${-c.x * s}px;top:${-c.y * s}px;transform:scale(${s});transform-origin:0 0">${html}</div></div>`;
  };

  /* What is where on Sola */
  const SOLA = [
    { k: "Left", z: 0, wins: [
      { ic: "bot", t: "kit-foundation · claude", show: true, focus: true, m: mark.ring(11),
        tabs: [["kit-foundation", mark.ring(10), true], ["menubar-pass", mark.diamond(10)], ["mail-focus", mark.check(10)]] },
      { ic: "terminal", t: "~/Workspace/Sola" },
    ] },
    { k: "Center", z: 1, wins: [
      { ic: "globe", t: "ext-background-effect-v1", show: true,
        tabs: [["ext-background-effect-v1", "", true], ["river-window-management-v1"], ["xdg-decoration-unstable-v1"], ["niri 26.04 — LWN.net"], ["iced 0.14 — docs.rs"]] },
      { ic: "globe", t: "sola · GitHub" },
    ] },
    { k: "Right", z: 2, wins: [{ ic: "terminal", t: "cargo make build sola-kit", show: true }] },
    { k: "Floating", z: "float", wins: [{ ic: "image", t: "invoice-0412.png", show: true }] },
  ];
  const OTHERS = [
    { n: 2, name: "Mail", icons: ["mail", "calendar", "globe"], m: wsMark.unread() },
    { n: 3, name: "Game", icons: ["gamepad"] },
  ];

  /* The bar. o.drag: lift "sola · GitHub" toward Right. o.tabs: one list. o.collapsed: every workspace folded. */
  function mbBar(o = {}) {
    const rows = [];
    const wsRow = (n, name, on, extra = "") =>
      `<div class="mb__ws${on ? "" : " is-off"}"><span class="n">${n}</span>${icon(on ? "chevD" : "chevR", 12, "currentColor", 2).replace('class="ic"', 'class="ic chev"')}${name}${extra}</div>`;
    if (o.collapsed) {
      rows.push(wsRow(1, "Sola", false, `<span class="icons">${["bot", "globe", "terminal", "image"].map((i) => icon(i, 13)).join("")}</span><span class="meta">${mark.ring(10)}</span>`));
    } else {
      rows.push(wsRow(1, "Sola", true, `<span class="meta">6</span>`));
      for (const zone of SOLA) {
        const drop = o.drag && zone.k === "Right";
        rows.push(`<div class="mb__zone${drop ? " is-drop" : ""}">${zoneGlyph(zone.z)}${zone.k}<span class="c">${zone.wins.length + (drop ? 1 : 0) - (o.drag && zone.k === "Center" ? 1 : 0)}</span></div>`);
        for (const w of zone.wins) {
          const lifted = o.drag && w.t === "sola · GitHub";
          const cls = ["mb__win", w.show ? "is-show" : "", w.focus ? "is-focus" : "", lifted ? "is-ghost" : ""].join(" ");
          rows.push(`<div class="${cls}">${icon(w.ic, 13)}<span>${w.t}</span>${w.m || ""}</div>`);
          if (o.tabs && w.tabs) for (const [t, m, on] of w.tabs) rows.push(`<div class="mb__tab${on ? " is-on" : ""}"><span>${t}</span>${m || ""}</div>`);
        }
        if (drop) rows.push(`<div class="mb__line"></div><div class="mb__win is-lift">${icon("globe", 13)}<span>sola · GitHub</span></div>`);
      }
    }
    rows.push(`<div class="mb__sep"></div>`);
    for (const w of OTHERS) rows.push(wsRow(w.n, w.name, false, `<span class="icons">${w.icons.map((i) => icon(i, 13)).join("")}</span><span class="meta">${w.m || ""}</span>`));
    rows.push(`<div class="mb__new">${icon("plus", 13)}New workspace</div>`);
    return rows.join("");
  }

  function mbRail() {
    const out = [`<div class="mb__num is-on">1</div>`];
    for (const zone of SOLA) {
      out.push(`<div class="mb__gap"></div>`);
      for (const w of zone.wins) out.push(`<div class="mb__ico${w.show ? " is-show" : ""}${w.focus ? " is-focus" : ""}">${icon(w.ic, 14)}</div>`);
    }
    out.push(`<div class="mb__sep" style="width:28px;margin:10px 0"></div>`);
    out.push(`<div class="mb__num">2${wsMark.unread()}</div><div class="mb__num">3</div><div class="mb__num">${icon("plus", 13)}</div>`);
    return out.join("");
  }

  const term = { ...APPS.terminal, title: "cargo make build sola-kit" };

  /* The desk with the bar docked (240), as a rail (52), or summoned (0). */
  function sideDesk(o = {}) {
    const W = 2560, H = 1080, g = 10, mode = o.mode || "docked";
    const bw = mode === "docked" ? 240 : mode === "rail" ? 52 : 0;
    const x0 = bw ? g + bw + g : g, zw = (W - x0 - g - 2 * g) / 3;
    const r = [0, 1, 2].map((i) => {
      const x = Math.round(x0 + i * (zw + g)), x2 = Math.round(x0 + i * (zw + g) + zw);
      return { x, y: 48, w: x2 - x, h: H - 48 - g };
    });
    const wins = [
      win("projects", { ...r[0], zone: 0, focus: true, tb: { stack: 2 } }),
      win("browser", { ...r[1], zone: 1, tb: { stack: 2 } }),
      win(term, { ...r[2], zone: 2 }),
      win("preview", { x: 1520, y: 600, w: 520, h: 380, float: true }),
    ];
    const top = mode === "sheet"
      ? topRow("spaces", { spaces: { current: 0, marks: { 1: wsMark.unread() }, open: true } })
      : topRow("bubbles", {});
    let b = "";
    if (mode === "docked") b = `<div class="mb" style="left:${g}px;top:48px;width:${bw}px;bottom:${g}px">${mbBar(o)}</div>`;
    if (mode === "rail") b = `<div class="mb mb--rail" style="left:${g}px;top:48px;width:${bw}px;bottom:${g}px">${mbRail()}</div>`;
    if (mode === "sheet") b = `<div class="mb mb--sheet" style="left:44px;top:44px;width:270px">${mbBar(o)}</div>`;
    return `<div class="desk" style="width:${W}px;height:${H}px">${top}${b}${wins.join("")}</div>`;
  }

  set("desk", view(sideDesk(), { x: 0, y: 0, w: 1296, h: 547, s: S }));
  set("anat", view(sideDesk(), { x: 0, y: 0, w: 700, h: 640 }));

  /* States */
  const stage = (inner, h = 600) => `<div class="stage" style="width:260px;height:${h}px"><div class="mb" style="left:10px;top:10px;width:240px;bottom:10px">${inner}</div></div>`;
  set("states", [
    ["Pick a window", "A click shows that window in its zone and focuses it. The one it replaces stays on the stack. Super+\\` walks the focused zone's stack.", stage(mbBar())],
    ["Drag it to another zone", "Dragging a row onto a zone header (or between rows) moves the window there; dropping on another workspace's row sends it there.", stage(mbBar({ drag: true }))],
    ["Fold everything", "Every workspace on one row: what it holds as icons, what needs you as marks. The bar a large desk might keep docked.", stage(mbBar({ collapsed: true }), 240)],
  ].map(([h, p, s]) => `<div class="sample">${s}<div class="sample__head"><h4>${h}</h4></div><p>${p}</p></div>`).join(""));

  /* Zone schematics: thirds after the bar */
  const box = (parts) => {
    // parts: [label, kind] for bar + zones; widths computed from what is filled.
    const W = 290, g = 6, bw = 30;
    const filled = parts.filter(([, k]) => k !== "empty");
    const zw = (W - g - bw - g - g * (filled.length - 1) - g) / filled.length;
    let x = g, html = `<i class="is-bar" style="left:${x}px;width:${bw}px"></i>`;
    x += bw + g;
    for (const [l, k] of parts) {
      if (k === "empty") continue;
      html += `<i style="left:${x}px;width:${zw}px">${l}</i>`;
      x += zw + g;
    }
    return `<div class="zs__box">${html}</div>`;
  };
  set("zones", [
    ["Three", "Left, Center, Right, a third each.", [["LEFT"], ["CENTER"], ["RIGHT"]]],
    ["Center empty", "Left and Right take half each. Center's header stays in the bar, empty, as a drop target.", [["LEFT"], ["", "empty"], ["RIGHT"]]],
    ["One", "The one zone with windows takes the width after the bar.", [["LEFT"], ["", "empty"], ["", "empty"]]],
    ["Floating only", "No tiles: the ground shows, and floats sit on it.", [["", "empty"], ["", "empty"], ["", "empty"]]],
  ].map(([h, p, parts]) => `<div class="sample">${box(parts)}<div class="sample__head"><h4>${h}</h4></div><p>${p}</p></div>`).join(""));

  /* Three lists, then one list */
  set("lists", view(sideDesk(), { x: 0, y: 30, w: 1296, h: 380 }));
  set("onelist",
    `<div class="stage" style="width:260px;height:470px"><div class="mb" style="left:10px;top:10px;width:240px;bottom:10px">${mbBar({ tabs: true })}</div></div>`);

  /* Modes */
  set("modes", [
    ["Docked", "", "Always there: 240 px and a gap. The tree is one glance away, and every workspace pays for it.", sideDesk({ mode: "docked" })],
    ["Rail", "", "52 px: workspace numbers, then each zone's windows as icons. Hover widens it to the full bar.", sideDesk({ mode: "rail" })],
    ["Summoned", "PROPOSED", "No bar. 04's workspace bubble grows down into the tree when you click it or hold Super; the zones keep the full width.", sideDesk({ mode: "sheet" })],
  ].map(([h, tag, p, html]) => `<div class="sample">${view(html, { x: 0, y: 0, w: 410, h: 480 })}<div class="sample__head"><h4>${h}</h4>${tag ? `<span class="tag is-pick">${tag}</span>` : ""}</div><p>${p}</p></div>`).join(""));

  finish();
}
