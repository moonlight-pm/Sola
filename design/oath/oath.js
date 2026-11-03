// Oath, as a working comp: list, map and tree over the real catalog snapshot
// (catalog.js), with samples.js laid on top and tagged. Query parameters pick
// the state a board opens in:
//   view=list|map|tree  take=strata|weight (map)
//   pkg=<name>  filter=<standing>  open=<sections>  sheet=1 (the sheet alone)  bare=1 (the tree alone)

const C = window.CATALOG, PK = C.pkgs, SMP = window.SAMPLES;
const q = new URLSearchParams(location.search);

// ── Lay the samples over the real data ─────────────────────────
for (const p of Object.values(PK)) p.smp = new Set();
for (const [n, s] of Object.entries(SMP.unpushed)) {
  Object.assign(PK[n], { h: s.h, src: s.src, unpushed: true });
  PK[n].smp.add('here');
}
for (const [n, s] of Object.entries(SMP.updates)) {
  Object.assign(PK[n], { newest: s.newest, newver: s.ver });
  PK[n].smp.add('newest');
}
for (const [n, a] of Object.entries(SMP.advisories)) {
  PK[n].adv = a;
  PK[n].smp.add('adv');
}
for (const r of SMP.remote) {
  PK[r.n] = { ...r, on: false, remote: true, h: '', rel: '', newest: '', comps: [], bins: [], svc: [], m: {}, vers: [], nvers: 0, store: 0, adv: null, rm: true, smp: new Set(['remote']) };
}

// ── The graph ──────────────────────────────────────────────────
const USERS = {};
for (const p of Object.values(PK)) for (const n of p.needs) (USERS[n] ||= []).push(p.n);
const usersOf = id => USERS[id] || [];
const needsOf = id => PK[id]?.needs || [];
function levels(id, next) {
  const out = [], seen = new Set([id]);
  let cur = [id];
  while (cur.length) {
    const nx = [];
    for (const c of cur) for (const x of next(c)) if (!seen.has(x)) { seen.add(x); nx.push(x); }
    if (nx.length) out.push(nx.sort());
    cur = nx;
  }
  return out;
}
const upLv = id => levels(id, usersOf), dnLv = id => levels(id, needsOf);
const closure = (id, next) => new Set(levels(id, next).flat());
const memo = {};
const depthOf = id => (memo[id] ??= needsOf(id).length ? 1 + Math.max(...needsOf(id).map(depthOf)) : 0);
const allUp = id => closure(id, usersOf).size, allDn = id => closure(id, needsOf).size;
const alone = id => !needsOf(id).length && !usersOf(id).length;
const inUse = p => (p.on && !p.remote ? p.sz || 0 : 0);

// ── Standing ───────────────────────────────────────────────────
function standing(p) {
  if (p.remote) return 'remote';
  if (!p.on) return 'off';
  if (p.unpushed) return 'unpushed';
  if (p.overlay) return 'local';
  if (p.newest && p.newest !== p.h) return 'update';
  return 'current';
}
const ST = {
  current: 'Current',
  local: 'Local build',
  unpushed: 'Not pushed',
  update: 'Update',
  off: 'Off',
  remote: 'On origin only',
};
const hasAdv = p => Array.isArray(p.adv) && p.adv.length > 0;
const mk = p => `<span class="mk mk--${standing(p)}"></span>`;
const advMk = p => hasAdv(p) ? `<span class="mk mk--adv"></span>` : '';
const SMPTAG = `<i class="smp">sample</i>`;
const all = Object.values(PK);
const count = f => all.filter(f).length;
const fmtSize = b => b >= 1e9 ? (b / 1e9).toFixed(1) + ' GB' : b >= 1e6 ? Math.round(b / 1e6) + ' MB' : b >= 1e3 ? Math.round(b / 1e3) + ' KB' : b ? b + ' B' : '–';
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const built = p => p.src ? `${p.src.branch || 'detached'}@${p.src.commit}` : '';

// ── State ──────────────────────────────────────────────────────
const S = {
  view: q.get('view') || 'list',
  map: q.get('take') === 'weight' ? 'weight' : 'strata',
  sel: q.get('pkg') === 'none' ? null : (q.get('pkg') || 'sola'),
  filter: q.get('filter') || 'all',
  fams: new Set((q.get('fams') || '').split(',').filter(Boolean)),
  open: new Set((q.get('open') || 'where,tree').split(',')),
  sheet: q.get('sheet') === '1',
  bare: q.get('bare') === '1',
  disk: q.get('size') !== 'live',
  trail: [],
};
const root = document.getElementById('root');
const ic = (d, s = 15) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const I = {
  list: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
  map: '<path d="M2 20h20M5 20v-6h4v6M10 20V8h4v12M15 20v-9h4v9"/>',
  tree: '<path d="M6 3v6M6 15v6M6 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 9c0 3-4 3-9 3"/>',
  caret: '<path d="m9 18 6-6-6-6"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
};

