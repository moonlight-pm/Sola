// Solar boards: shared drawing helpers and the light model.
// Tokens live in solar.css; this file reads them back for fades.

const FLOWER = "M242.641.561c-21.9 3.5-41.8 14-55.2 29.3-16.4 18.7-24.5 41.6-23.3 65.7 1.1 21 7.8 38.3 23.5 60.5 15.8 22.3 44.6 62.2 49 67.7 10.1 13 23.8 14.3 34.7 3.3 8.6-8.5 59.4-80.3 65.5-92.6 6.5-12.9 9.1-23.4 9.7-39 .8-19-1.6-30.9-8.8-45.3-12.5-24.6-34.1-41.6-61.1-48.1-7.8-1.9-26.4-2.7-34-1.5m-163.3 118.1c-36.5 5.3-67.3 32.9-76.6 68.7-2.7 10.2-3.5 26.9-1.9 36.9 5.9 36.8 33.6 67.6 69.5 77.3 10.1 2.7 28.9 3.8 37.7 2.1 9.8-1.9 103.9-33.5 111.4-37.4 7.1-3.7 11.4-11.3 11.4-20 0-5.300-.3-5.9-10.3-20.3-14.2-20.4-48.9-67.6-54.9-74.6-20.9-24.3-55-37.2-86.3-32.7m323 .3c-23.8 4.6-45.2 17.8-61 37.6-13 16.4-57.2 78.2-59.6 83.5-1 2.2-1.9 5.4-1.9 7.1 0 7.3 4.7 15.2 11.1 18.4 7.5 3.9 102.9 36.3 111.7 38 9.8 2 28.8.8 39.5-2.4 27.5-8.2 48.8-26.3 60.7-52 12.3-26.3 11.3-58.3-2.6-83.3-12.7-22.8-36.4-40.5-61.600-45.9-10.3-2.2-27.7-2.7-36.3-1m-185 163c-29.3 9.7-90.7 31.6-98.1 35.1-11.5 5.3-19.4 10.7-27.1 18.2-13.9 13.8-23.8 32.3-27.4 51.6-1.6 8.6-1.3 26.7.6 35.6 7.6 35.9 37.3 65.3 73.4 72.6 11.2 2.3 29.4 1.6 40.1-1.5 21-6 41.2-20.4 53.1-37.7 4.5-6.4 10.4-19.6 12.8-28.2 3.6-13.1 4.2-25.8 3.900-81.2l-.3-52.6-3-4.8c-5.8-9.3-14.6-11.5-28-7.1m56.7-.3c-4.7 2.4-8.3 6.6-10.1 11.8-1.8 5.5-1.5 113.4.4 124.2 6.4 36.9 30.8 64.7 66.2 75.6 7.9 2.4 10.3 2.7 24.3 2.7 14.1 0 16.3-.3 24.5-2.8 28-8.6 48.6-26.3 60.5-52 6.1-13.1 7.5-20.6 7.5-39.1 0-17.4-.9-22.5-6.5-35.9-6.7-16-20.2-32.3-34.9-42.1-11.2-7.4-20.6-11-91.1-35.1-29.9-10.2-33.5-10.9-40.8-7.3";

const ICONS = {
  mail: '<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  bluetooth: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  terminal: '<path d="m7 11 2-2-2-2"/><path d="M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  calendar: '<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
  settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2M20 14h2M15 13v2M9 13v2"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  sunrise: '<path d="M12 2v8"/><path d="m4.93 10.93 1.41 1.41"/><path d="M2 18h2"/><path d="M20 18h2"/><path d="m19.07 10.93-1.41 1.41"/><path d="M22 22H2"/><path d="m8 6 4-4 4 4"/><path d="M16 18a4 4 0 0 0-8 0"/>',
  sunset: '<path d="M12 10V2"/><path d="m4.93 10.93 1.41 1.41"/><path d="M2 18h2"/><path d="M20 18h2"/><path d="m19.07 10.93-1.41 1.41"/><path d="M22 22H2"/><path d="m16 6-4 4-4-4"/><path d="M16 18a4 4 0 0 0-8 0"/>',
  moon: '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>',
  left: '<path d="m15 18-6-6 6-6"/>',
  right: '<path d="m9 18 6-6-6-6"/>',
  calculator: '<rect width="16" height="20" x="4" y="2" rx="2"/><path d="M8 6h8M16 14v4M16 10h.01M12 10h.01M8 10h.01M12 14h.01M8 14h.01M12 18h.01M8 18h.01"/>',
  gamepad: '<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258A4 4 0 0 0 17.32 5z"/>',
  scan: '<path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3"/>',
  paperclip: '<path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>',
  plus: '<path d="M5 12h14M12 5v14"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  palette: '<circle cx="13.5" cy="6.5" r=".5"/><circle cx="17.5" cy="10.5" r=".5"/><circle cx="8.5" cy="7.5" r=".5"/><circle cx="6.5" cy="12.5" r=".5"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.93 0 1.65-.75 1.65-1.69 0-.44-.18-.84-.44-1.13-.29-.29-.44-.65-.44-1.13a1.64 1.64 0 0 1 1.67-1.67h2c3.05 0 5.56-2.5 5.56-5.55C21.97 6.01 17.46 2 12 2z"/>',
};

