import { reactive, html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';

// ── State ────────────────────────────────────────────────────────────────────

interface Session {
  id: string;
  name: string | null;
  status: string;
  firstPrompt: string | null;
  workingDir: string | null;
}

interface ToolCall {
  name: string;
  input: string;
  output: string | null;
  isError: boolean;
  expanded: boolean;
}

interface Message {
  role: string;
  content: string;
  streaming: boolean;
  cancelled?: boolean;
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

const state = reactive({
  sessions: [] as Session[],
  activeId: null as string | null,
  searchQuery: '',
  editingTitle: false,
  msgVersion: 0,
  metricsVersion: 0,
});

const messages: Record<string, Message[]> = {};
const metrics: Record<string, Metrics> = {};

// ── Events from Rust ─────────────────────────────────────────────────────────

on('session_state', (ev: any) => {
  const existing = state.sessions.find((x: Session) => x.id === ev.session_id);
  if (!existing) {
    state.sessions = [...state.sessions, {
      id: ev.session_id,
      name: ev.name || null,
      status: ev.status,
      firstPrompt: null,
      workingDir: ev.working_dir || null,
    }];
    messages[ev.session_id] = [];
    state.activeId = ev.session_id;
    focusInput();
  } else {
    const wasSaved = existing.status === 'saved';
    // Replace the object entirely — Arrow.js needs new references
    state.sessions = state.sessions.map(s =>
      s.id === ev.session_id ? {
        ...s,
        status: ev.status,
        name: ev.name || s.name,
        workingDir: ev.working_dir || s.workingDir,
      } : s
    );
    if (wasSaved) {
      if (!messages[ev.session_id]) messages[ev.session_id] = [];
      state.activeId = ev.session_id;
      focusInput();
    }
  }
});

on('message_start', (ev: any) => {
  const m = messages[ev.session_id];
  if (m) { m.push({ role: 'assistant', content: '', streaming: true, tools: [] }); state.msgVersion++; }
});

on('message_delta', (ev: any) => {
  const m = messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') { last.content += ev.text; state.msgVersion++; }
});

on('message_end', (ev: any) => {
  const m = messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) {
    last.streaming = false;
    if (ev.cancelled) last.cancelled = true;
    state.msgVersion++;
  }
});

on('tool_start', (ev: any) => {
  const m = messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') {
    last.tools.push({ name: ev.tool_name, input: ev.tool_input, output: null, isError: false, expanded: false });
    state.msgVersion++;
  }
});

on('tool_end', (ev: any) => {
  const m = messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) {
    const t = last.tools.find((t: ToolCall) => t.name === ev.tool_name && t.output === null);
    if (t) { t.output = ev.result; t.isError = ev.is_error; state.msgVersion++; }
  }
});