// ── Filters ────────────────────────────────────────────────────
const FILTERS = [
  ['all', 'Everything', () => true, ''],
  ['local', 'Local builds', p => p.overlay, 'local'],
  ['unpushed', 'Not pushed', p => p.unpushed, 'unpushed'],
  ['update', 'Updates', p => p.on && p.newest && p.newest !== p.h, 'update'],
  ['behind', 'Behind upstream', p => p.up, 'none'],
  ['off', 'Off', p => !p.on && !p.remote, 'off'],
  ['remote', 'On origin only', p => p.remote, 'remote'],
  ['adv', 'Advisories', hasAdv, 'adv'],
  ['never', 'Never checked', p => !p.remote && p.adv == null, 'none'],
];

// ── Sidebar ────────────────────────────────────────────────────
function side() {
  const nav = [['list', 'Packages', I.list], ['map', 'Map', I.map], ['tree', 'Tree', I.tree]];
  return `<nav class="side">
    <div class="side__group">${nav.map(([v, l, d]) => `<button class="si ${S.view === v ? 'on' : ''}" data-view="${v}">${ic(d)}<span>${l}</span></button>`).join('')}</div>
    <div class="side__group"><div class="side__cap">Standing</div>
      ${FILTERS.map(([k, l, f, m]) => `<button class="si ${S.view === 'list' && S.filter === k ? 'on' : ''}" data-filter="${k}">${m ? `<span class="mk mk--${m}"></span>` : '<span class="mk mk--current"></span>'}<span>${l}</span><span class="n">${count(f)}</span></button>`).join('')}
    </div>
    <div class="side__foot">Origin ${C.origin}<br>Read ${C.at}</div>
  </nav>`;
}

// ── Packages ───────────────────────────────────────────────────
function hcell(v, here, cls = '') {
  if (v == null) return `<span class="h h--off">not here</span>`;
  if (!v) return `<span class="h h--same">–</span>`;
  if (v === here) return `<span class="h h--same">·</span>`;
  return `<span class="h ${cls}">${v.slice(0, 8)}</span>`;
}
function upcell(p) {
  if (p.up) return `<span class="h">${esc(p.up)}</span>`;
  return `<span class="h h--same">${p.aud ? '·' : '–'}</span>`;
}
function row(p) {
  const st = standing(p);
  const here = p.remote ? `<span class="h h--off">not here</span>` : !p.on ? `<span class="h h--off">off</span>` : `<span class="h ${st === 'local' || st === 'unpushed' ? 'h--warn' : 'h--here'}">${p.h.slice(0, 8)}</span>`;
  const from = st === 'unpushed' ? `<span class="small small--mono" style="color:var(--warning)">${built(p)}${p.src.dirty ? ' dirty' : ''}</span>`
    : p.overlay ? `<span class="small small--mono">${built(p)}</span>`
    : `<span></span>`;
  return `<button class="tr tr--row ${S.sel === p.n ? 'on' : ''}" data-pkg="${p.n}">
    <span class="tr__name">${mk(p)}<span class="t">${p.n}</span>${advMk(p)}${p.smp.size ? SMPTAG : ''}</span>
    <span class="small small--mono faint">${esc(p.ver)}</span>
    ${here}
    ${p.on ? hcell(p.rel, p.h) : '<span class="h h--same">–</span>'}
    ${p.on ? hcell(p.newest, p.h, 'h--acc') : '<span class="h h--same">–</span>'}
    ${upcell(p)}
    ${from}
    <span class="small small--mono num faint">${allDn(p.n)}</span>
    <span class="small small--mono num faint">${allUp(p.n)}</span>
    <span class="small small--mono num faint">${fmtSize(p.sz)}</span>
  </button>`;
}
const led = p => `<span class="led led--${hasAdv(p) ? 'adv' : standing(p)}"></span>`;
const ORDER = { unpushed: 0, update: 1, local: 2, remote: 3, off: 4, current: 5 };
function packages() {
  const groups = [];
  const filter = FILTERS.find(f => f[0] === S.filter);
  if (S.filter === 'all') {
    const look = [
      ['Not pushed', all.filter(p => p.unpushed)],
      ['Advisories', all.filter(hasAdv)],
      ['Updates', all.filter(p => standing(p) === 'update' && !hasAdv(p))],
      ['Local builds, on origin', all.filter(p => standing(p) === 'local')],
    ];
    for (const [t, ps] of look) if (ps.length) groups.push(`<div class="grp"><div class="grp__cap">${t} <b>${ps.length}</b></div>${ps.map(row).join('')}</div>`);
    const fams = C.families.map(f => {
      const ps = all.filter(p => p.fam === f).sort((a, b) => ORDER[standing(a)] - ORDER[standing(b)] || a.n.localeCompare(b.n));
      const open = S.fams.has(f);
      return `<button class="fam" data-fam="${f}"><span class="caret ${open ? 'open' : ''}">${ic(I.caret, 14)}</span><span>${f}</span><span class="fam__n">${ps.length}</span><span class="leds">${ps.map(led).join('')}</span><span class="fam__sz">${fmtSize(ps.reduce((a, p) => a + inUse(p), 0))}</span></button>
      ${open ? `<div style="padding:2px 0 8px">${ps.map(row).join('')}</div>` : ''}`;
    }).join('');
    groups.push(`<div class="grp"><div class="grp__cap">Everything, by family <b>${all.length}</b></div>${fams}</div>`);
  } else {
    const ps = all.filter(filter[2]).sort((a, b) => a.fam.localeCompare(b.fam) || a.n.localeCompare(b.n));
    groups.push(`<div class="grp"><div class="grp__cap">${filter[1]} <b>${ps.length}</b></div>${ps.map(row).join('')}</div>`);
  }
  const moving = count(p => p.on && p.newest && p.newest !== p.h);
  const nLocal = count(p => p.overlay), nUn = count(p => p.unpushed);
  const tally = ['local', 'unpushed', 'update', 'off', 'remote', 'current'].map(k => [k, count(p => standing(p) === k)]).filter(x => x[1]);
  const leds = [...all].sort((a, b) => ORDER[standing(a)] - ORDER[standing(b)] || hasAdv(b) - hasAdv(a));
  return `<main class="main pane">
    <div id="summary">
      <div class="sum__where">novus · ${C.profile} profile</div>
      <div class="sum__line"><span class="q">Runs release</span> <span class="mono" style="font-size:20px">${C.release}</span> <span class="q">with</span> <span class="w">${nLocal} local builds</span><span class="q">, and</span> <span class="w">${nUn} is not pushed</span>. <span class="a">Release <span class="mono" style="font-size:20px">${C.newest}</span> is out and moves ${moving} packages</span><span class="q">.</span></div>
    </div>
    <div id="leds"><div class="leds">${leds.map(led).join('')}</div>
      <div class="tally">${tally.map(([k, n]) => `<span><span class="mk mk--${k}"></span>${ST[k]} <b>${n}</b></span>`).join('')}<span><span class="mk mk--adv"></span>Advisories <b>${count(hasAdv)}</b></span></div>
    </div>
    <div class="tools"><div class="field">${ic(I.search, 14)}<span>Filter by name, program, version, need</span></div><span class="tools__hint"><span class="mono">·</span> means no different from novus, <span class="mono">–</span> means nothing to compare</span></div>
    <div class="tbl" id="table">
      <div class="tr tr--head"><span>Package</span><span>Version</span><span>novus</span><span>Release</span><span>Newest</span><span id="col-upstream">Upstream</span><span>Built from</span><span class="num">Needs</span><span class="num">Used by</span><span class="num">Size</span></div>
      ${groups.join('')}
    </div>
  </main>`;
}

