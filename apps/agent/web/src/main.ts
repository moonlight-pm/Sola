import { reactive, html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { persist, save } from '@sola/store';
import { marked } from 'marked';

// Configure marked for inline rendering (no wrapping <p> for single lines).
marked.setOptions({ breaks: true, gfm: true });

// ── Types ────────────────────────────────────────────────────────────────────

type Status = 'saved' | 'idle' | 'running' | 'error';

interface ToolCall {
  id: string;
  name: string;
  input: string;
  output: string | null;
  isError: boolean;
  expanded: boolean;
}

type ContentBlock =
  | { kind: 'text'; text: string }
  | { kind: 'tool'; tool: ToolCall };

interface Message {
  role: 'user' | 'assistant' | 'error';
  content: string;
  blocks: ContentBlock[];
  streaming: boolean;
  cancelled: boolean;
}

interface Metrics {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  context_window: number;
  context_used_pct: number;
  total_cost_usd: number;
  duration_ms: number;
  model: string;
  num_turns: number;
}

interface McpServer {
  name: string;
  command: string;
  status: 'connected' | 'auth' | 'error';
}

type ModelChoice = 'opus' | 'sonnet';
type EffortLevel = 'low' | 'medium' | 'high' | 'max';

interface Session {
  id: string;
  name: string | null;
  status: Status;
  firstPrompt: string | null;
  workingDir: string | null;
  messages: Message[];
  metrics: Metrics | null;
  mcpServers: McpServer[];
  model: ModelChoice;
  effort: EffortLevel;
  /** True when a terminal `claude` process is currently attached to this
   *  session's task dir. Such sessions are read-only in this UI to avoid
   *  fighting with the terminal for stdin. */
  terminalActive: boolean;
  /** ms since epoch — mirrors SessionMeta.updated_at. Drives Recent +
   *  In-Terminal sort order and the date line in each sidebar row. */
  updatedAt: number | null;
}

// ── State ────────────────────────────────────────────────────────────────────
//
// One reactive tree. Arrow.js deep-wraps nested objects; leaf writes
// (`session.status = 'running'`) emit via the proxy set trap. For array
// changes we reassign (`s.messages = [...s.messages, m]`) — this matches
// the convention used elsewhere in the codebase (apps/terminal) and is
// the most reliable way to trigger re-render of list bindings.

const state = reactive({
  sessions: [] as Session[],
  activeId: null as string | null,
  searchQuery: '',
  editingTitle: false,
  statsWidth: 240,
  pinnedIds: [] as string[],
  sync: { active: false, current: 0, total: 0 },
});

persist(state, 'agent-ui', ['activeId', 'statsWidth', 'pinnedIds']);

// Track in-flight tool calls by stable id so output events match the right
// ToolCall entry even when tools of the same name run in sequence.
let nextToolId = 1;

// ── Helpers ──────────────────────────────────────────────────────────────────

function findSession(id: string | null): Session | undefined {
  if (!id) return undefined;
  return state.sessions.find((s: Session) => s.id === id);
}

function activeSession(): Session | undefined {
  return findSession(state.activeId);
}

function isRunning(): boolean {
  return activeSession()?.status === 'running';
}

function isReadOnly(): boolean {
  return !!activeSession()?.terminalActive;
}

function truncate(s: string | null, n: number): string {
  if (!s) return '';
  return s.length > n ? s.slice(0, n) + '…' : s;
}

const MONTHS = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];

