import { reactive, html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { persist, save } from '@sola/store';

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

interface Message {
  role: 'user' | 'assistant' | 'error';
  content: string;
  streaming: boolean;
  cancelled: boolean;
  tools: ToolCall[];
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

interface Session {
  id: string;
  name: string | null;
  status: Status;
  firstPrompt: string | null;
  workingDir: string | null;
  messages: Message[];
  metrics: Metrics | null;
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
});

persist(state, 'agent-ui', ['activeId', 'statsWidth']);

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

function truncate(s: string | null, n: number): string {
  if (!s) return '';
  return s.length > n ? s.slice(0, n) + '…' : s;
}

function setActive(id: string): void {
  state.activeId = id;
  save(state, 'agent-ui', ['activeId', 'statsWidth']);
}

function focusInput(): void {
  requestAnimationFrame(() => {
    const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
    if (ta && !ta.disabled) ta.focus();
  });
}

function scrollToBottom(): void {
  requestAnimationFrame(() => {
    const el = document.getElementById('msg-log');
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
    if (near) el.scrollTop = el.scrollHeight;
  });
}

function upsertSession(patch: Partial<Session> & { id: string }): Session {
  const existing = findSession(patch.id);
  if (existing) {
    if (patch.name !== undefined) existing.name = patch.name;
    if (patch.status !== undefined) existing.status = patch.status;
    if (patch.workingDir !== undefined) existing.workingDir = patch.workingDir;
    if (patch.firstPrompt !== undefined) existing.firstPrompt = patch.firstPrompt;
    if (patch.metrics !== undefined) existing.metrics = patch.metrics;
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
    });
  }
}

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
  session.messages = ev.messages.map((m: any) => ({
    role: m.role,
    content: m.content,
    streaming: false,
    cancelled: false,
    tools: [],
  }));
  setActive(ev.session_id);
  focusInput();
});

on('message_start', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  s.messages = [...s.messages, {
    role: 'assistant',
    content: '',
    streaming: true,
    cancelled: false,
    tools: [],
  }];
});

on('message_delta', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (last) last.content += ev.text;
});

on('message_end', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  last.streaming = false;
  if (ev.cancelled) last.cancelled = true;
});

on('tool_start', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  last.tools = [...last.tools, {
    id: `t${nextToolId++}`,
    name: ev.tool_name,
    input: ev.tool_input,
    output: null,
    isError: false,
    expanded: false,
  }];
});

on('tool_end', (ev: any) => {
  const s = findSession(ev.session_id);
  if (!s) return;
  const last = lastAssistantMessage(s);
  if (!last) return;
  // Match the most recent still-pending tool with the given name.
  for (let i = last.tools.length - 1; i >= 0; i--) {
    const t = last.tools[i];
    if (t.name === ev.tool_name && t.output === null) {
      t.output = ev.result;
      t.isError = ev.is_error;
      break;
    }
  }
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
    streaming: false,
    cancelled: false,
    tools: [],
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
    streaming: false,
    cancelled: false,
    tools: [],
  }];
  if (!s.firstPrompt) s.firstPrompt = text;

  ta.value = '';
  ta.style.height = 'auto';
  ta.focus();
  await invoke('send_message', { session_id: s.id, text });
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

function sessionRow(s: Session) {
  // Keyed by session id so Arrow.js reconciles by identity, not position.
  // This is the single source of truth for tab identity — without it, DOM
  // nodes get reused across logical rows and @click/class bindings go stale.
  return html`
    <div class="${() => 'convo-item' + (state.activeId === s.id ? ' active' : '')}"
      @click="${() => selectSession(s.id)}"
      @dblclick="${() => {
        const next = prompt('Rename session:', s.name || '');
        if (next) renameSession(s.id, next);
      }}"
    >
      <span class="${() => 'dot ' + s.status}"></span>
      <span class="convo-name">${() => s.name || truncate(s.firstPrompt, 30) || 'New session'}</span>
    </div>
  `.key(s.id);
}

function sidebarTemplate() {
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
      <div class="convo-list">
        ${() => {
          const all = filterSessions();
          const running = all.filter((s: Session) => s.status === 'running');
          const live = all.filter((s: Session) => s.status === 'idle' || s.status === 'error');
          const saved = all.filter((s: Session) => s.status === 'saved');
          const items: any[] = [];
          if (running.length) { items.push(html`<div class="group-label">Running</div>`.key('g-run'), ...running.map(sessionRow)); }
          if (live.length) { items.push(html`<div class="group-label">Sessions</div>`.key('g-live'), ...live.map(sessionRow)); }
          if (saved.length) { items.push(html`<div class="group-label">Saved</div>`.key('g-saved'), ...saved.map(sessionRow)); }
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
    save(state, 'agent-ui', ['activeId', 'statsWidth']);
  }

  return html`
    <div class="stats-panel" style="${() => !activeSession()?.metrics ? 'display:none' : `width:${state.statsWidth}px`}">
      <div class="stats-drag" @mousedown="${onDragStart}"></div>
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
    </div>
  `;
}

function toolTemplate(tool: ToolCall) {
  return html`
    <div class="${() => 'tool-call' + (tool.expanded ? ' expanded' : '')}">
      <div class="tool-hdr" @click="${() => { tool.expanded = !tool.expanded; }}">
        <span class="${() => 'icon icon-chevron-right arrow' + (tool.expanded ? ' open' : '')}"></span>
        <span class="tname">${tool.name}</span>
        <span class="${() => 'tstatus' + (tool.output === null ? '' : tool.isError ? ' error' : ' done')}">
          ${() => tool.output === null ? 'running…' : tool.isError ? 'error' : 'done'}
        </span>
      </div>
      <div class="${() => 'tool-body' + (tool.expanded ? ' open' : '')}">
        <div class="tool-label">Input</div>
        <pre>${() => truncate(tool.input, 2000)}</pre>
        ${() => tool.output !== null ? html`
          <div class="tool-label">Output</div>
          <pre class="${tool.isError ? 'terr' : ''}">${truncate(tool.output, 2000)}</pre>
        ` : html``}
      </div>
    </div>
  `.key(tool.id);
}

function messageTemplate(msg: Message, idx: number) {
  // No stable per-message id from the backend; index is fine since messages
  // are append-only within a session and never reordered.
  if (msg.role === 'user') {
    return html`<div class="msg user">${() => msg.content}</div>`.key(`u${idx}`);
  }
  if (msg.role === 'error') {
    return html`<div class="msg error-msg">${() => msg.content}</div>`.key(`e${idx}`);
  }
  return html`<div class="msg assistant">${() => msg.content}${() => msg.streaming
    ? html`<span class="cursor"></span>`
    : msg.cancelled ? html`<span class="cancelled-label"> Cancelled</span>` : html``}${() => msg.tools.map(toolTemplate)}</div>`.key(`a${idx}`);
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
    <div class="input-area">
      <textarea id="msg-input" rows="1"
        placeholder="${() => !state.activeId ? 'Create a session to start…' :
                           isRunning() ? 'Send a follow-up…' : 'Send a message…'}"
        disabled="${() => !state.activeId ? 'disabled' : false}"
        class="${() => isRunning() ? 'running' : ''}"
        @keydown="${(e: KeyboardEvent) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage();
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
        ${() => isRunning()
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