// ── The sheet ──────────────────────────────────────────────────
function verdict(p) {
  const st = standing(p);
  if (st === 'unpushed') return ['warn', `A local build from <b>${built(p)}</b>, and that commit is not on origin. The tree had uncommitted changes too. An update replaces this build, and it is the only installed copy of that work.`];
  if (hasAdv(p)) return ['adv', `${p.adv.length} advisor${p.adv.length > 1 ? 'ies' : 'y'} on this build, reaching ${allUp(p.n)} packages above it. The newest release moves it to ${esc(p.newver)}, which fixes ${p.adv.length > 1 ? 'them' : 'it'}.`];
  if (st === 'local') return ['', `A local build from <b>${built(p)}</b>, which is on origin. An update replaces it with the release's build, and nothing is lost.`];
  if (st === 'update') return ['', `The release this machine runs pins ${p.rel.slice(0, 8)}. The newest release moves it to ${p.newest.slice(0, 8)}.`];
  if (st === 'off') return ['', `In the catalog but not linked into /bin. ${p.nvers ? 'Its tree stays on disk, so turning it on needs no network.' : 'No tree on disk yet: it builds or fetches first.'}`];
  if (st === 'remote') return ['', `On the origin, and not in this machine's catalog.`];
  return ['', `The release's build, and no newer release is out.${p.up ? ` Upstream is at ${esc(p.up)}.` : ''}`];
}
function places(p) {
  const srcHere = !p.on ? '' : p.src ? built(p) : 'release';
  const sola = p.fam === 'Sola' && p.n !== 'sola-build';
  const rows = [
    ['<b>novus</b> <i>here</i>', p.on ? p.h : null, srcHere, standing(p) === 'local' || p.unpushed ? 'warn' : ''],
    ['rule'],
    [`Release <i>${C.release.slice(0, 6)}</i>`, p.rel, sola ? 'master@7ccee214' : '', ''],
    [`Newest <i>${C.newest.slice(0, 6)}</i>`, p.newest, sola ? 'master@d6343e8b' : p.smp.has('newest') ? SMPTAG : '', 'acc'],
  ];
  const cell = ([name, v, from, tone]) => {
    if (name === 'rule') return `<div class="pl__rule"></div>`;
    let val;
    if (v === null) val = `<span class="pl__h pl__h--same">not linked</span>`;
    else if (p.on && !name.startsWith('<b>') && v === p.h) val = `<span class="pl__h pl__h--same">same as novus</span>`;
    else val = `<span class="pl__h ${tone && p.on ? 'pl__h--' + tone : ''}">${esc(v)}</span>`;
    return `<div class="pl__name">${name}</div><div>${val}</div><div class="pl__from">${from || ''}</div>`;
  };
  const upstream = p.remote ? '' : `<div class="pl__rule"></div><div class="pl__name">Upstream</div><div><span class="pl__h ${p.up ? '' : 'pl__h--same'}">${p.up ? esc(p.up) : p.aud ? 'nothing newer known' : 'not audited'}</span></div><div class="pl__from">${p.aud ? `audit ${C.audit.at}` : ''}</div>`;
  return `<div class="places">${rows.map(cell).join('')}${upstream}</div>
    <div class="note-line">${p.nvers ? `On disk: ${p.nvers} version${p.nvers > 1 ? 's' : ''}, ${fmtSize(p.store)}.` : 'Nothing on disk.'} ${p.ok ? `Verified ${p.ok.ok ? 'good' : 'damaged'} ${String(p.ok.at).slice(0, 10)}.` : ''}</div>`;
}
function miniTree(p) {
  const chip = n => `<button class="chip" data-pkg="${n}">${PK[n] ? mk(PK[n]) : ''}${n}</button>`;
  const lv = (tag, ns) => {
    const shown = ns.slice(0, 18), more = ns.length - shown.length;
    return `<div class="mini__lv"><div class="mini__tag">${tag}</div><div class="chips">${shown.map(chip).join('')}${more > 0 ? `<span class="chip chip--plain">+${more}</span>` : ''}</div></div>`;
  };
  const up = upLv(p.n), dn = dnLv(p.n);
  const folded = (ls, tag) => ls.length > 2 ? [...ls.slice(0, 1), ls.slice(1).flat()] : ls;
  const upF = folded(up), dnF = folded(dn);
  return `<div class="mini">
    ${[...upF].reverse().map((ns, i) => lv(upF.length - i === 1 ? 'direct' : up.length > 2 ? 'further' : '2 up', ns)).join('')}
    ${up.length ? '' : `<div class="mini__lv"><div class="mini__tag"></div><div class="note-line" style="line-height:22px">Nothing needs it.</div></div>`}
    <div class="mini__self"><div class="mini__tag"></div><div class="bar">${mk(p)}${p.n}</div></div>
    ${dnF.map((ns, i) => lv(i === 0 ? 'direct' : dn.length > 2 ? 'further' : '2 down', ns)).join('')}
    ${dn.length ? '' : `<div class="mini__lv"><div class="mini__tag"></div><div class="note-line" style="line-height:22px">Needs nothing.</div></div>`}
  </div>`;
}
function sec(id, title, sub, body, act = '') {
  const open = S.open.has(id);
  return `<section class="sec" id="sec-${id}">
    <button class="sec__head" data-sec="${id}"><span class="caret ${open ? 'open' : ''}">${ic(I.caret, 14)}</span>${title}<span class="sub">${sub}</span>${act}</button>
    ${open ? `<div class="sec__body">${body}</div>` : ''}
  </section>`;
}
function security(p) {
  const ad = p.adv;
  let head;
  if (hasAdv(p)) head = ad.map(a => `<div class="adv">
      <div class="adv__top"><span class="mk mk--adv"></span><span class="adv__id">${a.id}</span><span class="adv__sev adv__sev--${a.severity}">${a.severity}</span>${SMPTAG}</div>
      <div class="adv__txt">${esc(a.summary)}</div>
      <div class="adv__meta"><span>In <b>${esc(a.component)}</b></span><span>Fixed in <b>${esc(a.fixed)}</b></span><a href="${a.url}">Read it</a></div>
    </div>`).join('') + `<div class="note-line">Reaches ${allUp(p.n)} packages through what uses it, ${closureSvc(p.n)} of them running as services.</div>`;
  else if (Array.isArray(ad)) head = `<div class="note-line">None listed at the last fetch. An empty list is not proof that it is safe.</div>`;
  else head = `<div class="note-line">Never checked: Oath has no advisory file for this build, which means unknown, not safe.${p.overlay ? ' Oath fetches them by hash when the origin serves a build, so a local build never gets one.' : ''}</div>`;
  const comps = p.comps?.length ? `<div class="comp">${p.comps.map(c => `<div>${esc(c[0])}</div><div class="mono">${esc(c[1])}</div><div class="mono">${esc(c[2])}</div>`).join('')}</div>` : `<div class="note-line">No components list, so what is inside is unknown.</div>`;
  return head + `<div class="side__cap" style="padding:8px 0 0">Upstream components</div>` + comps;
}
function closureSvc(n) {
  const names = [...closure(n, usersOf)];
  return names.filter(x => PK[x]?.svc?.length).length;
}
function restartsNote(p) {
  if (p.n === 'sola') return 'The desk restarts. Running apps and games keep going.';
  if (p.n === 'rivulet') return 'This ends the session: the compositor goes with it.';
  return '';
}
function sheet(p, bare) {
  const [tone, text] = verdict(p);
  const up = allUp(p.n), dn = allDn(p.n), bins = p.bins || [];
  const st = standing(p);
  const parts = [
    sec('where', 'Where it stands', '', places(p)),
    sec('tree', 'Tree', `${dn} below, ${up} above`, miniTree(p), `<span class="act" data-view="tree">Open Tree</span>`),
  ];
  if (p.svc?.length) parts.push(sec('restarts', 'Moving it restarts', `${p.svc.length} service${p.svc.length > 1 ? 's' : ''}`, `<div class="chips">${p.svc.map(([s]) => `<span class="chip chip--bin">${s}</span>`).join('')}</div>${restartsNote(p) ? `<div class="note-line">${restartsNote(p)}</div>` : ''}`));
  parts.push(sec('security', 'Security', hasAdv(p) ? `${p.adv.length} advisor${p.adv.length > 1 ? 'ies' : 'y'}` : Array.isArray(p.adv) ? 'none listed' : 'never checked', security(p)));
  if (bins.length) parts.push(sec('programs', 'Programs', `${bins.length} in /bin`, `<div class="chips">${bins.map(b => `<span class="chip chip--bin">${esc(b)}</span>`).join('')}</div>`));
  if (p.nvers) parts.push(sec('versions', 'Versions on disk', `${p.nvers}, ${fmtSize(p.store)}`, `<div class="vers">${p.vers.map(v => `<div class="mono">${v[0]}</div><div>${v[2]}</div><div class="${v[3] ? 'live' : ''}">${v[3] ? 'live' : ''}</div><div class="num">${fmtSize(v[1])}</div>`).join('')}</div>${p.nvers > p.vers.length ? `<div class="note-line">${p.nvers - p.vers.length} older not shown.</div>` : ''}`));
  if (p.src) parts.push(sec('source', 'Where it came from', built(p), `<dl class="kv"><dt>Repository</dt><dd>${esc(p.src.repo)}</dd><dt>Commit</dt><dd>${esc(p.src.commit)}</dd><dt>Branch</dt><dd>${esc(p.src.branch)}</dd><dt>Dirty</dt><dd>${p.src.dirty ? 'yes' : 'no'}</dd><dt>Built by</dt><dd>${esc(p.src.by)}</dd><dt>Built at</dt><dd>${esc(p.src.at)}</dd></dl>`));
  parts.push(sec('raw', 'Everything', `oath get pkg:${p.n}`, `<div class="raw">${esc(JSON.stringify({ id: 'pkg:' + p.n, present: p.on, hash: p.h, removable: p.rm, needs: p.needs, links: p.bins, source: p.src, verified: p.ok, advisories: p.adv }, null, 1))}</div>`));
  return `<aside class="sheet pane ${bare ? 'sheet--bare' : ''}" id="sheet">
    <div>
      <div class="sh__top"><div><span class="sh__name">${p.n}</span> <span class="sh__ver">${esc(p.ver)}</span> ${p.smp.size ? SMPTAG : ''}</div>
        <div class="sh__badges"><span class="badge badge--${st}">${mk(p)}${ST[st]}</span>${hasAdv(p) ? `<span class="badge badge--adv"><span class="mk mk--adv"></span>Advisories</span>` : ''}</div></div>
      <div class="sh__desc">${esc(p.desc)}</div>
      ${p.home ? `<div class="sh__home"><a href="${esc(p.home)}">${esc(p.home.replace('https://', ''))}</a></div>` : ''}
    </div>
    <div class="verdict ${tone ? 'verdict--' + tone : ''}" id="verdict">${text}</div>
    ${parts.join('')}
  </aside>`;
}