on('metrics', (ev: any) => {
  metrics[ev.session_id] = {
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
  state.metricsVersion++;
});

on('conversations_list', (ev: any) => {
  for (const c of ev.conversations) {
    if (!state.sessions.find((s: Session) => s.id === c.session_id)) {
      state.sessions = [...state.sessions, {
        id: c.session_id,
        name: c.name || null,
        status: 'saved',
        firstPrompt: c.first_prompt || null,
        workingDir: c.working_dir || null,
      }];
    }
  }
});

on('session_loaded', (ev: any) => {
  if (!messages[ev.session_id]) messages[ev.session_id] = [];
  for (const msg of ev.messages) {
    messages[ev.session_id].push({
      role: msg.role, content: msg.content, streaming: false, tools: [],
    });
  }
  state.activeId = ev.session_id;
  state.msgVersion++;
  focusInput();
});

on('error', (ev: any) => {
  const sid = ev.session_id || state.activeId;
  if (sid) {
    if (!messages[sid]) messages[sid] = [];
    messages[sid].push({ role: 'error', content: ev.message, streaming: false, tools: [] });
    state.msgVersion++;
  }
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function truncate(s: string | null, n: number): string {
  if (!s) return '';
  return s.length > n ? s.slice(0, n) + '...' : s;
}

function isRunning(): boolean {
  const s = state.sessions.find((x: Session) => x.id === state.activeId);
  return !!(s && s.status === 'running');
}

function activeSession(): Session | undefined {
  return state.sessions.find((x: Session) => x.id === state.activeId);
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
    if (el) {
      const near = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
      if (near) el.scrollTop = el.scrollHeight;
    }
  });
}

// ── Actions ──────────────────────────────────────────────────────────────────

async function sendMessage(): Promise<void> {
  const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
  if (!ta) return;
  const text = ta.value.trim();
  if (!text || !state.activeId) return;

  if (!messages[state.activeId]) messages[state.activeId] = [];
  messages[state.activeId].push({ role: 'user', content: text, streaming: false, tools: [] });

  const s = state.sessions.find((x: Session) => x.id === state.activeId);
  if (s && !s.firstPrompt) {
    state.sessions = state.sessions.map(x =>
      x.id === state.activeId ? { ...x, firstPrompt: text } : x
    );
  }

  state.msgVersion++;
  await invoke('send_message', { session_id: state.activeId, text });
  ta.value = '';
  ta.style.height = 'auto';
  ta.focus();
}

async function selectSession(id: string): Promise<void> {
  const s = state.sessions.find((x: Session) => x.id === id);
  if (s && s.status === 'saved' && !messages[id]) {
    await invoke('resume_session', { session_id: id });
  } else {
    state.activeId = id;
    state.msgVersion++;
    focusInput();
  }
}

async function createSession(dir: string): Promise<void> {
  if (!dir.trim()) return;
  await invoke('new_session', { working_dir: dir.trim() });
}

function showNewDialog(): void {
  const existing = document.querySelector('.overlay');
  if (existing) existing.remove();

  const overlay = document.createElement('div');
  overlay.className = 'overlay';
  overlay.addEventListener('click', (e) => { if (e.target === overlay) overlay.remove(); });

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
    if (e.key === 'Enter') { await createSession(input.value); overlay.remove(); }
    if (e.key === 'Escape') overlay.remove();
  });
  d.appendChild(input);

  const status = document.createElement('div');
  status.className = 'path-status valid';
  status.textContent = '~';
  input.addEventListener('input', () => { status.textContent = input.value; });
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
  startBtn.addEventListener('click', async () => { await createSession(input.value); overlay.remove(); });
  btns.appendChild(startBtn);
  d.appendChild(btns);

  overlay.appendChild(d);
  document.body.appendChild(overlay);
  requestAnimationFrame(() => { input.focus(); input.select(); });
}

async function renameSession(id: string, name: string): Promise<void> {
  if (!name.trim()) return;
  state.sessions = state.sessions.map(s =>
    s.id === id ? { ...s, name: name.trim() } : s
  );
  await invoke('rename_conversation', { session_id: id, name: name.trim() });
}