function icon(name, cls = "") {
  return `<svg class="${cls}" viewBox="0 0 24 24">${ICONS[name]}</svg>`;
}

function flower(size = 14, cls = "bar__flower") {
  return `<svg class="${cls}" style="width:${size}px;height:${size}px" viewBox="-25 -32 561 561"><path d="${FLOWER}" fill="var(--sol)"/></svg>`;
}

function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// A 12 × 5 stat graph: lit dots from the bottom.
function graph(seed, base, lit = "var(--led-on)", tint) {
  const r = rng(seed);
  let html = '<div class="leds">';
  for (let c = 0; c < 12; c++) {
    const level = Math.max(0, Math.min(5, Math.round(base + (r() - 0.5) * 3)));
    html += '<div class="leds__col">';
    for (let row = 0; row < 5; row++) {
      const on = row < level;
      const color = on ? (tint && row >= 3 ? tint : lit) : "var(--led-off)";
      html += `<div class="led" style="background:${color};${on ? `box-shadow:var(--led-fx) ${color}` : ""}"></div>`;
    }
    html += "</div>";
  }
  return html + "</div>";
}

// 12 spectrum bands, 3 dots wide, phosphor rows bottom to top.
function spectrum(seed) {
  const r = rng(seed);
  const shape = [3, 4, 5, 4, 3, 3, 4, 3, 2, 2, 3, 1];
  let html = '<div class="leds" style="gap:2px">';
  for (let b = 0; b < 12; b++) {
    const level = Math.max(0, Math.min(5, shape[b] + Math.round((r() - 0.6) * 2)));
    html += '<div style="display:flex;gap:1px">';
    for (let w = 0; w < 3; w++) {
      html += '<div class="leds__col">';
      for (let row = 0; row < 5; row++) {
        const on = row < level;
        const color = on ? `var(--led-${row + 1})` : "var(--led-off)";
        html += `<div class="led" style="background:${color};${on ? `box-shadow:var(--led-fx) ${color}` : ""}"></div>`;
      }
      html += "</div>";
    }
    html += "</div>";
  }
  return html + "</div>";
}

// The solar line: kind = sun | rise | set | moon, x = 0..1 across the bar.
function sky(kind, x) {
  const pct = (x * 100).toFixed(2) + "%";
  if (kind === "moon") return `<div class="horizon"></div><div class="moon" style="left:${pct}"></div>`;
  const cls = kind === "sun" ? "sun" : "sun sun--below";
  return `<div class="horizon"></div><div class="${cls}" style="left:${pct}"><div class="sun__bloom"></div><div class="sun__core"></div><div class="sun__disc"></div></div>`;
}

function bar(o) {
  const menus = (o.menus || ["File", "Edit", "View", "Window"])
    .map((m, i) => `<div class="bar__item${o.open === i ? " bar__item--open" : ""}">${m}</div>`).join("");
  const left = `<div class="bar__left"><div class="bar__item">${flower()}</div><div class="bar__item bar__item--app">${o.app}</div>${menus}</div>`;
  const full = o.compact ? "" : `
    <div class="bar__item" style="padding:0 6px">${spectrum(o.seed || 7)}</div>
    <div class="bar__stat">CPU ${graph((o.seed || 7) + 1, 1.6)}</div>
    <div class="bar__stat">GPU ${graph((o.seed || 7) + 2, 0.8)}</div>
    <div class="bar__stat">MEM ${graph((o.seed || 7) + 3, 3.4, "var(--led-on)", "var(--warn)")}</div>
    <div class="bar__stat" style="margin-left:6px">RX ${graph((o.seed || 7) + 4, 1.2, "var(--led-2)")}</div>
    <div class="bar__stat">TX ${graph((o.seed || 7) + 5, 0.7, "var(--led-1)")}</div>`;
  const right = `<div class="bar__right">
    <div class="bar__extras">${icon("mail", "bar__icon" + (o.unread === false ? "" : " bar__icon--lit"))}${icon("bell", "bar__icon")}${icon("bluetooth", "bar__icon")}</div>
    ${full}
    <div class="bar__clock${o.clockOpen ? " bar__item--open" : ""}">${o.clock || ""}</div></div>`;
  return left + right + sky(o.sky || "sun", o.x ?? 0.4);
}