// ── Tree ───────────────────────────────────────────────────────
const fr = (n, self) => `<button class="fr ${self ? 'fr--self' : ''}" data-pkg="${n}" data-node="${n}">${PK[n] ? mk(PK[n]) : ''}<span class="t">${n}</span>${PK[n] && hasAdv(PK[n]) ? advMk(PK[n]) : ''}</button>`;
function foldLevels(ls, keep) {
  return ls.length > keep ? [...ls.slice(0, keep - 1), ls.slice(keep - 1).flat().sort()] : ls;
}
function flowTake(id) {
  const up = foldLevels(upLv(id), 3), dn = foldLevels(dnLv(id), 3);
  const cap = (t, s) => `<div class="flow__cap"><b>${t}</b>${s}</div>`;
  const name = i => i === 0 ? 'direct' : i === 1 ? '2 away' : '3 or more';
  const upc = [...up].map((ns, i) => [i, ns]).reverse().map(([i, ns]) => `<div class="flow__col">${cap(`Uses it, ${name(i)}`, ns.length)}${ns.map(n => fr(n)).join('')}</div>`);
  const dnc = dn.map((ns, i) => `<div class="flow__col">${cap(`Needs, ${name(i)}`, ns.length)}${ns.map(n => fr(n)).join('')}</div>`);
  return `<div class="flow" id="tree-take" data-orient="h">
    <svg class="edges"></svg>
    ${upc.join('')}
    <div class="flow__col flow__col--self">${cap('This package', '')}${fr(id, true)}</div>
    ${dnc.join('')}
  </div>`;
}
function treeView(bare) {
  const id = S.sel, p = PK[id];
  return `<main class="treep pane">
    <div class="treep__head"><span class="treep__name">${id}</span><span class="treep__sub">${allDn(id)} below, ${allUp(id)} above. Each column uses the one to its right.</span></div>
    ${S.trail.length ? `<div class="crumbs">${S.trail.map(t => `<button class="chip" data-pkg="${t}">${t}</button>`).join('›')}</div>` : ''}
    ${flowTake(id)}
  </main>`;
}

