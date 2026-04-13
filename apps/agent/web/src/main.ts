import { reactive, html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';

// ── State ────────────────────────────────────────────────────────────────────

interface Session {
  id: string;
  name: string | null;
  status: string;
  firstPrompt: string | null;
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
  tools: ToolCall[];
}

const state = reactive({
  sessions: [] as Session[],
  messages: {} as Record<string, Message[]>,
  activeId: null as string | null,
  editingId: null as string | null,
  searchQuery: '',
});

// ── Events from Rust ─────────────────────────────────────────────────────────

on('session_state', (ev: any) => {
  let s = state.sessions.find((x: Session) => x.id === ev.session_id);
  if (!s) {
    state.sessions.push({ id: ev.session_id, name: null, status: ev.status, firstPrompt: null });
    state.messages[ev.session_id] = [];
    if (!state.activeId) state.activeId = ev.session_id;
  } else {
    s.status = ev.status;
  }
});

on('message_start', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (m) m.push({ role: 'assistant', content: '', streaming: true, tools: [] });
});

on('message_delta', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') last.content += ev.text;
});

on('message_end', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) last.streaming = false;
});

on('tool_start', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') {
    last.tools.push({ name: ev.tool_name, input: ev.tool_input, output: null, isError: false, expanded: false });
  }
});

on('tool_end', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) {
    const t = last.tools.find((t: ToolCall) => t.name === ev.tool_name && t.output === null);
    if (t) { t.output = ev.result; t.isError = ev.is_error; }
  }
});