// ── Sun times (NOAA approximation), local hours ────────────────────────
const PLACE = { name: "Denver", lat: 39.7392, lon: -104.9842 };

function sunEvent(date, zenith, rising, place = PLACE) {
  const N = (Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) - Date.UTC(date.getFullYear(), 0, 0)) / 86400000;
  const rad = Math.PI / 180;
  const lngHour = place.lon / 15;
  const t = N + ((rising ? 6 : 18) - lngHour) / 24;
  const M = 0.9856 * t - 3.289;
  let L = (M + 1.916 * Math.sin(M * rad) + 0.02 * Math.sin(2 * M * rad) + 282.634) % 360;
  if (L < 0) L += 360;
  let RA = (Math.atan(0.91764 * Math.tan(L * rad)) / rad + 360) % 360;
  RA = (RA + Math.floor(L / 90) * 90 - Math.floor(RA / 90) * 90) / 15;
  const sinDec = 0.39782 * Math.sin(L * rad);
  const cosDec = Math.cos(Math.asin(sinDec));
  const cosH = (Math.cos(zenith * rad) - sinDec * Math.sin(place.lat * rad)) / (cosDec * Math.cos(place.lat * rad));
  let H = Math.acos(cosH) / rad;
  if (rising) H = 360 - H;
  H /= 15;
  const T = H + RA - 0.06571 * t - 6.622;
  const UT = (((T - lngHour) % 24) + 24) % 24;
  const offset = -date.getTimezoneOffset() / 60;
  return (((UT + offset) % 24) + 24) % 24;
}

function sunDay(date) {
  return {
    civilDawn: sunEvent(date, 96, true),
    sunrise: sunEvent(date, 90.833, true),
    sunset: sunEvent(date, 90.833, false),
    civilDusk: sunEvent(date, 96, false),
  };
}

const FADE = 40 / 60;

// The light at hour h: { from, to, t } — a hold has from === to.
function lightAt(h, s) {
  const bands = [
    [s.civilDawn - FADE, s.civilDawn, "night", "dawn"],
    [s.civilDawn, s.sunrise, "dawn", "dawn"],
    [s.sunrise, s.sunrise + FADE, "dawn", "day"],
    [s.sunrise + FADE, s.sunset - FADE, "day", "day"],
    [s.sunset - FADE, s.sunset, "day", "dusk"],
    [s.sunset, s.civilDusk, "dusk", "dusk"],
    [s.civilDusk, s.civilDusk + FADE, "dusk", "night"],
  ];
  for (const [a, b, from, to] of bands) {
    if (h >= a && h < b) return { from, to, t: from === to ? 0 : (h - a) / (b - a) };
  }
  return { from: "night", to: "night", t: 0 };
}