// ── Map ────────────────────────────────────────────────────────
function loadOf(p) {
  const u = allUp(p.n);
  return u >= 30 ? 3 : u >= 10 ? 2 : u >= 2 ? 1 : 0;
}
function blockCls(p) {
  const st = standing(p);
  return `bl bl--l${loadOf(p)} ${st !== 'current' ? 'bl--' + st : ''} ${hasAdv(p) ? 'bl--advd' : ''}`;
}
const block = p => `<button class="${blockCls(p)}" data-pkg="${p.n}" data-node="${p.n}">${standing(p) !== 'current' ? mk(p) : ''}<span class="t">${p.n}</span>${advMk(p)}</button>`;
function strata() {
  const fams = C.families;
  const graph = all.filter(p => !alone(p.n));
  const lone = all.filter(p => alone(p.n));
  const maxD = Math.max(...graph.map(p => depthOf(p.n)));
  const rows = [];
  for (let d = maxD; d >= 0; d--) {
    const cells = fams.map(f => {
      const ps = graph.filter(p => p.fam === f && depthOf(p.n) === d).sort((a, b) => allUp(b.n) - allUp(a.n) || a.n.localeCompare(b.n));
      return `<div class="st__cell">${ps.map(block).join('')}</div>`;
    }).join('');
    rows.push(`<div class="st__row"><div class="st__lv">${d}</div>${cells}</div>`);
  }
  return `<div class="st" style="--nf:${fams.length}">
    <div class="st__rows" id="strata-rows" data-orient="v"><svg class="edges"></svg>${rows.join('')}
      <div class="st__ground"><div></div>${fams.map(f => {
        const ps = all.filter(p => p.fam === f);
        return `<div class="st__fam">${f}<i>${ps.length} · ${fmtSize(ps.reduce((a, p) => a + inUse(p), 0))}</i></div>`;
      }).join('')}</div>
    </div>
    <div class="st__tray"><div class="st__tray__cap">Stand alone ${lone.length}</div><div class="st__tray__bl">${lone.map(block).join('')}</div></div>
  </div>`;
}
function card(p) {
  return `<div class="card" id="map-card"><div class="card__top"><span class="card__name">${p.n}</span><span class="sh__ver">${esc(p.ver)}</span><span class="badge badge--${standing(p)}" style="margin-left:auto">${mk(p)}${ST[standing(p)]}</span></div>
    <div class="card__txt">${verdict(p)[1]}</div>
    <div class="card__cnt"><div><b>${allDn(p.n)}</b>below</div><div><b>${allUp(p.n)}</b>above</div><div><b>${fmtSize(p.sz)}</b>in use</div><div><b>${fmtSize(p.store)}</b>on disk</div></div>
    <div class="card__acts"><button class="btn" data-view="list">Details</button><button class="btn" data-view="tree">Open Tree</button></div></div>`;
}
const mapLegend = () => `<div class="legend" id="map-legend">
  <span><span class="mk mk--local"></span>Local build</span><span><span class="mk mk--unpushed"></span>Not pushed</span><span><span class="mk mk--update"></span>Update</span><span><span class="mk mk--adv"></span>Advisory</span><span><span class="mk mk--off"></span>Off</span><span><span class="mk mk--remote"></span>On origin only</span>
  <span><i class="ln" style="color:var(--up)"></i>Uses it</span><span><i class="ln" style="color:var(--down)"></i>It needs</span><span style="color:var(--ink-3);margin-left:auto"><i class="smp">sample</i> Solid amber, cyan, red and dashed are invented for this comp.</span></div>`;