on('error', (ev: any) => {
  const sid = ev.session_id || state.activeId;
  if (sid) {
    if (!state.messages[sid]) state.messages[sid] = [];
    state.messages[sid].push({ role: 'error', content: ev.message, streaming: false, tools: [] });
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

function scrollToBottom(): void {
  requestAnimationFrame(() => {
    const el = document.getElementById('msg-log');
    if (el) {
      const near = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
      if (near) el.scrollTop = el.scrollHeight;
    }
  });
}

function el(tag: string, cls?: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

// ── Actions ──────────────────────────────────────────────────────────────────

async function sendMessage(): Promise<void> {
  const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
  if (!ta) return;
  const text = ta.value.trim();
  if (!text || !state.activeId || isRunning()) return;

  if (!state.messages[state.activeId]) state.messages[state.activeId] = [];
  state.messages[state.activeId].push({ role: 'user', content: text, streaming: false, tools: [] });

  const s = state.sessions.find((x: Session) => x.id === state.activeId);
  if (s && !s.firstPrompt) s.firstPrompt = text;

  await invoke('send_message', { session_id: state.activeId, text });
  ta.value = '';
  ta.style.height = 'auto';
}

async function showNewDialog(): Promise<void> {
  const existing = document.querySelector('.overlay');
  if (existing) existing.remove();

  const overlay = el('div', 'overlay');
  overlay.addEventListener('click', (e) => { if (e.target === overlay) overlay.remove(); });

  const dialog = el('div', 'dialog');
  dialog.appendChild(el('h3', undefined, 'New Session'));

  const input = document.createElement('input');
  input.type = 'text';
  input.placeholder = '/path/to/project';
  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') { await createSession(input.value); overlay.remove(); }
    if (e.key === 'Escape') overlay.remove();
  });
  dialog.appendChild(input);

  const btns = el('div', 'dialog-btns');
  const cancelBtn = el('button', 'dbtn-cancel', 'Cancel');
  cancelBtn.addEventListener('click', () => overlay.remove());
  btns.appendChild(cancelBtn);
  const createBtn = el('button', 'dbtn-create', 'Create');
  createBtn.addEventListener('click', async () => { await createSession(input.value); overlay.remove(); });
  btns.appendChild(createBtn);
  dialog.appendChild(btns);
  overlay.appendChild(dialog);
  document.body.appendChild(overlay);
  requestAnimationFrame(() => input.focus());
}

async function createSession(dir: string): Promise<void> {
  if (!dir.trim()) return;
  await invoke('new_session', { working_dir: dir.trim() });
}

function startRename(id: string): void {
  state.editingId = id;
}

async function finishRename(id: string, name: string): Promise<void> {
  state.editingId = null;
  if (name.trim()) {
    const s = state.sessions.find((x: Session) => x.id === id);
    if (s) s.name = name.trim();
    await invoke('rename_conversation', { session_id: id, name: name.trim() });
  }
}

// ── Render ────────────────────────────────────────────────────────────────────

function renderSidebar(list: HTMLElement): void {
  list.textContent = '';
  const query = state.searchQuery.toLowerCase();
  const filtered = state.sessions.filter((s: Session) => {
    if (!query) return true;
    return (s.name || '').toLowerCase().includes(query) ||
           (s.firstPrompt || '').toLowerCase().includes(query);
  });
  const running = filtered.filter((s: Session) => s.status === 'running');
  const other = filtered.filter((s: Session) => s.status !== 'running');

  function addGroup(label: string, items: Session[]): void {
    if (!items.length) return;
    list.appendChild(el('div', 'group-label', label));
    for (const s of items) {
      const item = el('div', 'convo-item' + (s.id === state.activeId ? ' active' : ''));
      item.addEventListener('click', () => { state.activeId = s.id; });
      item.addEventListener('dblclick', () => startRename(s.id));
      item.appendChild(el('span', 'dot ' + s.status));

      if (state.editingId === s.id) {
        const inp = document.createElement('input');
        inp.className = 'convo-name-input';
        inp.value = s.name || truncate(s.firstPrompt, 30) || 'New session';
        inp.addEventListener('blur', (e) => finishRename(s.id, (e.target as HTMLInputElement).value));
        inp.addEventListener('keydown', (e) => {
          if (e.key === 'Enter') finishRename(s.id, (e.target as HTMLInputElement).value);
          if (e.key === 'Escape') { state.editingId = null; }
        });
        item.appendChild(inp);
        requestAnimationFrame(() => { inp.focus(); inp.select(); });
      } else {
        item.appendChild(el('span', 'convo-name', s.name || truncate(s.firstPrompt, 30) || 'New session'));
      }
      list.appendChild(item);
    }
  }
  addGroup('Running', running);
  addGroup('Sessions', other);
}

function renderMain(area: HTMLElement): void {
  area.textContent = '';

  if (!state.activeId || !state.messages[state.activeId]) {
    area.appendChild(el('div', 'empty-state', 'Create or select a conversation'));
    return;
  }

  const log = el('div', 'messages');
  log.id = 'msg-log';
  const msgs = state.messages[state.activeId] || [];

  for (const msg of msgs) {
    if (msg.role === 'user') {
      log.appendChild(el('div', 'msg user', msg.content));
    } else if (msg.role === 'error') {
      log.appendChild(el('div', 'msg error-msg', msg.content));
    } else {
      const div = el('div', 'msg assistant');
      div.appendChild(document.createTextNode(msg.content));
      if (msg.streaming) div.appendChild(el('span', 'cursor'));

      for (const tool of msg.tools) {
        const tc = el('div', 'tool-call' + (tool.expanded ? ' expanded' : ''));
        const arrow = el('span', 'icon icon-chevron-right arrow' + (tool.expanded ? ' open' : ''));
        const hdr = el('div', 'tool-hdr');
        hdr.appendChild(arrow);
        hdr.appendChild(el('span', 'tname', tool.name));
        const statusCls = tool.output === null ? '' : (tool.isError ? ' error' : ' done');
        const statusText = tool.output === null ? 'running...' : (tool.isError ? 'error' : 'done');
        hdr.appendChild(el('span', 'tstatus' + statusCls, statusText));

        const body = el('div', 'tool-body' + (tool.expanded ? ' open' : ''));
        body.appendChild(el('div', 'tool-label', 'Input'));
        const inp = el('pre'); inp.textContent = truncate(tool.input, 2000); body.appendChild(inp);
        if (tool.output !== null) {
          body.appendChild(el('div', 'tool-label', 'Output'));
          const outp = el('pre', tool.isError ? 'terr' : '');
          outp.textContent = truncate(tool.output, 2000);
          body.appendChild(outp);
        }

        hdr.addEventListener('click', () => {
          tool.expanded = !tool.expanded;
          arrow.className = 'icon icon-chevron-right arrow' + (tool.expanded ? ' open' : '');
          body.className = 'tool-body' + (tool.expanded ? ' open' : '');
          tc.className = 'tool-call' + (tool.expanded ? ' expanded' : '');
        });
        tc.appendChild(hdr);
        tc.appendChild(body);
        div.appendChild(tc);
      }
      log.appendChild(div);
    }
  }
  area.appendChild(log);

  const inputArea = el('div', 'input-area');
  const textarea = document.createElement('textarea') as HTMLTextAreaElement;
  textarea.id = 'msg-input';
  textarea.rows = 1;
  textarea.placeholder = isRunning() ? 'Agent is working...' : 'Send a message...';
  if (isRunning()) textarea.disabled = true;
  textarea.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
  });
  textarea.addEventListener('input', (e) => {
    const t = e.target as HTMLTextAreaElement;
    t.style.height = 'auto';
    t.style.height = Math.min(t.scrollHeight, 200) + 'px';
  });
  inputArea.appendChild(textarea);

  if (isRunning()) {
    textarea.classList.add('running');
    const btn = el('button', 'btn-cancel');
    btn.appendChild(el('span', 'icon icon-square'));
    btn.addEventListener('click', () => invoke('cancel', { session_id: state.activeId }));
    inputArea.appendChild(btn);
  } else {
    const btn = el('button', 'btn-send');
    btn.appendChild(el('span', 'icon icon-send'));
    btn.addEventListener('click', () => sendMessage());
    inputArea.appendChild(btn);
  }
  area.appendChild(inputArea);
  scrollToBottom();
}

// ── Mount ────────────────────────────────────────────────────────────────────

const app = document.getElementById('app')!;
const container = el('div', 'app');

// Sidebar
const sidebar = el('div', 'sidebar');

// Toolbar: search + new button on same row
const toolbar = el('div', 'sidebar-toolbar');
const searchWrap = el('div', 'search-wrap');
const searchIcon = el('span', 'icon icon-search search-icon');
searchWrap.appendChild(searchIcon);
const searchBox = document.createElement('input');
searchBox.type = 'text';
searchBox.placeholder = 'Search...';
searchBox.addEventListener('input', () => { state.searchQuery = searchBox.value; });
searchWrap.appendChild(searchBox);
toolbar.appendChild(searchWrap);
const newBtn = el('button', 'btn-new');
newBtn.appendChild(el('span', 'icon icon-plus'));
newBtn.addEventListener('click', () => showNewDialog());
toolbar.appendChild(newBtn);
sidebar.appendChild(toolbar);

const convoList = el('div', 'convo-list');
sidebar.appendChild(convoList);
container.appendChild(sidebar);

// Main area
const mainArea = el('div', 'main');
container.appendChild(mainArea);
app.appendChild(container);

// Reactive re-render when state changes
// We use a simple polling approach since Arrow.js reactive doesn't deep-watch arrays
setInterval(() => {
  renderSidebar(convoList);
  renderMain(mainArea);
}, 100);
