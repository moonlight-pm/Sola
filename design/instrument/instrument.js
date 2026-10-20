/* Instrument: dot-matrix readouts, LED meters, the flower raster, icons.
 * Everything that lights is drawn on the meters' own grid: a 2px dot on a
 * 3px pitch (sola-shell stats/pixel.rs DOT + GAP). Pages mark elements with
 * data-* attributes; render() fills them on load. */
(() => {
  // 5×7: the readout face. Rows top → bottom; width is the row length.
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
    A: [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
    B: ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
    C: [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
    D: ["###..", "#..#.", "#...#", "#...#", "#...#", "#..#.", "###.."],
    E: ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
    F: ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
    G: [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
    H: ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
    I: [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
    J: ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."],
    K: ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
    L: ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
    M: ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
    N: ["#...#", "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#"],
    O: [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
    P: ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
    Q: [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
    R: ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
    S: [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
    T: ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
    U: ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
    V: ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
    W: ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#."],
    X: ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
    Y: ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
    Z: ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
    ":": [".", ".", "#", ".", "#", ".", "."],
    ".": [".", ".", ".", ".", ".", ".", "#"],
    "·": [".", ".", ".", "#", ".", ".", "."],
    "%": ["##...", "##..#", "...#.", "..#..", ".#...", "#..##", "...##"],
    "-": ["...", "...", "...", "###", "...", "...", "..."],
    "+": [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."],
    "/": [".....", "....#", "...#.", "..#..", ".#...", "#....", "....."],
    "×": [".....", ".....", "#...#", ".#.#.", "..#..", ".#.#.", "#...#"],
    "↑": ["..#..", ".###.", "#.#.#", "..#..", "..#..", "..#..", "..#.."],
    "↓": ["..#..", "..#..", "..#..", "..#..", "#.#.#", ".###.", "..#.."],
    " ": ["..", "..", "..", "..", "..", "..", ".."],
  };

  // 3×5: the micro face. Five rows: the height of every menubar meter.
  const F35 = {
    "0": ["###", "#.#", "#.#", "#.#", "###"],
    "1": [".#.", "##.", ".#.", ".#.", "###"],
    "2": ["###", "..#", "###", "#..", "###"],
    "3": ["###", "..#", ".##", "..#", "###"],
    "4": ["#.#", "#.#", "###", "..#", "..#"],
    "5": ["###", "#..", "###", "..#", "###"],
    "6": ["###", "#..", "###", "#.#", "###"],
    "7": ["###", "..#", "..#", "..#", "..#"],
    "8": ["###", "#.#", "###", "#.#", "###"],
    "9": ["###", "#.#", "###", "..#", "###"],
    A: [".#.", "#.#", "###", "#.#", "#.#"],
    B: ["##.", "#.#", "##.", "#.#", "##."],
    C: [".##", "#..", "#..", "#..", ".##"],
    D: ["##.", "#.#", "#.#", "#.#", "##."],
    E: ["###", "#..", "##.", "#..", "###"],
    F: ["###", "#..", "##.", "#..", "#.."],
    G: [".##", "#..", "#.#", "#.#", ".##"],
    H: ["#.#", "#.#", "###", "#.#", "#.#"],
    I: ["###", ".#.", ".#.", ".#.", "###"],
    J: ["..#", "..#", "..#", "#.#", ".#."],
    K: ["#.#", "#.#", "##.", "#.#", "#.#"],
    L: ["#..", "#..", "#..", "#..", "###"],
    M: ["#.#", "###", "###", "#.#", "#.#"],
    N: ["##.", "#.#", "#.#", "#.#", "#.#"],
    O: [".#.", "#.#", "#.#", "#.#", ".#."],
    P: ["##.", "#.#", "##.", "#..", "#.."],
    Q: [".#.", "#.#", "#.#", "###", ".##"],
    R: ["##.", "#.#", "##.", "#.#", "#.#"],
    S: [".##", "#..", ".#.", "..#", "##."],
    T: ["###", ".#.", ".#.", ".#.", ".#."],
    U: ["#.#", "#.#", "#.#", "#.#", "###"],
    V: ["#.#", "#.#", "#.#", "#.#", ".#."],
    W: ["#.#", "#.#", "###", "###", "#.#"],
    X: ["#.#", "#.#", ".#.", "#.#", "#.#"],
    Y: ["#.#", "#.#", ".#.", ".#.", ".#."],
    Z: ["###", "..#", ".#.", "#..", "###"],
    ":": [".", "#", ".", "#", "."],
    ".": [".", ".", ".", ".", "#"],
    "%": ["#.#", "..#", ".#.", "#..", "#.#"],
    "-": ["...", "...", "###", "...", "..."],
    "+": ["...", ".#.", "###", ".#.", "..."],
    "/": ["..#", "..#", ".#.", "#..", "#.."],
    "×": ["...", "#.#", ".#.", "#.#", "..."],
    "↑": [".#.", "###", ".#.", ".#.", ".#."],
    "↓": [".#.", ".#.", ".#.", "###", ".#."],
    " ": ["..", "..", "..", "..", ".."],
  };

  const FONTS = { "57": F57, "35": F35 };

  /** Text → bit rows (1 = lit) with one dark column between glyphs. */
  function bits(text, font) {
    const map = FONTS[font] || F57;
    const h = font === "35" ? 5 : 7;
    const rows = Array.from({ length: h }, () => []);
    [...String(text).toUpperCase()].forEach((ch, i) => {
      const g = map[ch] || map[" "];
      if (i > 0) rows.forEach((r) => r.push(0));
      g.forEach((line, y) => {
        for (const c of line) rows[y].push(c === "#" ? 1 : 0);
      });
    });
    return rows;
  }

  /** Pad a bit matrix to `cols` (align left / right / center) and add `pad` dark cells around. */
  function frame(rows, { cols = 0, align = "left", pad = 0, padY = pad } = {}) {
    let w = rows[0].length;
    if (cols > w) {
      const extra = cols - w;
      const l = align === "right" ? extra : align === "center" ? Math.floor(extra / 2) : 0;
      rows = rows.map((r) => [...Array(l).fill(0), ...r, ...Array(extra - l).fill(0)]);
      w = cols;
    }
    if (pad || padY) {
      const blank = () => Array(w + pad * 2).fill(0);
      rows = [
        ...Array.from({ length: padY }, blank),
        ...rows.map((r) => [...Array(pad).fill(0), ...r, ...Array(pad).fill(0)]),
        ...Array.from({ length: padY }, blank),
      ];
    }
    return rows;
  }

  /** Bit matrix → SVG. `cell(x, y, v)` may return a color for a lit cell. */
  function svg(rows, { dot = 2, pitch = 3, field = true, round = 0, cell } = {}) {
    const h = rows.length;
    const w = rows[0].length;
    const W = w * pitch - (pitch - dot);
    const H = h * pitch - (pitch - dot);
    let off = "";
    let on = "";
    const r = round ? ` rx="${round}"` : "";
    rows.forEach((row, y) =>
      row.forEach((v, x) => {
        const at = `x="${x * pitch}" y="${y * pitch}" width="${dot}" height="${dot}"${r}`;
        if (v) {
          const c = cell && cell(x, y, v);
          on += c ? `<rect ${at} style="fill:${c}"/>` : `<rect ${at}/>`;
        } else if (field) off += `<rect ${at}/>`;
      }),
    );
    return (
      `<svg class="dots" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">` +
      `<g class="dots__off">${off}</g><g class="dots__on">${on}</g></svg>`
    );
  }

  /** <span data-dots="14:32" data-font="35" data-cols="17" data-align="right" data-pad="0"> */
  function renderDots(el) {
    const d = el.dataset;
    const font = d.font || "57";
    let rows = bits(d.dots, font);
    rows = frame(rows, {
      cols: +d.cols || 0,
      align: d.align || "left",
      pad: +d.pad || 0,
      padY: d.padY !== undefined ? +d.padY : +d.pad || 0,
    });
    el.innerHTML = svg(rows, {
      dot: +d.dot || 2,
      pitch: +d.pitch || 3,
      field: d.field !== "0",
      round: +d.round || 0,
    });
  }

  // ---- Meters --------------------------------------------------------------

  const BAYER = [
    [0.125, 0.625],
    [0.875, 0.375],
  ];

  /** One LED cell = 2×2 sub-squares (pixel.rs cell_pixel_on dithers partial cells). */
  function meterSvg(cols, rowsN, fill, color, offColor, dot = 2, pitch = 3) {
    // fill(x, fromBottom) → 0..1, color(x, fromBottom) → css color
    const W = cols * pitch - (pitch - dot);
    const H = rowsN * pitch - (pitch - dot);
    const q = dot / 2;
    let s = "";
    for (let x = 0; x < cols; x++) {
      for (let fb = 0; fb < rowsN; fb++) {
        const y = (rowsN - 1 - fb) * pitch;
        const f = fill(x, fb);
        const c = color(x, fb);
        if (f <= 0 || f >= 1) {
          s += `<rect x="${x * pitch}" y="${y}" width="${dot}" height="${dot}" style="fill:${f >= 1 ? c : offColor}"/>`;
          continue;
        }
        for (let dy = 0; dy < 2; dy++)
          for (let dx = 0; dx < 2; dx++) {
            const lit = f > BAYER[dy][dx];
            s += `<rect x="${x * pitch + dx * q}" y="${y + dy * q}" width="${q}" height="${q}" style="fill:${lit ? c : offColor}"/>`;
          }
      }
    }
    return `<svg class="meter" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">${s}</svg>`;
  }

  const levelColor = (v) => (v >= 90 ? "var(--lamp-live)" : v >= 75 ? "var(--lamp-look)" : "var(--fn-value)");

  /** <span data-graph="12 20 31 …" data-tint="level|value|signal|focus" data-cols data-rows data-dot data-pitch> — stats graph, 0–100. */
  function renderGraph(el) {
    const d = el.dataset;
    const vals = d.graph.trim().split(/\s+/).map(Number);
    const tint = d.tint || "level";
    const max = +d.max || 100;
    const cols = +d.cols || 12;
    const rowsN = +d.rows || 5;
    const v = Array(Math.max(0, cols - vals.length)).fill(0).concat(vals.slice(-cols));
    const fill = (x, fb) => Math.min(1, Math.max(0, (v[x] / max) * rowsN - fb));
    const color = (x) => (tint === "level" ? levelColor(v[x]) : `var(--fn-${tint})`);
    el.innerHTML = meterSvg(cols, rowsN, fill, color, "var(--dot-off)", +d.dot || 2, +d.pitch || 3);
  }

  /** <span data-spectrum="0.2 0.5 …" data-peaks="…"> — 12 bars × 3 LED columns, 2px band gutter. */
  function renderSpectrum(el) {
    const bands = el.dataset.spectrum.trim().split(/\s+/).map(Number);
    const peaks = (el.dataset.peaks || "").trim().split(/\s+/).filter(Boolean).map(Number);
    const muted = el.dataset.muted === "1";
    const lit = muted ? "var(--dot-muted)" : "var(--fn-signal)";
    const BAR = 8; // 3 dots × 2 + 2 gaps
    const W = bands.length * BAR + (bands.length - 1) * 2;
    const H = 14;
    let s = "";
    bands.forEach((b, i) => {
      const x0 = i * (BAR + 2);
      const peakRow = peaks.length ? Math.min(4, Math.round(peaks[i] * 5) - 1) : -1;
      for (let fb = 0; fb < 5; fb++) {
        const y = (4 - fb) * 3;
        const f = Math.min(1, Math.max(0, b * 5 - fb));
        const isPeak = fb === peakRow && f < 1;
        for (let c = 0; c < 3; c++)
          for (let dy = 0; dy < 2; dy++)
            for (let dx = 0; dx < 2; dx++) {
              const on = f >= 1 || (f > 0 && f > BAYER[dy][dx]);
              const col = on ? lit : isPeak ? "var(--fn-value)" : "var(--dot-off)";
              s += `<rect x="${x0 + c * 3 + dx}" y="${y + dy}" width="1" height="1" style="fill:${col}"/>`;
            }
      }
    });
    el.innerHTML = `<svg class="meter" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">${s}</svg>`;
  }

  /** <span data-bar="0.62" data-cells="20" data-tint="value"> — a horizontal LED bar, one row by default. */
  function renderBar(el) {
    const d = el.dataset;
    const v = +d.bar;
    const n = +d.cells || 20;
    const rowsN = +d.rows || 1;
    const dot = +d.dot || 2;
    const pitch = +d.pitch || 3;
    const tint = d.tint || "value";
    const q = dot / 2;
    const color = (x) => (tint === "level" ? levelColor(((x + 1) / n) * 100) : `var(--fn-${tint})`);
    const W = n * pitch - (pitch - dot);
    const H = rowsN * pitch - (pitch - dot);
    let s = "";
    for (let x = 0; x < n; x++) {
      const f = Math.min(1, Math.max(0, v * n - x));
      for (let r = 0; r < rowsN; r++) {
        if (f <= 0 || f >= 1) {
          s += `<rect x="${x * pitch}" y="${r * pitch}" width="${dot}" height="${dot}" style="fill:${f >= 1 ? color(x) : "var(--dot-off)"}"/>`;
          continue;
        }
        for (let dy = 0; dy < 2; dy++)
          for (let dx = 0; dx < 2; dx++)
            s += `<rect x="${x * pitch + dx * q}" y="${r * pitch + dy * q}" width="${q}" height="${q}" style="fill:${f > BAYER[dy][dx] ? color(x) : "var(--dot-off)"}"/>`;
      }
    }
    el.innerHTML = `<svg class="meter" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" shape-rendering="crispEdges">${s}</svg>`;
  }

  // ---- The flower ------------------------------------------------------------

  const FLOWER =
    "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.300-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.600-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.900-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";

  /** The vector mark (system menu size and up). */
  function flowerSvg(size, color = "var(--fn-system)") {
    return `<svg class="flower" width="${size}" height="${size}" viewBox="-25 -32 561 561"><path d="${FLOWER}" style="fill:${color}"/></svg>`;
  }

  /** The mark rastered onto an n×n LED grid (alpha ≥ threshold lights a cell). */
  function flowerBits(n, threshold = 0.42) {
    const S = 8; // supersample
    const c = document.createElement("canvas");
    c.width = c.height = n * S;
    const g = c.getContext("2d");
    const k = (n * S) / 561;
    g.setTransform(k, 0, 0, k, 25 * k, 32 * k);
    g.fill(new Path2D(FLOWER));
    const px = g.getImageData(0, 0, n * S, n * S).data;
    const rows = [];
    for (let y = 0; y < n; y++) {
      const r = [];
      for (let x = 0; x < n; x++) {
        let a = 0;
        for (let sy = 0; sy < S; sy++)
          for (let sx = 0; sx < S; sx++) a += px[((y * S + sy) * n * S + (x * S + sx)) * 4 + 3];
        r.push(a / (S * S * 255) >= threshold ? 1 : 0);
      }
      rows.push(r);
    }
    return rows;
  }

  /** Drop empty rows and columns around a bit matrix. */
  function trim(rows) {
    const lit = (r) => r.some(Boolean);
    let top = rows.findIndex(lit);
    let bottom = rows.length - 1 - [...rows].reverse().findIndex(lit);
    rows = rows.slice(top, bottom + 1);
    const w = rows[0].length;
    let l = 0, r = w - 1;
    while (l < w && rows.every((row) => !row[l])) l++;
    while (r > l && rows.every((row) => !row[r])) r--;
    return rows.map((row) => row.slice(l, r + 1));
  }

  // ---- Icons (lucide, 24 grid, stroke) ---------------------------------------

  const ICON = {
    mail: '<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
    bell: '<path d="M10.268 21a2 2 0 0 0 3.464 0"/><path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"/>',
    bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
    terminal: '<path d="m7 11 2-2-2-2"/><path d="M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2" ry="2"/>',
    globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
    calendar: '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>',
    settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
    search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
    calculator: '<rect width="16" height="20" x="4" y="2" rx="2"/><line x1="8" x2="16" y1="6" y2="6"/><line x1="16" x2="16" y1="14" y2="18"/><path d="M16 10h.01"/><path d="M12 10h.01"/><path d="M8 10h.01"/><path d="M12 14h.01"/><path d="M8 14h.01"/><path d="M12 18h.01"/><path d="M8 18h.01"/>',
    gamepad: '<line x1="6" x2="10" y1="11" y2="11"/><line x1="8" x2="8" y1="9" y2="13"/><line x1="15" x2="15.01" y1="12" y2="12"/><line x1="18" x2="18.01" y1="10" y2="10"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z"/>',
    scope: '<path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3"/><path d="m16 16-1.9-1.9"/>',
    capture: '<path d="M5 3a2 2 0 0 0-2 2"/><path d="M19 3a2 2 0 0 1 2 2"/><path d="M21 19a2 2 0 0 1-2 2"/><path d="M5 21a2 2 0 0 1-2-2"/><path d="M9 3h1"/><path d="M9 21h1"/><path d="M14 3h1"/><path d="M14 21h1"/><path d="M3 9v1"/><path d="M21 9v1"/><path d="M3 14v1"/><path d="M21 14v1"/>',
    bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>',
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
    palette: '<circle cx="13.5" cy="6.5" r=".5"/><circle cx="17.5" cy="10.5" r=".5"/><circle cx="8.5" cy="7.5" r=".5"/><circle cx="6.5" cy="12.5" r=".5"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z"/>',
    frame: '<line x1="22" x2="2" y1="6" y2="6"/><line x1="22" x2="2" y1="18" y2="18"/><line x1="6" x2="6" y1="2" y2="22"/><line x1="18" x2="18" y1="2" y2="22"/>',
    mic: '<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" x2="12" y1="19" y2="22"/>',
    camera: '<path d="m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5"/><rect x="2" y="6" width="14" height="12" rx="2"/>',
    check: '<path d="M20 6 9 17l-5-5"/>',
    plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
    chevron: '<path d="m9 18 6-6-6-6"/>',
  };

  function renderIcon(el) {
    const name = el.dataset.icon;
    const sw = el.dataset.stroke || 1.6;
    el.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${ICON[name] || ""}</svg>`;
  }

  // ---- Zones (sola-bus topics.rs Zone::rect) ---------------------------------

  const ZONES = {
    left: [0, 0, 0.28, 1],
    right: [0.72, 0, 0.28, 1],
    top: [0, 0, 1, 0.7],
    bottom: [0, 0.7, 1, 0.3],
    "top-middle": [0.28, 0, 0.44, 0.7],
    "bottom-middle": [0.28, 0.7, 0.44, 0.3],
    "full-middle": [0.28, 0, 0.44, 1],
    "middle-right": [0.28, 0, 0.72, 1],
    fullscreen: [0, 0, 1, 1],
    cinema: [0, 0, 1, 1],
    float: [0.36, 0.24, 0.28, 0.42],
  };

  /** <span data-zone="left"> — the desk at 64:27 (5120×2160) with the zone lit. */
  function renderZone(el) {
    const z = ZONES[el.dataset.zone];
    const W = +el.dataset.w || 24;
    const H = Math.round((W * 2160) / 5120);
    const [x, y, w, h] = z;
    const pad = 0.5;
    const isFloat = el.dataset.zone === "float";
    const isCinema = el.dataset.zone === "cinema";
    const bar = isCinema ? "" : `<rect x="${pad}" y="${pad}" width="${W - 1}" height="1" class="zone__bar"/>`;
    el.innerHTML =
      `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">` +
      `<rect x="${pad}" y="${pad}" width="${W - 1}" height="${H - 1}" rx="1" class="zone__desk"/>` +
      `<rect x="${pad + x * (W - 1)}" y="${pad + y * (H - 1)}" width="${w * (W - 1)}" height="${h * (H - 1)}" rx="${isFloat ? 1 : 0.5}" class="zone__lit"/>` +
      bar +
      `</svg>`;
  }

  function render(root = document) {
    root.querySelectorAll("[data-dots]").forEach(renderDots);
    root.querySelectorAll("[data-graph]").forEach(renderGraph);
    root.querySelectorAll("[data-spectrum]").forEach(renderSpectrum);
    root.querySelectorAll("[data-bar]").forEach(renderBar);
    root.querySelectorAll("[data-icon]").forEach(renderIcon);
    root.querySelectorAll("[data-zone]").forEach(renderZone);
    root.querySelectorAll("[data-flower]").forEach((el) => {
      el.innerHTML = flowerSvg(+el.dataset.flower || 14, el.dataset.color || "var(--fn-system)");
    });
  }

  window.Instrument = { F57, F35, bits, frame, svg, flowerBits, trim, flowerSvg, render, ICON, ZONES };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => render());
  else render();
})();