function mapView() {
  const tabs = [['strata', 'Strata'], ['weight', 'Weight']];
  const blurb = { strata: 'Each column a family, each row how far it stands from the ground. Brighter blocks hold up more.', weight: S.disk ? 'Area is what each package takes on disk. Solid is the build in use, hatched is old versions.' : 'Area is the size of the build in use.' };
  return `<main class="mapp pane"><div class="mapp__head"><span class="mapp__title">Map</span><span class="treep__sub" style="color:var(--ink-3)">${blurb[S.map]}</span>
      ${S.map === 'weight' ? `<div class="tabs" style="margin-left:12px"><button class="tab ${S.disk ? 'on' : ''}" data-disk="1">On disk ${fmtSize(all.reduce((a, p) => a + (p.store || 0), 0))}</button><button class="tab ${S.disk ? '' : 'on'}" data-disk="0">In use ${fmtSize(all.reduce((a, p) => a + inUse(p), 0))}</button></div>` : ''}
      <div class="tabs">${tabs.map(([k, l]) => `<button class="tab ${S.map === k ? 'on' : ''}" data-map="${k}">${l}</button>`).join('')}</div></div>
    <div class="mapp__stage" id="stage">${S.map === 'strata' ? strata() : '<div class="wt" id="wt"></div>'}${S.sel ? card(PK[S.sel]) : ''}</div>
    <div style="padding:0 24px 14px;flex-shrink:0">${mapLegend()}</div>
  </main>`;
}