function formatRelative(ms: number | null): string {
  if (!ms) return '';
  const now = Date.now();
  const delta = now - ms;
  if (delta < 60_000) return 'just now';
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`;
  if (delta < 7 * 86_400_000) return `${Math.floor(delta / 86_400_000)}d ago`;
  const d = new Date(ms);
  const thisYear = new Date(now).getFullYear();
  const label = `${MONTHS[d.getMonth()]} ${d.getDate()}`;
  return d.getFullYear() === thisYear ? label : `${label}, ${d.getFullYear()}`;
}

function saveUi(): void {
  save(state, 'agent-ui', ['activeId', 'statsWidth', 'pinnedIds']);
}

function setActive(id: string): void {
  state.activeId = id;
  saveUi();
  loadMcps(id);
  // Session switch — always start pinned to the bottom of the new log.
  scrollToBottom(true);
}

const mcpLoading = reactive({ active: false });

function loadMcps(sessionId: string): void {
  const s = findSession(sessionId);
  if (!s || mcpLoading.active) return;
  mcpLoading.active = true;
  const dir = s.workingDir || '.';
  invoke('list_mcps', { working_dir: dir }).then((result: any) => {
    const current = findSession(sessionId);
    if (current && result?.servers) {
      current.mcpServers = result.servers;
    }
    mcpLoading.active = false;
  }).catch(() => { mcpLoading.active = false; });
}

function pinSession(id: string, atIndex?: number): void {
  if (state.pinnedIds.indexOf(id) >= 0) return;
  const copy = [...state.pinnedIds];
  if (atIndex !== undefined) copy.splice(atIndex, 0, id);
  else copy.push(id);
  state.pinnedIds = copy;
  saveUi();
}

function unpinSession(id: string): void {
  if (state.pinnedIds.indexOf(id) < 0) return;
  state.pinnedIds = state.pinnedIds.filter((x: string) => x !== id);
  saveUi();
}

function reorderPinned(fromIndex: number, toIndex: number): void {
  const copy = [...state.pinnedIds];
  const [moved] = copy.splice(fromIndex, 1);
  copy.splice(toIndex, 0, moved);
  state.pinnedIds = copy;
  saveUi();
}

function focusInput(): void {
  requestAnimationFrame(() => {
    const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
    if (ta && !ta.disabled) ta.focus();
  });
}

// Log-tail scroll behavior: pinned to bottom by default. The listener
// on #msg-log updates this on any scroll (user or programmatic); when
// the user scrolls up, stickyBottom becomes false and we stop chasing
// the tail. When the user scrolls back to the bottom, it becomes true
// and auto-follow resumes.
let stickyBottom = true;
const STICKY_THRESHOLD = 32;

function scrollToBottom(force = false): void {
  requestAnimationFrame(() => {
    const el = document.getElementById('msg-log');
    if (!el) return;
    if (force) stickyBottom = true;
    if (stickyBottom) el.scrollTop = el.scrollHeight;
  });
}

function bindMsgLogScroll(): void {
  const el = document.getElementById('msg-log');
  if (!el || (el as any)._stickyBound) return;
  (el as any)._stickyBound = true;
  el.addEventListener('scroll', () => {
    stickyBottom = el.scrollHeight - el.scrollTop - el.clientHeight < STICKY_THRESHOLD;
  }, { passive: true });
}

function upsertSession(patch: Partial<Session> & { id: string }): Session {
  const existing = findSession(patch.id);
  if (existing) {
    if (patch.name !== undefined) existing.name = patch.name;
    if (patch.status !== undefined) existing.status = patch.status;
    if (patch.workingDir !== undefined) existing.workingDir = patch.workingDir;
    if (patch.firstPrompt !== undefined) existing.firstPrompt = patch.firstPrompt;
    if (patch.metrics !== undefined) existing.metrics = patch.metrics;
    if ((patch as any).model !== undefined) existing.model = (patch as any).model;
    if ((patch as any).effort !== undefined) existing.effort = (patch as any).effort;
    if (patch.terminalActive !== undefined) existing.terminalActive = patch.terminalActive;
    if (patch.updatedAt !== undefined) existing.updatedAt = patch.updatedAt;
    return existing;
  }
  const fresh: Session = {
    id: patch.id,
    name: patch.name ?? null,
    status: patch.status ?? 'saved',
    firstPrompt: patch.firstPrompt ?? null,
    workingDir: patch.workingDir ?? null,
    messages: [],
    metrics: patch.metrics ?? null,
    mcpServers: [],
    model: ((patch as any).model || 'opus') as ModelChoice,
    effort: ((patch as any).effort || 'high') as EffortLevel,
    terminalActive: patch.terminalActive ?? false,
    updatedAt: patch.updatedAt ?? null,
  };
  // Reassign rather than push: triggers the outer state's set trap, which
  // is the idiom the rest of this codebase uses (see apps/terminal).
  state.sessions = [...state.sessions, fresh];
  return findSession(patch.id)!;
}

function lastAssistantMessage(session: Session): Message | undefined {
  for (let i = session.messages.length - 1; i >= 0; i--) {
    const m = session.messages[i];
    if (m.role === 'assistant') return m;
  }
  return undefined;
}

// ── Events from Rust ─────────────────────────────────────────────────────────

function ingestConversations(conversations: any[]): void {
  for (const c of conversations) {
    const m = c.metrics;
    upsertSession({
      id: c.session_id,
      name: c.name || null,
      status: findSession(c.session_id)?.status ?? 'saved',
      firstPrompt: c.first_prompt || null,
      workingDir: c.working_dir || null,
      metrics: m ? {
        input_tokens: m.input_tokens || 0,
        output_tokens: m.output_tokens || 0,
        cache_read_tokens: m.cache_read_tokens || 0,
        cache_creation_tokens: m.cache_creation_tokens || 0,
        context_window: m.context_window || 0,
        context_used_pct: m.context_used_pct || 0,
        total_cost_usd: m.total_cost_usd || 0,
        duration_ms: m.duration_ms || 0,
        model: m.model || 'unknown',
        num_turns: m.num_turns || 0,
      } : undefined,
      model: c.model || undefined,
      effort: c.effort || undefined,
      terminalActive: !!c.active,
      updatedAt: typeof c.updated_at === 'number' ? c.updated_at : null,
    } as any);
  }
}

on('sync_start', (ev: any) => {
  state.sync = { active: true, current: 0, total: ev.total || 0 };
});

on('session_updated', (ev: any) => {
  ingestConversations([ev]);
  if (ev.total) {
    state.sync = { active: true, current: ev.current || 0, total: ev.total };
  }
});

on('sync_complete', () => {
  state.sync = { active: false, current: 0, total: 0 };
});

on('active_sessions', (ev: any) => {
  const live = new Set<string>(ev.ids || []);
  for (const s of state.sessions) {
    const next = live.has(s.id);
    if (s.terminalActive !== next) s.terminalActive = next;
  }
});

on('session_state', (ev: any) => {
  const wasSaved = findSession(ev.session_id)?.status === 'saved';
  const session = upsertSession({
    id: ev.session_id,
    name: ev.name ?? undefined,
    status: ev.status as Status,
    workingDir: ev.working_dir ?? undefined,
  });
  // Activate on initial creation or when a saved session is promoted to live.
  if (wasSaved || session.messages.length === 0 && state.activeId !== session.id) {
    setActive(session.id);
    focusInput();
  }
});

on('session_loaded', (ev: any) => {
  const session = findSession(ev.session_id);
  if (!session) return;
  const msgs: Message[] = [];
  for (const m of ev.messages) {
    const content = m.content;
    // Skip tool_result-only user messages — they're API plumbing, not user text.
    if (m.role === 'user' && Array.isArray(content) &&
        content.every((b: any) => b.type === 'tool_result')) continue;

    // Extract text content.
    let text = '';
    if (typeof content === 'string') {
      text = content;
    } else if (Array.isArray(content)) {
      text = content
        .filter((b: any) => b.type === 'text')
        .map((b: any) => b.text || '')
        .join('');
    }

    // Build interleaved blocks for assistant messages.
    const blocks: ContentBlock[] = [];
    if (m.role === 'assistant' && Array.isArray(content)) {
      for (const b of content) {
        if (b.type === 'text' && b.text) {
          blocks.push({ kind: 'text', text: b.text });
        } else if (b.type === 'tool_use') {
          blocks.push({ kind: 'tool', tool: {
            id: b.id || `t${nextToolId++}`,
            name: b.name || 'unknown',
            input: typeof b.input === 'string' ? b.input : JSON.stringify(b.input || {}),
            output: null,
            isError: false,
            expanded: false,
          }});
        }
      }
    }

    msgs.push({ role: m.role, content: text, blocks, streaming: false, cancelled: false });
  }

  // Match tool_result messages (skipped above) back to tool blocks.
  // The JSONL order is: assistant(tool_use) → user(tool_result) → ...
  // Walk the original messages to pair them up.
  for (let i = 0; i < ev.messages.length; i++) {
    const m = ev.messages[i];
    if (m.role !== 'user' || !Array.isArray(m.content)) continue;
    for (const b of m.content) {
      if (b.type !== 'tool_result') continue;
      // Find the tool block with matching id across all loaded messages.
      for (const msg of msgs) {
        for (const blk of msg.blocks) {
          if (blk.kind === 'tool' && blk.tool.id === b.tool_use_id) {
            blk.tool.output = typeof b.content === 'string' ? b.content : JSON.stringify(b.content || '');
            blk.tool.isError = b.is_error || false;
          }
        }
      }
    }
  }

  session.messages = msgs;
  setActive(ev.session_id);
  focusInput();
  requestAnimationFrame(flushMd);
  scrollToBottom(true);
});

on('message_start', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  s.messages = [...s.messages, {
    role: 'assistant',
    content: '',
    blocks: [],
    streaming: true,
    cancelled: false,
  }];
  scrollToBottom();
});

on('message_delta', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  // Trim leading whitespace from the very first text delta.
  const delta = last.content === '' ? ev.text.replace(/^\s+/, '') : ev.text;
  if (!delta) return;
  last.content += delta;
  // Extend the last text block or create a new one.
  const b = last.blocks;
  const tail = b.length ? b[b.length - 1] : null;
  if (tail && tail.kind === 'text') {
    tail.text += delta;
  } else {
    last.blocks = [...b, { kind: 'text' as const, text: delta }];
  }
  flushMd();
  scrollToBottom();
});

on('message_end', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  last.streaming = false;
  if (ev.cancelled) last.cancelled = true;
  flushMd();
  scrollToBottom();
});

on('tool_start', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  const tool: ToolCall = {
    id: `t${nextToolId++}`,
    name: ev.tool_name,
    input: ev.tool_input,
    output: null,
    isError: false,
    expanded: false,
  };
  last.blocks = [...last.blocks, { kind: 'tool' as const, tool }];
  scrollToBottom();
});

on('tool_end', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  // Match the most recent still-pending tool with the given name.
  for (let i = last.blocks.length - 1; i >= 0; i--) {
    const b = last.blocks[i];
    if (b.kind === 'tool' && b.tool.name === ev.tool_name && b.tool.output === null) {
      b.tool.output = ev.result;
      b.tool.isError = ev.is_error;
      break;
    }
  }
  scrollToBottom();
});

on('metrics', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  s.metrics = {
    input_tokens: ev.input_tokens || 0,
    output_tokens: ev.output_tokens || 0,
    cache_read_tokens: ev.cache_read_tokens || 0,
    cache_creation_tokens: ev.cache_creation_tokens || 0,
    context_window: ev.context_window || 0,
    context_used_pct: ev.context_used_pct || 0,
    total_cost_usd: ev.total_cost_usd || 0,
    duration_ms: ev.duration_ms || 0,
    model: ev.model || 'unknown',
    num_turns: ev.num_turns || 0,
  };
});

on('error', (ev: any) => {
  const sid = ev.session_id || state.activeId;
  const s = findSession(sid);
  if (!s) return;
  s.messages = [...s.messages, {
    role: 'error',
    content: ev.message,
    blocks: [],
    streaming: false,
    cancelled: false,
  }];
});

// ── Actions ──────────────────────────────────────────────────────────────────

async function sendMessage(): Promise<void> {
  const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
  if (!ta) return;
  const text = ta.value.trim();
  const s = activeSession();
  if (!text || !s) return;

  s.messages = [...s.messages, {
    role: 'user',
    content: text,
    blocks: [],
    streaming: false,
    cancelled: false,
  }];
  if (!s.firstPrompt) s.firstPrompt = text;

  ta.value = '';
  ta.style.height = 'auto';
  ta.focus();
  scrollToBottom(true);
  await invoke('send_message', { session_id: s.id, text, model: s.model, effort: s.effort });
}

async function selectSession(id: string): Promise<void> {
  const s = findSession(id);
  if (!s) return;
  if (s.status === 'saved' && s.messages.length === 0) {
    // Backend will emit session_state then session_loaded.
    await invoke('resume_session', { session_id: id });
    return;
  }
  setActive(id);
  focusInput();
}

async function createSession(dir: string): Promise<void> {
  const trimmed = dir.trim();
  if (!trimmed) return;
  await invoke('new_session', { working_dir: trimmed });
}

async function renameSession(id: string, name: string): Promise<void> {
  const trimmed = name.trim();
  if (!trimmed) return;
  const s = findSession(id);
  if (s) s.name = trimmed;
  await invoke('rename_conversation', { session_id: id, name: trimmed });
}

function showNewDialog(): void {
  const existing = document.querySelector('.overlay');
  if (existing) existing.remove();

  const overlay = document.createElement('div');
  overlay.className = 'overlay';
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) overlay.remove();
  });

  const d = document.createElement('div');
  d.className = 'dialog';

  const title = document.createElement('h3');
  title.textContent = 'New Session';
  d.appendChild(title);

  const label = document.createElement('div');
  label.className = 'field-label';
  label.textContent = 'WORKING DIRECTORY';
  d.appendChild(label);

  const input = document.createElement('input');
  input.type = 'text';
  input.value = '~';
  input.placeholder = '~/path/to/project';
  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      await createSession(input.value);
      overlay.remove();
    }
    if (e.key === 'Escape') overlay.remove();
  });
  d.appendChild(input);

  const status = document.createElement('div');
  status.className = 'path-status valid';
  status.textContent = '~';
  input.addEventListener('input', () => {
    status.textContent = input.value;
  });
  d.appendChild(status);

  const btns = document.createElement('div');
  btns.className = 'dialog-btns';

  const cancelBtn = document.createElement('button');
  cancelBtn.className = 'dbtn-cancel';
  cancelBtn.textContent = 'Cancel';
  cancelBtn.addEventListener('click', () => overlay.remove());
  btns.appendChild(cancelBtn);

  const startBtn = document.createElement('button');
  startBtn.className = 'dbtn-start';
  startBtn.textContent = 'Start Session';
  startBtn.addEventListener('click', async () => {
    await createSession(input.value);
    overlay.remove();
  });
  btns.appendChild(startBtn);

  d.appendChild(btns);
  overlay.appendChild(d);
  document.body.appendChild(overlay);

  requestAnimationFrame(() => {
    input.focus();
    input.select();
  });
}

// ── Templates ────────────────────────────────────────────────────────────────

function filterSessions(): Session[] {
  const query = state.searchQuery.toLowerCase();
  if (!query) return state.sessions;
  return state.sessions.filter((s: Session) =>
    (s.name || '').toLowerCase().includes(query) ||
    (s.firstPrompt || '').toLowerCase().includes(query)
  );
}

function deleteSession(id: string): void {
  unpinSession(id);
  state.sessions = state.sessions.filter((x: Session) => x.id !== id);
  if (state.activeId === id) {
    state.activeId = null;
    saveUi();
  }
  invoke('delete_session', { session_id: id });
}

function sessionRow(s: Session, group: string) {
  let confirmTimer: number | null = null;
  const del = reactive({ confirming: false });

  function onDeleteClick(e: MouseEvent) {
    e.stopPropagation();
    if (del.confirming) {
      if (confirmTimer) clearTimeout(confirmTimer);
      del.confirming = false;
      deleteSession(s.id);
    } else {
      del.confirming = true;
      confirmTimer = window.setTimeout(() => { del.confirming = false; }, 2500);
    }
  }

  return html`
    <div class="${() => 'convo-item' + (state.activeId === s.id ? ' active' : '') + (s.terminalActive ? ' terminal' : '')}"
      data-sid="${s.id}" data-group="${group}"
      @click="${() => selectSession(s.id)}"
    >
      <span class="${() => s.terminalActive ? 'dot terminal' : 'dot ' + s.status}"></span>
      <div class="convo-text">
        <div class="convo-name">${() => s.name || truncate(s.firstPrompt, 30) || 'New session'}</div>
        <div class="convo-date">${() => formatRelative(s.updatedAt)}</div>
      </div>
      <button class="${() => 'btn-del' + (del.confirming ? ' confirm' : '')}" @click="${onDeleteClick}" @mousedown="${(e: MouseEvent) => e.stopPropagation()}"><span class="${() => del.confirming ? 'icon icon-check' : 'icon icon-x'}"></span></button>
    </div>
  `.key(s.id);
}

function sidebarTemplate() {
  // ── Drag state (imperative, not reactive) ──
  let dragSid: string | null = null;
  let dragStartY = 0;
  let isDragging = false;
  let dropGroup: 'pinned' | 'recent' | null = null;
  let dropIndex: number | null = null;
  let ghost: HTMLElement | null = null;

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    const row = (e.target as HTMLElement).closest('.convo-item') as HTMLElement | null;
    if (!row?.dataset.sid) return;
    if (row.dataset.group === 'terminal') return;
    dragSid = row.dataset.sid;
    dragStartY = e.clientY;
    isDragging = false;
  }

  function createGhost(sourceRow: HTMLElement, e: MouseEvent) {
    const label = sourceRow.querySelector('.convo-name') as HTMLElement | null;
    const dot = sourceRow.querySelector('.dot') as HTMLElement | null;
    ghost = document.createElement('div');
    ghost.className = 'drag-ghost';
    if (dot) {
      const dotClone = dot.cloneNode(true) as HTMLElement;
      ghost.appendChild(dotClone);
    }
    const text = document.createElement('span');
    text.textContent = label?.textContent || 'Session';
    ghost.appendChild(text);
    document.body.appendChild(ghost);
    positionGhost(e);
  }

  function positionGhost(e: MouseEvent) {
    if (!ghost) return;
    ghost.style.left = (e.clientX + 12) + 'px';
    ghost.style.top = (e.clientY - 14) + 'px';
  }

  function destroyGhost() {
    ghost?.remove();
    ghost = null;
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragSid) return;
    if (!isDragging && Math.abs(e.clientY - dragStartY) > 5) {
      isDragging = true;
      const src = document.querySelector(`[data-sid="${dragSid}"]`) as HTMLElement | null;
      if (src) {
        src.classList.add('drag-source');
        createGhost(src, e);
      }
    }
    if (!isDragging) return;
    positionGhost(e);

    const list = document.querySelector('.convo-list') as HTMLElement;
    if (!list) return;
    clearHighlights(list);
    dropGroup = null;
    dropIndex = null;

    const pinnedRows = list.querySelectorAll('[data-group="pinned"].convo-item');
    const recentLabel = list.querySelector('[data-group="recent"].group-label');

    const recentTop = recentLabel?.getBoundingClientRect().top ?? Infinity;
    if (e.clientY < recentTop) {
      dropGroup = 'pinned';
      dropIndex = 0;
      for (let i = 0; i < pinnedRows.length; i++) {
        const rect = pinnedRows[i].getBoundingClientRect();
        if (e.clientY > rect.top + rect.height / 2) dropIndex = i + 1;
      }
      if (pinnedRows[dropIndex]) {
        pinnedRows[dropIndex].classList.add('drop-before');
      } else {
        const target = list.querySelector('.empty-drop') || list.querySelector('[data-group="pinned"].group-label');
        target?.classList.add('drop-target');
      }
    } else {
      dropGroup = 'recent';
      recentLabel?.classList.add('drop-target');
    }
  }

  function clearHighlights(list: HTMLElement) {
    list.querySelectorAll('.drop-target,.drop-before,.drag-source').forEach(el =>
      el.classList.remove('drop-target', 'drop-before', 'drag-source')
    );
  }

  function onMouseUp() {
    if (isDragging && dragSid && dropGroup) {
      const isPinned = state.pinnedIds.indexOf(dragSid) >= 0;
      if (dropGroup === 'pinned') {
        if (isPinned) {
          const from = state.pinnedIds.indexOf(dragSid);
          const to = dropIndex ?? state.pinnedIds.length;
          if (from !== to) reorderPinned(from, to > from ? to - 1 : to);
        } else {
          pinSession(dragSid, dropIndex ?? undefined);
        }
      } else if (dropGroup === 'recent' && isPinned) {
        unpinSession(dragSid);
      }
    }
    destroyGhost();
    const list = document.querySelector('.convo-list') as HTMLElement;
    if (list) clearHighlights(list);
    dragSid = null;
    isDragging = false;
    dropGroup = null;
    dropIndex = null;
  }

  window.addEventListener('mousemove', onMouseMove);
  window.addEventListener('mouseup', onMouseUp);

  return html`
    <div class="sidebar">
      <div class="sidebar-toolbar">
        <div class="search-wrap">
          <span class="icon icon-search search-icon"></span>
          <input type="text" placeholder="Search..."
            @input="${(e: Event) => { state.searchQuery = (e.target as HTMLInputElement).value; }}"
          />
        </div>
        <button class="btn-new" @click="${showNewDialog}">
          <span class="icon icon-plus"></span>
        </button>
      </div>
      <div class="sync-indicator" style="${() => state.sync.active ? '' : 'display:none'}">
        <span class="sync-dot"></span>
        <span class="sync-text">${() => `Syncing ${state.sync.current}/${state.sync.total}…`}</span>
      </div>
      <div class="convo-list" @mousedown="${onMouseDown}">
        ${() => {
          const all = filterSessions();
          const pinnedSet = new Set(state.pinnedIds);
          const byUpdated = (a: Session, b: Session) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0);
          const terminal = all.filter((s: Session) => s.terminalActive).sort(byUpdated);
          const terminalSet = new Set(terminal.map((s: Session) => s.id));
          // Pinned stays in the user-defined order from state.pinnedIds.
          const pinned = state.pinnedIds
            .map((id: string) => all.find((s: Session) => s.id === id))
            .filter((s): s is Session => !!s && !terminalSet.has(s.id));
          const recent = all
            .filter((s: Session) => !pinnedSet.has(s.id) && !terminalSet.has(s.id))
            .sort(byUpdated);
          const items: any[] = [];
          if (terminal.length) {
            items.push(html`<div class="group-label" data-group="terminal">In terminal</div>`.key('g-term'));
            terminal.forEach(s => items.push(sessionRow(s, 'terminal')));
          }
          items.push(html`<div class="group-label" data-group="pinned">Pinned</div>`.key('g-pin'));
          if (pinned.length) {
            pinned.forEach(s => items.push(sessionRow(s, 'pinned')));
          } else {
            items.push(html`<div class="empty-drop" data-group="pinned">Drag here to pin</div>`.key('gp-empty'));
          }
          items.push(html`<div class="group-label" data-group="recent">Recent</div>`.key('g-rec'));
          recent.forEach(s => items.push(sessionRow(s, 'recent')));
          return items;
        }}
      </div>
    </div>
  `;
}

function headerTemplate() {
  return html`
    <div class="header-bar" style="${() => state.activeId ? '' : 'display:none'}">
      <div class="header-left">
        ${() => {
          const s = activeSession();
          if (!s) return html``;
          if (state.editingTitle) {
            return html`<input class="header-title-input"
              value="${s.name || ''}"
              @blur="${(e: Event) => {
                state.editingTitle = false;
                renameSession(s.id, (e.target as HTMLInputElement).value);
              }}"
              @keydown="${(e: KeyboardEvent) => {
                if (e.key === 'Enter') {
                  state.editingTitle = false;
                  renameSession(s.id, (e.target as HTMLInputElement).value);
                }
                if (e.key === 'Escape') state.editingTitle = false;
              }}"
            />`;
          }
          return html`
            <span class="header-title">${() => s.name || 'Untitled'}</span>
            <button class="header-edit-btn" @click="${() => { state.editingTitle = true; }}">
              <span class="icon icon-pencil"></span>
            </button>
          `;
        }}
      </div>
      <span class="header-cwd">${() => activeSession()?.workingDir || ''}</span>
    </div>
  `;
}

// ── Right sidebar: session stats ─────────────────────────────────────────────

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'k';
  return String(n);
}

function fmtDuration(ms: number): string {
  if (ms >= 60_000) return (ms / 60_000).toFixed(1) + 'm';
  return (ms / 1_000).toFixed(1) + 's';
}

function fmtCost(usd: number): string {
  if (usd >= 1) return '$' + usd.toFixed(2);
  return '$' + usd.toFixed(4);
}

function statsTemplate() {
  // Resize drag state (non-reactive, imperative)
  let dragging = false;
  let dragStartX = 0;
  let dragStartWidth = 0;

  function onDragStart(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    dragStartX = e.clientX;
    dragStartWidth = state.statsWidth;
    document.addEventListener('mousemove', onDragMove);
    document.addEventListener('mouseup', onDragEnd);
    document.body.style.cursor = 'col-resize';
  }

  function onDragMove(e: MouseEvent) {
    if (!dragging) return;
    const delta = dragStartX - e.clientX;
    state.statsWidth = Math.max(180, Math.min(400, dragStartWidth + delta));
  }

  function onDragEnd() {
    dragging = false;
    document.removeEventListener('mousemove', onDragMove);
    document.removeEventListener('mouseup', onDragEnd);
    document.body.style.cursor = '';
    saveUi();
  }

  function setModel(m: string) {
    const s = activeSession();
    if (!s) return;
    s.model = m as ModelChoice;
    invoke('update_session_config', { session_id: s.id, model: m });
  }
  function setEffort(e: string) {
    const s = activeSession();
    if (!s) return;
    s.effort = e as EffortLevel;
    invoke('update_session_config', { session_id: s.id, effort: e });
  }

  return html`
    <div class="stats-panel" style="${() => !activeSession() ? 'display:none' : `width:${state.statsWidth}px`}">
      <div class="stats-drag" @mousedown="${onDragStart}"></div>
      ${() => {
        const s = activeSession();
        if (!s) return html`<div class="config-section"></div>`;
        return html`<div class="config-section">
          <div class="config-row"><span class="config-label">Model</span><div class="toggle-group"><button class="${() => s.model === 'opus' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setModel('opus')}">Opus</button><button class="${() => s.model === 'sonnet' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setModel('sonnet')}">Sonnet</button></div></div>
          <div class="config-row"><span class="config-label">Effort</span><div class="toggle-group"><button class="${() => s.effort === 'low' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setEffort('low')}">Low</button><button class="${() => s.effort === 'medium' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setEffort('medium')}">Med</button><button class="${() => s.effort === 'high' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setEffort('high')}">High</button><button class="${() => s.effort === 'max' ? 'toggle-btn active' : 'toggle-btn'}" @click="${() => setEffort('max')}">Max</button></div></div>
        </div>`;
      }}
      ${() => {
        const s = activeSession();
        const m = s?.metrics;
        if (!m) return html`<div class="stats-body"></div>`;
        const pct = Math.min(m.context_used_pct, 100);
        const barClass = pct > 90 ? 'ctx-bar danger' : pct > 70 ? 'ctx-bar warning' : 'ctx-bar';
        const totalIn = m.input_tokens + m.cache_read_tokens + m.cache_creation_tokens;
        const barStyle = `width:${pct}%`;
        return html`<div class="stats-body">
          <div class="stats-section"><div class="stats-label">Model</div><div class="stats-value model-name">${m.model}</div></div>
          <div class="stats-section"><div class="stats-label">Context</div><div class="ctx-bar-wrap"><div class="${barClass}" style="${barStyle}"></div></div><div class="stats-row"><span class="stats-dim">${pct + '%'}</span><span class="stats-dim">${fmtTokens(m.context_window) + ' window'}</span></div></div>
          <div class="stats-section"><div class="stats-label">Tokens</div><div class="stats-row"><span>In</span><span class="stats-num">${fmtTokens(totalIn)}</span></div><div class="stats-row"><span>Out</span><span class="stats-num">${fmtTokens(m.output_tokens)}</span></div><div class="stats-row sub"><span>Cache read</span><span class="stats-num">${fmtTokens(m.cache_read_tokens)}</span></div><div class="stats-row sub"><span>Cache create</span><span class="stats-num">${fmtTokens(m.cache_creation_tokens)}</span></div></div>
          <div class="stats-section"><div class="stats-label">Usage</div><div class="stats-row"><span>Turns</span><span class="stats-num">${m.num_turns}</span></div><div class="stats-row"><span>Duration</span><span class="stats-num">${fmtDuration(m.duration_ms)}</span></div><div class="stats-row"><span>Cost</span><span class="stats-num cost">${fmtCost(m.total_cost_usd)}</span></div></div>
        </div>`;
      }}
      ${() => {
        const s = activeSession();
        if (!s) return html`<div class="mcp-section"></div>`;
        const servers = s.mcpServers;
        const reloadBtn = html`<button class="${() => 'btn-mcp-reload' + (mcpLoading.active ? ' spinning' : '')}" @click="${() => loadMcps(s.id)}"><span class="icon icon-refresh"></span></button>`;
        if (!servers.length) return html`<div class="mcp-section"><div class="stats-label mcp-hdr">MCP Servers${reloadBtn}</div><div class="stats-dim">${() => mcpLoading.active ? 'Loading...' : 'No servers'}</div></div>`;
        return html`<div class="mcp-section"><div class="stats-label mcp-hdr">MCP Servers${reloadBtn}</div>${servers.map((srv: McpServer) => html`<div class="mcp-row"><span class="${'mcp-dot ' + srv.status}"></span><span class="mcp-name">${srv.name}</span></div>`.key(srv.name))}</div>`;
      }}
    </div>
  `;
}

function toolTemplate(tool: ToolCall) {
  return html`<div class="${() => 'tool-call' + (tool.expanded ? ' expanded' : '')}"><div class="tool-hdr" @click="${() => { tool.expanded = !tool.expanded; }}"><span class="${() => 'icon icon-chevron-right arrow' + (tool.expanded ? ' open' : '')}"></span><span class="tname">${tool.name}</span><span class="${() => 'tstatus' + (tool.output === null ? '' : tool.isError ? ' error' : ' done')}">${() => tool.output === null ? 'running…' : tool.isError ? 'error' : 'done'}</span></div><div class="${() => 'tool-body' + (tool.expanded ? ' open' : '')}"><div class="tool-label">Input</div><pre>${() => truncate(tool.input, 2000)}</pre>${() => tool.output !== null ? html`<div class="tool-label">Output</div><pre class="${tool.isError ? 'terr' : ''}">${truncate(tool.output, 2000)}</pre>` : html``}</div></div>`.key(tool.id);
}

// Render markdown into .md-block elements by scanning the DOM.
// Uses a WeakMap keyed on the block object to generate stable ids across re-renders.
const mdSources = new Map<string, () => string>();
const mdBlockIds = new WeakMap<object, string>();
let mdIdCounter = 0;

function getMdId(block: object, getText: () => string): string {
  let id = mdBlockIds.get(block);
  if (!id) {
    id = `md${mdIdCounter++}`;
    mdBlockIds.set(block, id);
  }
  mdSources.set(id, getText);
  return id;
}

function flushMd(): void {
  for (const [id, getText] of mdSources) {
    const el = document.querySelector(`[data-md-id="${id}"]`) as HTMLElement | null;
    if (!el) { mdSources.delete(id); continue; }
    const text = getText();
    if (el.dataset.mdLast === text) continue;
    el.dataset.mdLast = text;
    el.innerHTML = marked.parse(text) as string;
  }
}


function blockTemplate(block: ContentBlock) {
  if (block.kind === 'text') {
    const id = getMdId(block, () => block.text);
    return html`<div class="md-block" data-md-id="${id}"></div>`.key(id);
  }
  return toolTemplate(block.tool);
}

function messageTemplate(msg: Message, idx: number) {
  if (msg.role === 'user') {
    return html`<div class="msg user">${() => msg.content}</div>`.key(`u${idx}`);
  }
  if (msg.role === 'error') {
    return html`<div class="msg error-msg">${() => msg.content}</div>`.key(`e${idx}`);
  }
  return html`<div class="msg assistant">${() => {
    if (msg.blocks.length) return msg.blocks.map(blockTemplate);
    if (msg.content) {
      const id = getMdId(msg, () => msg.content);
      return [html`<div class="md-block" data-md-id="${id}"></div>`.key(id)];
    }
    return [];
  }}${() => msg.streaming
    ? html`<span class="cursor"></span>`
    : msg.cancelled ? html`<span class="cancelled-label"> Cancelled</span>` : html``}</div>`.key(`a${idx}`);
}

function messagesTemplate() {
  return html`
    <div class="messages" id="msg-log" style="${() => state.activeId ? '' : 'display:none'}">
      ${() => {
        const s = activeSession();
        if (!s) return html``;
        scrollToBottom();
        return s.messages.map(messageTemplate);
      }}
    </div>
    <div class="empty-state" style="${() => state.activeId ? 'display:none' : ''}">
      Create or select a conversation
    </div>
  `;
}

function inputTemplate() {
  return html`
    <div class="${() => 'input-area' + (isReadOnly() ? ' readonly' : '')}">
      <textarea id="msg-input" rows="1"
        placeholder="${() => !state.activeId ? 'Create a session to start…' :
                           isReadOnly() ? 'Read-only — this session is live in a terminal' :
                           isRunning() ? 'Send a follow-up…' : 'Send a message…'}"
        disabled="${() => !state.activeId || isReadOnly() ? 'disabled' : false}"
        class="${() => isRunning() ? 'running' : ''}"
        @keydown="${(e: KeyboardEvent) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            if (!isReadOnly()) sendMessage();
          }
          if (e.key === 'Escape' && isRunning() && state.activeId) {
            invoke('cancel', { session_id: state.activeId });
          }
        }}"
        @input="${(e: Event) => {
          const t = e.target as HTMLTextAreaElement;
          t.style.height = 'auto';
          t.style.height = Math.min(t.scrollHeight, 200) + 'px';
        }}"
      ></textarea>
      <div>
        ${() => isReadOnly()
          ? html``
          : isRunning()
          ? html`<button class="btn-cancel" @click="${() => invoke('cancel', { session_id: state.activeId })}">
              <span class="icon icon-square"></span>
            </button>`
          : html`<button class="btn-send" @click="${sendMessage}">
              <span class="icon icon-send"></span>
            </button>`
        }
      </div>
    </div>
  `;
}

// ── Mount ────────────────────────────────────────────────────────────────────

html`
  <div class="app">
    ${sidebarTemplate()}
    <div class="main">
      ${headerTemplate()}
      ${messagesTemplate()}
      ${inputTemplate()}
    </div>
    ${statsTemplate()}
  </div>
`(document.getElementById('app')!);

bindMsgLogScroll();

// Load saved sessions. Read the reply directly — the old event-based path
// went through an mpsc + glib timer bridge that proved fragile.
invoke('list_conversations', {}).then((result: any) => {
  if (result?.conversations) ingestConversations(result.conversations);
  // If a saved activeId was restored by persist(), auto-resume that session.
  const restored = findSession(state.activeId);
  if (restored && restored.status === 'saved' && restored.messages.length === 0) {
    invoke('resume_session', { session_id: restored.id });
  }
});