// ── Templates ────────────────────────────────────────────────────────────────

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
          const query = state.searchQuery.toLowerCase();
          const filtered = state.sessions.filter((s: Session) => {
            if (!query) return true;
            return (s.name || '').toLowerCase().includes(query) ||
                   (s.firstPrompt || '').toLowerCase().includes(query);
          });
          const running = filtered.filter((s: Session) => s.status === 'running');
          const active = filtered.filter((s: Session) => s.status === 'idle' || s.status === 'error');
          const saved = filtered.filter((s: Session) => s.status === 'saved');
          return html`
            ${() => sessionGroup('Running', running)}
            ${() => sessionGroup('Sessions', active)}
            ${() => sessionGroup('Saved', saved)}
          `;
        }}
      </div>
    </div>
  `;
}

function sessionGroup(label: string, items: Session[]) {
  if (!items.length) return html``;
  return html`
    <div class="group-label">${label}</div>
    ${items.map(s => html`
      <div class="${() => 'convo-item' + (state.activeId === s.id ? ' active' : '')}"
        @click="${() => selectSession(s.id)}"
        @dblclick="${() => {
          const newName = prompt('Rename session:', s.name || '');
          if (newName) renameSession(s.id, newName);
        }}"
      >
        <span class="${() => 'dot ' + s.status}"></span>
        <span class="convo-name">${() => s.name || truncate(s.firstPrompt, 30) || 'New session'}</span>
      </div>
    `)}
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
                if (e.key === 'Enter') { state.editingTitle = false; renameSession(s.id, (e.target as HTMLInputElement).value); }
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
      ${() => {
        void state.metricsVersion;
        const m = state.activeId ? metrics[state.activeId] : null;
        if (!m || m.context_window <= 0) return html``;
        const pct = Math.min(m.context_used_pct, 100);
        const totalTokens = m.input_tokens + m.output_tokens + m.cache_read_tokens + m.cache_creation_tokens;
        const barClass = pct > 90 ? 'context-bar danger' : pct > 70 ? 'context-bar warning' : 'context-bar';
        return html`
          <div class="header-metrics">
            <div class="context-bar-wrap"><div class="${barClass}" style="width:${pct}%"></div></div>
            <span class="header-stats">${(totalTokens / 1000).toFixed(1)}k / ${(m.context_window / 1000).toFixed(0)}k (${pct}%)</span>
          </div>
        `;
      }}
      <span class="header-cwd">${() => activeSession()?.workingDir || ''}</span>
    </div>
  `;
}

function messagesTemplate() {
  return html`
    <div class="messages" id="msg-log" style="${() => state.activeId ? '' : 'display:none'}">
      ${() => {
        void state.msgVersion;
        const msgs = state.activeId ? (messages[state.activeId] || []) : [];
        requestAnimationFrame(() => scrollToBottom());
        return msgs.map(msg => {
          if (msg.role === 'user') {
            return html`<div class="msg user">${msg.content}</div>`;
          }
          if (msg.role === 'error') {
            return html`<div class="msg error-msg">${msg.content}</div>`;
          }
          return html`<div class="msg assistant">${() => {
            void state.msgVersion;
            return msg.content;
          }}${() => {
            void state.msgVersion;
            return msg.streaming ? html`<span class="cursor"></span>` :
                   msg.cancelled ? html`<span class="cancelled-label"> Cancelled</span>` : html``;
          }}${() => {
            void state.msgVersion;
            return msg.tools.map(tool => toolCallTemplate(tool));
          }}</div>`;
        });
      }}
    </div>
    <div class="empty-state" style="${() => state.activeId ? 'display:none' : ''}">
      Create or select a conversation
    </div>
  `;
}

function toolCallTemplate(tool: ToolCall) {
  const ui = reactive({ expanded: tool.expanded });
  return html`
    <div class="${() => 'tool-call' + (ui.expanded ? ' expanded' : '')}">
      <div class="tool-hdr" @click="${() => { ui.expanded = !ui.expanded; tool.expanded = ui.expanded; }}">
        <span class="${() => 'icon icon-chevron-right arrow' + (ui.expanded ? ' open' : '')}"></span>
        <span class="tname">${tool.name}</span>
        <span class="${'tstatus' + (tool.output === null ? '' : tool.isError ? ' error' : ' done')}">
          ${tool.output === null ? 'running...' : tool.isError ? 'error' : 'done'}
        </span>
      </div>
      <div class="${() => 'tool-body' + (ui.expanded ? ' open' : '')}">
        <div class="tool-label">Input</div>
        <pre>${truncate(tool.input, 2000)}</pre>
        ${tool.output !== null ? html`
          <div class="tool-label">Output</div>
          <pre class="${tool.isError ? 'terr' : ''}">${truncate(tool.output, 2000)}</pre>
        ` : html``}
      </div>
    </div>
  `;
}

function inputTemplate() {
  return html`
    <div class="input-area">
      <textarea id="msg-input" rows="1"
        placeholder="${() => !state.activeId ? 'Create a session to start...' :
                           isRunning() ? 'Send a follow-up...' : 'Send a message...'}"
        disabled="${() => !state.activeId ? 'disabled' : false}"
        class="${() => isRunning() ? 'running' : ''}"
        @keydown="${(e: KeyboardEvent) => {
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
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
  </div>
`(document.getElementById('app')!);

invoke('list_conversations', {});