// Edges for strata and tree takes: the selected package's cone, lit.
function drawEdges(wrap, id, hot) {
  const svg = wrap.querySelector(':scope > svg.edges');
  if (!svg) return;
  const b = wrap.getBoundingClientRect();
  const rect = {};
  wrap.querySelectorAll('[data-node]').forEach(n => { rect[n.dataset.node] = n.getBoundingClientRect(); });
  const ups = new Set([id, ...closure(id, usersOf)]), dns = new Set([id, ...closure(id, needsOf)]);
  const horiz = wrap.dataset.orient === 'h';
  let out = '';
  const add = (from, to, cls) => {
    const a = rect[from], c = rect[to];
    if (!a || !c) return;
    if (horiz) {
      const x1 = a.right - b.left, y1 = a.top + a.height / 2 - b.top, x2 = c.left - b.left, y2 = c.top + c.height / 2 - b.top;
      if (x2 <= x1) return;
      const mx = (x1 + x2) / 2;
      out += `<path class="${cls}" d="M${x1} ${y1}C${mx} ${y1} ${mx} ${y2} ${x2} ${y2}"/>`;
    } else {
      if (Math.abs(a.top - c.top) < 4) return;
      const down = c.top >= a.top;
      const x1 = a.left + a.width / 2 - b.left, y1 = (down ? a.bottom : a.top) - b.top, x2 = c.left + c.width / 2 - b.left, y2 = (down ? c.top : c.bottom) - b.top;
      const my = (y1 + y2) / 2;
      out += `<path class="${cls}" d="M${x1} ${y1}C${x1} ${my} ${x2} ${my} ${x2} ${y2}"/>`;
    }
  };
  for (const u of ups) for (const n of needsOf(u)) if (ups.has(n)) add(u, n, `e-up ${u === id || n === id ? 'hot' : ''} ${hot && !(hot.has(u) && hot.has(n)) ? 'e-faint' : hot ? 'hot' : ''}`);
  for (const u of dns) for (const n of needsOf(u)) if (dns.has(n)) add(u, n, `e-down ${u === id || n === id ? 'hot' : ''} ${hot && !(hot.has(u) && hot.has(n)) ? 'e-faint' : hot ? 'hot' : ''}`);
  svg.innerHTML = out;
}
function lightStrata(id) {
  const stage = document.getElementById('strata-rows');
  if (!stage) return;
  if (!id) {
    document.querySelectorAll('.st [data-node]').forEach(n => n.classList.remove('self', 'lit', 'lit-1', 'dim'));
    stage.querySelector(':scope > svg.edges').innerHTML = '';
    return;
  }
  const ups = closure(id, usersOf), dns = closure(id, needsOf);
  const dir = new Set([...usersOf(id), ...needsOf(id)]);
  document.querySelectorAll('.st [data-node]').forEach(n => {
    const k = n.dataset.node;
    n.classList.toggle('self', k === id);
    n.classList.toggle('lit', ups.has(k) || dns.has(k));
    n.classList.toggle('lit-1', dir.has(k));
    n.classList.toggle('dim', k !== id && !ups.has(k) && !dns.has(k));
  });
  drawEdges(stage, id);
}
function pathsBetween(id, h) {
  if (h === id) return null;
  const ups = closure(id, usersOf), dns = closure(id, needsOf);
  if (ups.has(h)) { const r = closure(h, needsOf); return new Set([id, h, ...[...r].filter(x => ups.has(x))]); }
  if (dns.has(h)) { const r = closure(h, usersOf); return new Set([id, h, ...[...r].filter(x => dns.has(x))]); }
  return null;
}
function wireTree(id) {
  const wrap = document.getElementById('tree-take');
  if (!wrap || !wrap.dataset.orient) return;
  drawEdges(wrap, id);
  wrap.querySelectorAll('[data-node]').forEach(n => {
    n.addEventListener('mouseenter', () => {
      const hot = pathsBetween(id, n.dataset.node);
      if (!hot) return;
      wrap.querySelectorAll('[data-node]').forEach(m => { m.classList.toggle('dim', !hot.has(m.dataset.node)); m.classList.toggle('lit', hot.has(m.dataset.node) && m.dataset.node !== id); });
      drawEdges(wrap, id, hot);
    });
    n.addEventListener('mouseleave', () => {
      wrap.querySelectorAll('[data-node]').forEach(m => m.classList.remove('dim', 'lit'));
      drawEdges(wrap, id);
    });
  });
}

