// Machines: every Oath machine in the consort, as oath-bus's socket gives
// them (consort.js), in the same marks and quiet as the packages. Query
// parameters, beside oath.js's:
//   view=machines  mt=rows|wall|across  m=<machine>  mopen=<sections>
//   msheet=1 (the sheet alone)  ask=1 (a machine asking to join)  alone=1 (no oath-bus here)

window.MV = (() => {
  const K = window.CONSORT, MS = K.machines, AT = Date.parse(K.at);
  const get = n => MS.find(m => m.name === n);
  const here = MS.find(m => m.me);
  const SERVER = '<rect x="2" y="3" width="20" height="7" rx="1.5"/><rect x="2" y="14" width="20" height="7" rx="1.5"/><path d="M6 6.5h.01M6 17.5h.01"/>';
  const WORDS = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve'];
  const word = n => WORDS[n] ?? String(n);
  const cap = s => s[0].toUpperCase() + s.slice(1);
  const smp = (m, f) => m.smp?.includes(f) ? SMPTAG : '';

  // ── Time, in the machine's own zone (MST here) ─────────────────
  const local = iso => { const d = new Date(Date.parse(iso) - 7 * 3600e3); return d.toISOString().slice(11, 16); };
  const day = iso => { const d = new Date(Date.parse(iso) - 7 * 3600e3); return d.toISOString().slice(0, 10); };
  function span(ms) {
    const m = Math.round(ms / 60e3);
    if (m < 60) return `${m} min`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h} h ${m % 60 ? (m % 60) + ' min' : ''}`.trim();
    return `${Math.floor(h / 24)} d ${h % 24} h`;
  }
  const since = iso => span(AT - Date.parse(iso));
  const gb = b => b >= 2 ** 30 ? `${Math.round(b / 2 ** 30)} GB` : `${Math.round(b / 2 ** 20)} MB`;

  // ── What a machine is: DMI and cpuinfo, the boards' placeholders left out
  const PLACEHOLDER = /^(System Product Name|To Be Filled By O\.E\.M\.|Default string|Not Specified)$/i;
  function cpuName(c) {
    return (c || '').replace(/\((R|TM)\)/g, '').replace(/\b\d+(th|st|nd|rd) Gen /, '').replace(/ @ [\d.]+GHz$/, '').replace(/ CPU\b/, '')
      .replace(/ \d+-Core Processor$/, '').replace(/^(Intel|AMD) /, '').replace(/\s+/g, ' ').trim();
  }
  function what(m) {
    const x = m.status.machine || {};
    const vendor = (x.vendor || '').replace(/ Inc\.?$/, '');
    const product = PLACEHOLDER.test(x.product || '') ? '' : x.product;
    return [product ? `${vendor} ${product}` : vendor, cpuName(x.cpu), `${m.load.cpus} threads`, gb(m.load.memory.total)].filter(Boolean).join(' · ');
  }

  // ── Standing of a machine: the worst thing about it ─────────────
  const failed = m => m.status.failed || [];
  function standing(m) {
    if (failed(m).length) return 'down';
    if (!m.online) return 'away';
    if (m.update?.moves) return 'update';
    if (m.status.drift) return 'drift';
    return 'well';
  }
  const MK = { down: 'adv', away: 'off', update: 'update', drift: 'local', well: 'current' };
  const LABEL = { down: 'Service down', away: 'Away', update: 'Update', drift: 'Drift', well: 'Current' };
  const mark = m => `<span class="mk mk--${MK[standing(m)]}"></span>`;
  const ORDER = { down: 0, away: 1, update: 2, drift: 3, well: 4 };
  const sorted = () => [...MS].sort((a, b) => (!!b.me - !!a.me) || a.name.localeCompare(b.name));

  // ── Small pictures ─────────────────────────────────────────────
  function spark(vals, w, h, cpus, cls = '') {
    if (!vals?.length) return `<svg width="${w}" height="${h}"></svg>`;
    const top = Math.max(1, ...vals) * 1.15, n = vals.length;
    const pts = vals.map((v, i) => [(i / Math.max(1, n - 1)) * w * (n / 48), h - (v / top) * h]);
    const line = pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
    const hot = vals[n - 1] / cpus > 0.75;
    return `<svg class="spark ${hot ? 'spark--hot' : ''} ${cls}" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none">
      <polygon points="0,${h} ${line} ${pts[n - 1][0].toFixed(1)},${h}"/><polyline points="${line}"/></svg>`;
  }
  function gauge(used, total, warnAt = 0.9) {
    const f = Math.min(1, used / total);
    return `<span class="gauge ${f >= warnAt ? 'gauge--warn' : ''}"><i style="width:${(f * 100).toFixed(1)}%"></i></span>`;
  }
  const memUsed = m => m.load.memory.total - m.load.memory.available;
  const diskLow = m => m.load.disk.free / m.load.disk.size < 0.1;

  // ── The sentence ───────────────────────────────────────────────
  function sentence() {
    const away = MS.filter(m => !m.online), down = MS.filter(m => failed(m).length), up = MS.filter(m => m.online && m.update?.moves);
    const parts = [`<span class="q">${cap(word(MS.length))} machines</span>${away.length ? `<span class="q">, ${word(MS.length - away.length)} awake</span>` : ''}<span class="q">.</span>`];
    for (const m of down) parts.push(`<span class="d"><b>${m.name}</b> has ${failed(m).length > 1 ? `${word(failed(m).length)} services` : 'a service'} down: ${failed(m).join(', ')}.</span>`);
    for (const m of away) parts.push(`<span class="q"><b>${m.name}</b> went away at ${local(m.seen)}.</span>`);
    for (const m of up) parts.push(`<span class="a"><b>${m.name}</b> can move to <span class="mono">${m.update.output.slice(0, 8)}</span>, ${m.update.moves} packages.</span>`);
    if (parts.length === 1) parts.push(`<span class="q">All current, and nothing has failed.</span>`);
    return parts.join(' ');
  }

  // ── The sidebar's group ────────────────────────────────────────
  function side() {
    if (S.alone) return `<div class="side__group"><div class="side__cap">Machines</div><div class="side__none">None yet</div></div>`;
    const note = m => {
      const st = standing(m);
      if (st === 'down') return `<span class="side__note side__note--d">down</span>`;
      if (st === 'away') return `<span class="side__note">away</span>`;
      if (st === 'update') return `<span class="side__note side__note--a">${m.update.moves}</span>`;
      return '';
    };
    const near = K.nearby.filter(n => !MS.some(m => m.name === n.name));
    return `<div class="side__group"><div class="side__cap">Machines</div>
      ${sorted().map(m => `<button class="si ${S.view === 'machines' && S.m === m.name ? 'on' : ''}" data-m="${m.name}">${mark(m)}<span>${m.name}</span>${m.me ? '<span class="side__here">here</span>' : ''}${note(m)}</button>`).join('')}
    </div>
    ${near.length ? `<div class="side__group"><div class="side__cap">Nearby, not joined</div>${near.map(n => `<div class="si si--near"><span class="mk mk--none"></span><span>${n.name}</span><span class="side__note">${n.address}</span></div>`).join('')}</div>` : ''}`;
  }

  // ── Take: rows ─────────────────────────────────────────────────
  function rowOf(m) {
    const st = standing(m), s = m.status, away = !m.online;
    const rel = m.update?.runs ? `<span class="h h--same">·</span>` : `<span class="h">${s.output.slice(0, 8)}</span>`;
    const upd = m.update?.error ? `<span class="small" style="color:var(--warning)">can't check</span>`
      : m.update?.moves ? `<span class="small" style="color:var(--accent)">${m.update.moves} to move</span>` : `<span class="h h--same">·</span>`;
    const svc = failed(m).length ? `<span class="small" style="color:var(--danger)">${failed(m).join(', ')} failed</span>`
      : `<span class="small faint">${s.services.running} of ${s.services.total}</span>`;
    const l1 = m.load.load[0];
    return `<button class="tr tr--row tr--m ${away ? 'tr--away' : ''} ${S.m === m.name ? 'on' : ''}" data-m="${m.name}">
      <span class="tr__name">${mark(m)}<span class="t">${m.name}</span>${m.me ? '<i class="here">here</i>' : ''}${m.smp?.length > 1 ? SMPTAG : ''}</span>
      <span class="small faint">${s.profile}</span>
      ${rel}
      <span class="small small--mono faint num">${s.generation}</span>
      ${upd}
      ${svc}
      <span class="cell-load">${spark(m.history, 92, 20, m.load.cpus)}<span class="small small--mono">${l1.toFixed(1)}</span><span class="small small--mono faint">/ ${m.load.cpus}</span></span>
      <span class="cell-bar">${gauge(memUsed(m), m.load.memory.total)}<span class="small small--mono faint">${gb(memUsed(m))} / ${gb(m.load.memory.total)}</span></span>
      <span class="cell-bar">${gauge(m.load.disk.size - m.load.disk.free, m.load.disk.size)}<span class="small small--mono ${diskLow(m) ? '' : 'faint'}" ${diskLow(m) ? 'style="color:var(--warning)"' : ''}>${gb(m.load.disk.free)} free</span></span>
      <span class="small ${away ? '' : 'faint'}">${away ? `away ${since(m.seen)}` : `up ${since(s.booted)}`}</span>
    </button>`;
  }
  function lately() {
    const ev = [...K.lately].sort((a, b) => Date.parse(b[0]) - Date.parse(a[0]));
    return `<div class="grp" id="lately"><div class="grp__cap">Lately <b>${ev.length}</b>${SMPTAG}</div>
      ${ev.map(([t, n, txt, tone]) => `<div class="ev"><span class="ev__t mono">${local(t)}</span><span class="ev__m">${get(n) ? mark(get(n)) : ''}${n}</span><span class="ev__x ${tone ? 'ev__x--' + tone : ''}">${esc(txt)}</span></div>`).join('')}
    </div>`;
  }
  function rows() {
    return `<div class="tbl tbl--m" id="mtable">
      <div class="tr tr--head"><span>Machine</span><span>Profile</span><span>Release</span><span class="num">Gen</span><span>Update</span><span>Services</span><span>Load, 12 min</span><span>Memory</span><span>Disk</span><span>Up</span></div>
      ${sorted().map(rowOf).join('')}
    </div>${lately()}`;
  }

  // ── Take: wall ─────────────────────────────────────────────────
  function tile(m) {
    const s = m.status, st = standing(m), away = !m.online;
    const line = away ? `Away since ${local(m.seen)}. What it last said:`
      : st === 'down' ? `${failed(m).join(', ')} failed. ${s.services.running} of ${s.services.total} services run.`
      : st === 'update' ? `On <span class="mono">${s.output.slice(0, 8)}</span>; the newest moves ${m.update.moves} packages.`
      : `Current, generation ${s.generation}. ${s.services.running} of ${s.services.total} services run.`;
    const top = [...m.processes].sort((a, b) => b.cpu - a.cpu).slice(0, 3);
    return `<button class="tile tile--${st} ${S.m === m.name ? 'on' : ''}" data-m="${m.name}">
      <div class="tile__top">${mark(m)}<span class="tile__name">${m.name}</span><span class="tile__prof">${s.profile}</span>${m.me ? '<i class="here">here</i>' : ''}<span class="tile__up">${away ? `away ${since(m.seen)}` : `up ${since(s.booted)}`}</span></div>
      <div class="tile__what">${esc(what(m))}</div>
      <div class="tile__line">${line}</div>
      <div class="tile__load">${spark(m.history, 400, 64, m.load.cpus, 'spark--big')}<div class="tile__ln"><b class="mono">${m.load.load[0].toFixed(1)}</b><span>load on ${m.load.cpus}</span></div></div>
      <div class="tile__bars">
        <div><span>Memory</span>${gauge(memUsed(m), m.load.memory.total)}<span class="mono">${gb(memUsed(m))} / ${gb(m.load.memory.total)}</span></div>
        <div><span>Disk</span>${gauge(m.load.disk.size - m.load.disk.free, m.load.disk.size)}<span class="mono" ${diskLow(m) ? 'style="color:var(--warning)"' : ''}>${gb(m.load.disk.free)} free</span></div>
      </div>
      <div class="tile__procs">${top.map(p => `<div><span>${esc(p.name)}</span><span class="mono">${p.cpu.toFixed(0)}%</span><span class="mono">${gb(p.rss)}</span></div>`).join('')}</div>
    </button>`;
  }
  const wall = () => `<div class="wall" id="wall">${sorted().map(tile).join('')}</div>${lately()}`;

  // ── Take: packages across machines ─────────────────────────────
  function across() {
    const cols = sorted();
    const has = cols.map(m => Object.fromEntries(m.packages.filter(p => p.present).map(p => [p.name, p])));
    const names = [...new Set(cols.flatMap(m => m.packages.map(p => p.name)))].sort();
    // No head or pin: the build most machines run stands in for newest.
    const most = n => { const c = {}; for (const h of has) if (h[n]) c[h[n].hash] = (c[h[n].hash] || 0) + 1; return Object.entries(c).sort((a, b) => b[1] - a[1])[0]?.[0]; };
    const refOf = n => K.ref[n] || most(n);
    const differs = names.filter(n => has.some(h => h[n] && h[n].hash !== refOf(n)));
    const same = names.length - differs.length;
    const cell = (h, n) => {
      const p = h[n];
      if (!p) return `<span class="h h--same">–</span>`;
      if (p.hash === refOf(n)) return `<span class="h h--same">·</span>`;
      return `<span class="h h--warn">${p.hash.slice(0, 8)}</span>`;
    };
    const behind = cols.map((m, i) => differs.filter(n => has[i][n] && has[i][n].hash !== refOf(n)).length);
    return `<div class="tools"><span class="tools__hint" style="margin-left:0"><span class="mono">·</span> means the newest build, <span class="mono">–</span> means not on that machine. Only packages that differ somewhere are listed.</span></div>
    <div class="tbl tbl--x" id="across" style="--n:${cols.length}">
      <div class="tr tr--head"><span>Package</span><span>Newest</span>${cols.map(m => `<span class="xh">${mark(m)}${m.name}</span>`).join('')}</div>
      <div class="tr tr--sum"><span class="small faint">Behind the newest</span><span></span>${behind.map(b => `<span class="small small--mono ${b ? '' : 'faint'}">${b || '·'}</span>`).join('')}</div>
      <div class="grp"><div class="grp__cap">Differ somewhere <b>${differs.length}</b></div>
        ${differs.map(n => `<div class="tr tr--row"><span class="tr__name"><span class="t">${n}</span></span><span class="h">${(refOf(n) || '').slice(0, 8)}</span>${has.map(h => cell(h, n)).join('')}</div>`).join('')}
      </div>
      <button class="fam" style="grid-template-columns:14px 1fr"><span class="caret">${ic(I.caret, 14)}</span><span>The same wherever present <span class="fam__n">${same}</span></span></button>
    </div>`;
  }

  // ── A machine asking to join ───────────────────────────────────
  function ask() {
    const a = K.ask;
    const code = a.code.replace(/(\d{3})(\d{3})/, '$1 $2');
    return `<div class="ask" id="ask">
      <div class="ask__top"><span class="mk mk--update"></span><b>${a.name}</b> asks to join<span class="faint">, through ${a.via}, ${since(a.at)} ago · ${a.address} · ${a.fingerprint.slice(0, 18)}…</span></div>
      <div class="ask__body"><div class="ask__code mono">${code}</div>
        <div class="ask__txt">${a.name} shows six digits on its own screen. If they are these, it is the machine you think it is.<div class="note-line">Admit runs <span class="mono">sudo oath consort admit ${a.name} --code ${a.code}</span>.</div></div>
        <div class="ask__acts"><span class="btn btn--acc">Admit ${a.name}</span><span class="btn">Not now</span></div></div>
    </div>`;
  }

  function main() {
    if (S.alone) {
      return `<main class="main pane"><div class="mempty" id="alone"><div class="mempty__t">No consort here</div>
        <div class="mempty__c">oath-bus isn't running on novus, so it hears no other machine.<br>Once <span class="mono">svc:oath-bus</span> runs, every Oath machine on the network shows here.</div></div></main>`;
    }
    const tabs = [['rows', 'Machines'], ['wall', 'Wall'], ['across', 'Packages']];
    const body = S.mt === 'wall' ? wall() : S.mt === 'across' ? across() : rows();
    return `<main class="main pane">
      ${S.ask ? ask() : ''}
      <div id="msummary"><div class="sum__where">Consort ${K.consort.short} · ${MS.length} machines · read ${local(K.at)}</div>
        <div class="sum__line">${sentence()}</div></div>
      <div class="mhead"><div class="tabs" style="margin-left:0">${tabs.map(([k, l]) => `<button class="tab ${S.mt === k ? 'on' : ''}" data-mt="${k}">${l}</button>`).join('')}</div>${S.mt === 'across' ? '' : '<span class="tools__hint"><span class="mono">·</span> under Release means the newest for its profile: desk and base each have their own</span>'}</div>
      ${body}
    </main>`;
  }

  // ── The sheet ──────────────────────────────────────────────────
  function msec(id, title, sub, body) {
    const open = S.mopen.has(id);
    return `<section class="sec" id="msec-${id}">
      <button class="sec__head" data-msec="${id}"><span class="caret ${open ? 'open' : ''}">${ic(I.caret, 14)}</span>${title}<span class="sub">${sub}</span></button>
      ${open ? `<div class="sec__body">${body}</div>` : ''}
    </section>`;
  }
  function verdict(m) {
    const s = m.status, st = standing(m);
    if (st === 'down') {
      const f = m.services.find(x => x.name === failed(m)[0]);
      return ['adv', `<b>svc:${f.name}</b> failed at ${local(f.since)} (${f.reason || 'exited'}) and has restarted ${f.restarts} times. ${s.services.running} of ${s.services.total} services run.`];
    }
    if (st === 'away') return ['', `Away since ${local(m.seen)}, ${since(m.seen)} ago. What follows is what it last said; it may be asleep.`];
    if (st === 'update') return ['acc', `Runs release <span class="mono">${s.output.slice(0, 8)}</span> from ${day(s.published)}. The newest, <span class="mono">${m.update.output.slice(0, 8)}</span>, moves ${m.update.moves} packages here. It looked at ${local(m.update.checked)}.`];
    if (st === 'drift') return ['warn', `${s.drift} object${s.drift > 1 ? 's' : ''} differ from what it was told: ${s.drifted.join(', ')}.`];
    return ['', `Current: release <span class="mono">${s.output.slice(0, 8)}</span>, generation ${s.generation}, nothing to move. Up ${since(s.booted)}.`];
  }
  function stands(m) {
    const s = m.status;
    const rows = [
      ['Release', `<span class="pl__h">${s.output}</span>`, `published ${day(s.published)}`],
      ['Newest', m.update?.runs ? `<span class="pl__h pl__h--same">the same</span>` : `<span class="pl__h pl__h--acc">${m.update?.output || '–'}</span>`, `for ${s.profile}`],
      ['Generation', `<span class="pl__h">${s.generation}</span>`, ''],
      ['Drift', s.drift ? `<span class="pl__h pl__h--warn">${s.drift}</span>` : `<span class="pl__h pl__h--same">none</span>`, s.drifted.join(', ')],
      ['Kernel', `<span class="pl__h">${s.kernel}</span>`, ''],
      ['Booted', `<span class="pl__h">${day(s.booted)} ${local(s.booted)}</span>`, `up ${since(s.booted)}`],
    ];
    return `<div class="places">${rows.map(([a, b, c]) => `<div class="pl__name">${a}</div><div>${b}</div><div class="pl__from">${esc(c)}</div>`).join('')}</div>`;
  }
  function update(m) {
    const u = m.update;
    if (!u) return `<div class="note-line">Not checked yet.</div>`;
    if (u.error) return `<div class="note-line">The last check failed: ${esc(u.error)}.</div>`;
    if (!u.moves) return `<div class="note-line">Nothing to move. Checked at ${local(u.checked)}; it checks every 30 minutes.</div>`;
    return `<div class="chips">${u.moving.map(id => `<span class="chip chip--bin">${id.replace(/^pkg:/, '')}</span>`).join('')}</div>
      <div class="note-line">${u.moves} move${u.refused ? `, ${u.refused} refused` : ''}. Checked at ${local(u.checked)}; it checks every 30 minutes.</div>`;
  }
  function services(m) {
    const rank = x => x.state === 'failed' || failed(m).includes(x.name) ? 0 : x.state === 'running' ? 2 : 1;
    const list = [...m.services].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
    const exit = x => x.last_exit?.signal ? `signal ${x.last_exit.signal}` : x.last_exit ? `exit ${x.last_exit.code}` : '';
    return `<div class="svcs">${list.map(x => `<div class="svc ${rank(x) === 0 ? 'svc--bad' : ''}">
      <span class="mono">${x.name}</span><span class="svc__st svc__st--${x.state}">${x.state}${x.state === 'stopped' && x.last_exit?.code === 0 ? ', done' : ''}</span>
      <span class="mono faint">${x.restarts ? `${x.restarts} restarts` : ''}</span><span class="mono faint">${x.since ? since(x.since) : ''}</span><span class="mono faint">${rank(x) === 0 ? exit(x) : ''}</span></div>`).join('')}</div>`;
  }
  function procs(m) {
    const by = (k, f) => [...m.processes].sort((a, b) => b[k] - a[k]).slice(0, 8)
      .map(p => `<div><span>${esc(p.name)}</span><span class="faint">${p.user}</span><span class="mono num">${f(p)}</span></div>`).join('');
    return `<div class="procs"><div><div class="side__cap">By CPU</div>${by('cpu', p => p.cpu.toFixed(0) + '%')}</div>
      <div><div class="side__cap">By memory</div>${by('rss', p => gb(p.rss))}</div></div>
      <div class="note-line">Names only: a command line can hold a secret, so oath-bus never sends one.</div>`;
  }
  function load(m) {
    const l = m.load;
    return `<div class="loadbig">${spark(m.history, 690, 70, l.cpus, 'spark--big')}</div>
      <dl class="kv"><dt>Load</dt><dd>${l.load.map(v => v.toFixed(2)).join('  ')} on ${l.cpus} threads</dd>
      <dt>Memory</dt><dd>${gb(memUsed(m))} used of ${gb(l.memory.total)}</dd>
      <dt>Disk</dt><dd>${gb(l.disk.free)} free of ${gb(l.disk.size)}</dd></dl>`;
  }
  function packages(m) {
    const present = m.packages.filter(p => p.present);
    const diff = present.filter(p => K.ref[p.name] && p.hash !== K.ref[p.name]);
    const overlays = present.filter(p => p.overlay).length;
    return `${diff.length ? `<div class="pdiff">${diff.slice(0, 14).map(p => `<span class="mono">${p.name}</span><span class="pl__h pl__h--warn">${p.hash.slice(0, 8)}</span><span class="pl__h pl__h--acc">${K.ref[p.name].slice(0, 8)}</span>`).join('')}</div>${diff.length > 14 ? `<div class="note-line">${diff.length - 14} more.</div>` : ''}` : ''}
      <div class="note-line">${present.length} present${overlays ? `, ${overlays} local builds` : ''}. ${diff.length ? `${diff.length} differ from the newest: here, then newest.` : 'Every one is the newest build.'}</div>`;
  }
  function member(m) {
    return `<dl class="kv"><dt>Address</dt><dd>${m.address}</dd><dt>Key</dt><dd>${m.fingerprint}</dd>
      <dt>Admitted</dt><dd>by ${m.admitted_by}, ${day(m.admitted)} ${local(m.admitted)}</dd>${m.seen ? `<dt>Seen</dt><dd>${day(m.seen)} ${local(m.seen)}</dd>` : ''}</dl>`;
  }
  function sheet(m, bare) {
    const st = standing(m), [tone, text] = verdict(m), s = m.status;
    const nf = failed(m).length, nsv = s.services;
    const parts = [
      msec('stands', 'Where it stands', '', stands(m)),
      msec('update', 'An update', m.update?.moves ? `${m.update.moves} to move` : 'nothing to move', update(m)),
      msec('services', 'Services', `${nsv.running} of ${nsv.total} running${nf ? `, ${nf} failed` : ''}`, services(m)),
      msec('procs', 'Processes', `top of ${m.processes.length}`, procs(m)),
      msec('load', 'Load', `${m.load.load[0].toFixed(1)} on ${m.load.cpus}`, load(m)),
      msec('pkgs', 'Packages', `${m.packages.filter(p => p.present).length} present`, packages(m)),
      msec('member', 'In the consort', `since ${day(m.admitted)}`, member(m)),
      msec('raw', 'Everything', `records from ${m.name}`, `<div class="raw">${esc(JSON.stringify({ status: s, update: m.update, load: m.load }, null, 1))}</div>`),
    ];
    return `<aside class="sheet pane ${bare ? 'sheet--bare' : ''}" id="msheet">
      <div>
        <div class="sh__top"><div><span class="sh__name">${m.name}</span> <span class="sh__ver">${s.profile}</span> ${m.smp?.length > 1 ? SMPTAG : ''}</div>
          <div class="sh__badges">${m.me ? '<span class="badge">Here</span>' : ''}<span class="badge badge--m${st}">${mark(m)}${LABEL[st]}</span></div></div>
        <div class="sh__desc">${esc(what(m))}</div>
      </div>
      <div class="verdict ${tone ? 'verdict--' + tone : ''}" id="mverdict">${text}</div>
      ${parts.join('')}
    </aside>`;
  }

  return { SERVER, side, main, sheet, get, here, title: () => S.mt === 'across' ? 'Machines · Packages' : 'Machines' };
})();