function hhmm(h) {
  const m = Math.round(h * 60);
  return `${String(Math.floor(m / 60) % 24).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

// ── OKLCH blending of state tokens ─────────────────────────────────────
const TOKENS = ["bar", "bg", "chrome", "raised", "hover", "selection", "border", "hairline", "hairline-strong",
  "well", "well-edge", "text", "text-2", "text-3", "signal", "signal-soft", "signal-edge", "on-signal",
  "ok", "warn", "bad", "horizon", "sun-bloom", "moon", "led-off", "led-on", "led-1", "led-2", "led-3", "led-4", "led-5", "term-bg"];
const INK = ["text", "text-2", "text-3", "on-signal", "led-on", "led-off", "led-1", "led-2", "led-3", "led-4", "led-5"];

function readState(name) {
  const probe = document.createElement("div");
  probe.className = "l-" + name;
  probe.style.display = "none";
  document.body.appendChild(probe);
  const cs = getComputedStyle(probe);
  const out = {};
  for (const k of TOKENS) out[k] = cs.getPropertyValue("--" + k).trim();
  probe.remove();
  return out;
}

function hexToRgba(hex) {
  const h = hex.replace("#", "");
  const n = (i) => parseInt(h.slice(i, i + 2), 16) / 255;
  return [n(0), n(2), n(4), h.length >= 8 ? n(6) : 1];
}
const toLin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toSrgb = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

function toOklch([r, g, b, a]) {
  [r, g, b] = [r, g, b].map(toLin);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(A, B), Math.atan2(B, A), a];
}

function fromOklch([L, C, H, a]) {
  const A = C * Math.cos(H), B = C * Math.sin(H);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  const rgb = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((c) => Math.round(Math.min(1, Math.max(0, toSrgb(c))) * 255));
  const al = Math.round(a * 255);
  return "#" + [...rgb, al].map((v) => v.toString(16).padStart(2, "0")).join("");
}

function mixOklch(x, y, t, tL = t) {
  const a = toOklch(hexToRgba(x)), b = toOklch(hexToRgba(y));
  let ha = a[2], hb = b[2];
  if (a[1] < 0.02) ha = hb;
  if (b[1] < 0.02) hb = ha;
  let dh = hb - ha;
  if (dh > Math.PI) dh -= 2 * Math.PI;
  if (dh < -Math.PI) dh += 2 * Math.PI;
  return fromOklch([a[0] + (b[0] - a[0]) * tL, a[1] + (b[1] - a[1]) * t, ha + dh * t, a[3] + (b[3] - a[3]) * t]);
}

function luminance(hex) {
  const [r, g, b] = hexToRgba(hex).map(toLin);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrast(x, y) {
  const a = luminance(x), b = luminance(y);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// Surfaces blend; ink is chosen. Text takes whichever side reads better
// on the blended background, so it flips once instead of passing gray.
// Lightness that crosses polarity eases on a steep curve, so the desk
// spends minutes near mid-gray, not the middle of the fade.
function easeL(t, steep) {
  if (!steep) return t;
  const k = 12, f = (x) => 1 / (1 + Math.exp(-k * (x - 0.5)));
  return (f(t) - f(0)) / (f(1) - f(0));
}

const STATES = {};
function state(name) { return (STATES[name] ||= readState(name)); }

function blend(fromName, toName, t) {
  const A = state(fromName), B = state(toName);
  const steep = (luminance(A.bg) > 0.18) !== (luminance(B.bg) > 0.18);
  const tL = easeL(t, steep);
  const out = {};
  for (const k of TOKENS) out[k] = mixOklch(A[k], B[k], t, tL);
  const side = contrast(A.text, out.bg) >= contrast(B.text, out.bg) ? A : B;
  for (const k of INK) out[k] = side[k];
  out.signal = side === A ? mixOklch(A.signal, B.signal, Math.min(t, 0.5)) : mixOklch(A.signal, B.signal, Math.max(t, 0.5));
  out.__side = side === A ? fromName : toName;
  return out;
}

// Gradient stops for a day of light, sampled so fades follow blend().
function dayStops(s, token = "bg") {
  const pct = (h) => ((h / 24) * 100).toFixed(3) + "%";
  const fades = [[s.civilDawn - FADE, "night", "dawn"], [s.sunrise, "dawn", "day"], [s.sunset - FADE, "day", "dusk"], [s.civilDusk, "dusk", "night"]];
  const out = [`${state("night")[token]} 0%`];
  for (const [start, a, b] of fades) {
    for (let i = 0; i <= 16; i++) {
      const t = i / 16;
      out.push(`${blend(a, b, t)[token]} ${pct(start + t * FADE)}`);
    }
  }
  out.push(`${state("night")[token]} 100%`);
  return out.join(", ");
}

function applyVars(el, vars) {
  for (const [k, v] of Object.entries(vars)) if (!k.startsWith("__")) el.style.setProperty("--" + k, v);
}

// ── Mount ──────────────────────────────────────────────────────────────
function mount() {
  for (const el of document.querySelectorAll("[data-bar]")) {
    el.classList.add("bar");
    el.innerHTML = bar(JSON.parse(el.dataset.bar));
  }
  for (const el of document.querySelectorAll("[data-icon]")) {
    el.outerHTML = icon(el.dataset.icon, el.className);
  }
  for (const el of document.querySelectorAll("[data-flower]")) {
    el.outerHTML = flower(+el.dataset.flower, el.className);
  }
  for (const el of document.querySelectorAll("[data-graph]")) {
    const [seed, base] = el.dataset.graph.split(",").map(Number);
    el.innerHTML = graph(seed, base);
  }
  for (const el of document.querySelectorAll("[data-spectrum]")) {
    el.innerHTML = spectrum(+el.dataset.spectrum);
  }
}
document.addEventListener("DOMContentLoaded", () => {
  mount();
  if (window.afterMount) window.afterMount();
});