// Take: weight. A squarified treemap, families then packages.
function squarify(items, x, y, w, h) {
  const out = [], total = items.reduce((a, b) => a + b.v, 0);
  if (!total) return out;
  const scale = w * h / total;
  const rest = items.map(i => ({ ...i, a: i.v * scale }));
  while (rest.length) {
    const side = Math.min(w, h);
    let row = [], best = Infinity;
    while (rest.length) {
      const t = [...row, rest[0]], s = t.reduce((a, b) => a + b.a, 0);
      const mx = Math.max(...t.map(r => r.a)), mn = Math.min(...t.map(r => r.a));
      const worst = Math.max(side * side * mx / (s * s), s * s / (side * side * mn));
      if (worst <= best) { best = worst; row = t; rest.shift(); } else break;
    }
    const s = row.reduce((a, b) => a + b.a, 0), th = s / side;
    let o = 0;
    for (const r of row) {
      const l = r.a / th;
      out.push(w >= h ? { ...r, x, y: y + o, w: th, h: l } : { ...r, x: x + o, y, w: l, h: th });
      o += l;
    }
    if (w >= h) { x += th; w -= th; } else { y += th; h -= th; }
  }
  return out;
}
function drawWeight(id) {
  const box = document.getElementById('wt');
  if (!box) return;
  const W = box.clientWidth, H = box.clientHeight, val = p => S.disk ? (p.store || 0) : inUse(p);
  const here = all.filter(p => !p.remote && (S.disk || p.on));
  const tot = here.reduce((a, p) => a + val(p), 0), floor = tot * .00035;
  const fams = C.families.map(f => {
    const ps = here.filter(p => p.fam === f);
    return { f, ps, v: ps.reduce((a, p) => a + Math.max(val(p), floor), 0), real: ps.reduce((a, p) => a + val(p), 0) };
  }).sort((a, b) => b.v - a.v);
  let html = '';
  for (const r of squarify(fams, 0, 0, W, H)) {
    html += `<div class="wt__fam" style="left:${r.x}px;top:${r.y}px;width:${r.w - 4}px;height:${r.h - 4}px"><div class="wt__famcap">${r.f}<i>${fmtSize(r.real)}</i></div>`;
    const tiles = squarify(r.ps.map(p => ({ p, v: Math.max(val(p), floor) })).sort((a, b) => b.v - a.v), 1, 24, r.w - 6, r.h - 29);
    for (const t of tiles) {
      const p = t.p, st = hasAdv(p) ? 'advd' : standing(p), v = val(p), frac = S.disk && v ? Math.max(.02, Math.min(1, (p.sz || 0) / v)) : 1;
      const big = t.w > 58 && t.h > 36;
      html += `<div class="tl tl--${st} ${p.n === id ? 'self' : ''}" style="left:${t.x}px;top:${t.y}px;width:${t.w - 1}px;height:${t.h - 1}px" data-pkg="${p.n}" title="${p.n} · ${fmtSize(v)}"><div class="tl__hatch"></div><div class="tl__live" style="height:${frac * 100}%"></div>${big ? `<span class="tl__t">${p.n}</span><span class="tl__s">${fmtSize(v)}${S.disk && p.nvers > 1 ? ` · ${p.nvers} versions` : ''}</span>` : ''}</div>`;
    }
    html += '</div>';
  }
  box.innerHTML = html;
}

// ── Render ─────────────────────────────────────────────────────
function titlebar() {
  const titles = { list: 'Packages', map: 'Map', tree: S.sel };
  return `<header class="tb"><span class="tb__dot"></span><b>Oath</b><span>Packages</span><span>View</span><span>Window</span><span class="tb__title">${titles[S.view]} · novus</span></header>`;
}
function render() {
  const p = PK[S.sel] || PK.sola;
  if (S.sheet) {
    root.innerHTML = sheet(p, true);
  } else if (S.bare) {
    root.innerHTML = treeView(true);
  } else if (S.view === 'map') {
    root.innerHTML = `<div class="win win--fixed">${titlebar()}<div class="body body--map">${side()}${mapView()}</div></div>`;
  } else if (S.view === 'tree') {
    root.innerHTML = `<div class="win win--auto">${titlebar()}<div class="body body--list">${side()}${treeView()}${sheet(p)}</div></div>`;
  } else {
    root.innerHTML = `<div class="win win--auto">${titlebar()}<div class="body body--list">${side()}${packages()}${sheet(p)}</div></div>`;
  }
  after();
}
function after() {
  const id = S.sel;
  if (S.view === 'map' && !S.sheet && !S.bare) {
    if (S.map === 'strata') lightStrata(id);
    if (S.map === 'weight') drawWeight(id);
  }
  wireTree(id);
}
function repaintMap() {
  document.getElementById('map-card')?.remove();
  if (S.sel) document.getElementById('stage').insertAdjacentHTML('beforeend', card(PK[S.sel]));
  after();
}
root.addEventListener('click', e => {
  const t = e.target.closest('[data-view],[data-filter],[data-fam],[data-sec],[data-map],[data-disk],[data-pkg]');
  if (!t) {
    if (S.view === 'map' && e.target.closest('#stage') && !e.target.closest('#map-card')) { S.sel = null; repaintMap(); }
    return;
  }
  const d = t.dataset;
  if (d.view) { S.view = d.view; S.sheet = false; S.bare = false; if (d.view !== 'map') S.sel ||= 'sola'; render(); return; }
  if (d.filter) { S.filter = d.filter; S.view = 'list'; S.sel ||= 'sola'; render(); return; }
  if (d.fam) { S.fams.has(d.fam) ? S.fams.delete(d.fam) : S.fams.add(d.fam); render(); return; }
  if (d.sec) { S.open.has(d.sec) ? S.open.delete(d.sec) : S.open.add(d.sec); render(); return; }
  if (d.map) { S.map = d.map; render(); return; }
  if (d.disk) { S.disk = d.disk === '1'; render(); return; }
  if (d.pkg) {
    if (S.view === 'tree' && d.pkg !== S.sel) S.trail = [...S.trail.filter(x => x !== d.pkg), S.sel].slice(-6);
    S.sel = d.pkg;
    if (S.view === 'map' && !S.sheet) repaintMap(); else render();
  }
});
root.addEventListener('mouseover', e => {
  if (S.view !== 'map' || S.map !== 'strata' || S.sheet) return;
  const t = e.target.closest('[data-node]');
  if (t) lightStrata(t.dataset.node);
});
root.addEventListener('mouseout', e => {
  if (S.view !== 'map' || S.map !== 'strata' || S.sheet) return;
  if (e.target.closest('[data-node]')) lightStrata(S.sel);
});
render();
document.fonts?.ready.then(after);
addEventListener('resize', after);
