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
  tools: ToolCall[];
}

const state = reactive({
  sessions: [] as Session[],
  messages: {} as Record<string, Message[]>,
  activeId: null as string | null,
  editingId: null as string | null,
  editingTitle: false,
  searchQuery: '',
});

// Track what's rendered to avoid unnecessary DOM rebuilds
let renderedMsgCount = -1;
let renderedActiveId: string | null = null;
let renderedLastContent = '';
let renderedLastToolCount = -1;

// ── Events from Rust ─────────────────────────────────────────────────────────

on('session_state', (ev: any) => {
  let s = state.sessions.find((x: Session) => x.id === ev.session_id);
  if (!s) {
    s = {
      id: ev.session_id,
      name: ev.name || null,
      status: ev.status,
      firstPrompt: null,
      workingDir: ev.working_dir || null,
    };
    state.sessions.push(s);
    state.messages[ev.session_id] = [];
    state.activeId = ev.session_id;
    focusInput();
  } else {
    s.status = ev.status;
    if (ev.name) s.name = ev.name;
    if (ev.working_dir) s.workingDir = ev.working_dir;
  }
  updateInputState();
});

on('message_start', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (m) { m.push({ role: 'assistant', content: '', streaming: true, tools: [] }); renderMessages(); }
});

on('message_delta', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') { last.content += ev.text; renderMessages(); }
});

on('message_end', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) { last.streaming = false; renderMessages(); }
});

on('tool_start', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last && last.role === 'assistant') {
    last.tools.push({ name: ev.tool_name, input: ev.tool_input, output: null, isError: false, expanded: false });
    renderMessages();
  }
});

on('tool_end', (ev: any) => {
  const m = state.messages[ev.session_id];
  if (!m) return;
  const last = m[m.length - 1];
  if (last) {
    const t = last.tools.find((t: ToolCall) => t.name === ev.tool_name && t.output === null);
    if (t) { t.output = ev.result; t.isError = ev.is_error; renderMessages(); }
  }
});

on('error', (ev: any) => {
  const sid = ev.session_id || state.activeId;
  if (sid) {
    if (!state.messages[sid]) state.messages[sid] = [];
    state.messages[sid].push({ role: 'error', content: ev.message, streaming: false, tools: [] });
    renderMessages();
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

function focusInput(): void {
  requestAnimationFrame(() => {
    const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
    if (ta && !ta.disabled) ta.focus();
  });
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
  renderMessages();
  updateInputState();
}

async function showNewDialog(): Promise<void> {
  const existing = document.querySelector('.overlay');
  if (existing) existing.remove();

  const overlay = el('div', 'overlay');
  overlay.addEventListener('click', (e) => { if (e.target === overlay) overlay.remove(); });

  const dialog = el('div', 'dialog');
  dialog.appendChild(el('h3', undefined, 'New Session'));

  const fieldLabel = el('div', 'field-label', 'WORKING DIRECTORY');
  dialog.appendChild(fieldLabel);

  const input = document.createElement('input');
  input.type = 'text';
  input.value = '~';
  input.placeholder = '~/path/to/project';
  dialog.appendChild(input);

  const status = el('div', 'path-status');
  dialog.appendChild(status);

  function updateStatus(path: string): void {
    status.textContent = '';
    status.className = 'path-status';
    if (path.trim()) {
      status.classList.add('valid');
      const checkIcon = el('span', 'icon icon-check');
      checkIcon.style.marginRight = '6px';
      status.appendChild(checkIcon);
      status.appendChild(document.createTextNode(path));
    }
  }

  input.addEventListener('input', () => updateStatus(input.value));
  updateStatus(input.value);

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') { await createSession(input.value); overlay.remove(); }
    if (e.key === 'Escape') overlay.remove();
  });

  const btns = el('div', 'dialog-btns');
  const cancelBtn = el('button', 'dbtn-cancel', 'Cancel');
  cancelBtn.addEventListener('click', () => overlay.remove());
  btns.appendChild(cancelBtn);
  const startBtn = el('button', 'dbtn-start', 'Start Session');
  startBtn.addEventListener('click', async () => { await createSession(input.value); overlay.remove(); });
  btns.appendChild(startBtn);
  dialog.appendChild(btns);

  overlay.appendChild(dialog);
  document.body.appendChild(overlay);
  requestAnimationFrame(() => { input.focus(); input.select(); });
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
  renderHeader();
}

function startTitleEdit(): void {
  state.editingTitle = true;
  renderHeader();
}

async function finishTitleEdit(name: string): Promise<void> {
  state.editingTitle = false;
  if (name.trim() && state.activeId) {
    const s = activeSession();
    if (s) s.name = name.trim();
    await invoke('rename_conversation', { session_id: state.activeId, name: name.trim() });
  }
  renderHeader();
}

// ── Render: Sidebar ──────────────────────────────────────────────────────────

function renderSidebar(): void {
  const list = convoList;
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
      item.addEventListener('click', () => {
        state.activeId = s.id;
        invalidateRender();
        focusInput();
      });
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

// ── Render: Header ───────────────────────────────────────────────────────────

function renderHeader(): void {
  headerBar.textContent = '';
  const s = activeSession();
  if (!s) { headerBar.style.display = 'none'; return; }
  headerBar.style.display = '';

  // Left: title + edit button
  const left = el('div', 'header-left');
  if (state.editingTitle) {
    const inp = document.createElement('input');
    inp.className = 'header-title-input';
    inp.value = s.name || '';
    inp.addEventListener('blur', (e) => finishTitleEdit((e.target as HTMLInputElement).value));
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') finishTitleEdit((e.target as HTMLInputElement).value);
      if (e.key === 'Escape') { state.editingTitle = false; renderHeader(); }
    });
    left.appendChild(inp);
    requestAnimationFrame(() => { inp.focus(); inp.select(); });
  } else {
    left.appendChild(el('span', 'header-title', s.name || 'Untitled'));
    const editBtn = el('button', 'header-edit-btn');
    editBtn.appendChild(el('span', 'icon icon-pencil'));
    editBtn.addEventListener('click', startTitleEdit);
    left.appendChild(editBtn);
  }
  headerBar.appendChild(left);

  // Right: cwd
  if (s.workingDir) {
    headerBar.appendChild(el('span', 'header-cwd', s.workingDir));
  }
}

// ── Render: Messages ─────────────────────────────────────────────────────────

function renderMessages(): void {
  // If session changed, full rebuild
  if (state.activeId !== renderedActiveId) {
    invalidateRender();
  }

  const msgs = state.activeId ? (state.messages[state.activeId] || []) : [];

  if (!state.activeId) {
    msgLog.textContent = '';
    emptyState.style.display = '';
    msgLog.style.display = 'none';
    return;
  }

  emptyState.style.display = 'none';
  msgLog.style.display = '';

  // Full rebuild if session changed
  if (state.activeId !== renderedActiveId) {
    msgLog.textContent = '';
    renderedMsgCount = 0;
    renderedActiveId = state.activeId;
    renderedLastContent = '';
    renderedLastToolCount = -1;
  }

  // Append new messages
  for (let i = renderedMsgCount; i < msgs.length; i++) {
    const msg = msgs[i];
    if (msg.role === 'user') {
      msgLog.appendChild(el('div', 'msg user', msg.content));
    } else if (msg.role === 'error') {
      msgLog.appendChild(el('div', 'msg error-msg', msg.content));
    } else {
      const div = el('div', 'msg assistant');
      div.id = 'assistant-msg-' + i;
      div.appendChild(document.createTextNode(msg.content));
      if (msg.streaming) div.appendChild(el('span', 'cursor'));
      appendToolCalls(div, msg);
      msgLog.appendChild(div);
    }
  }
  renderedMsgCount = msgs.length;

  // Update last assistant message (streaming content + tools)
  if (msgs.length > 0) {
    const lastMsg = msgs[msgs.length - 1];
    if (lastMsg.role === 'assistant') {
      const lastDiv = document.getElementById('assistant-msg-' + (msgs.length - 1));
      if (lastDiv && (lastMsg.content !== renderedLastContent || lastMsg.tools.length !== renderedLastToolCount)) {
        lastDiv.textContent = '';
        lastDiv.appendChild(document.createTextNode(lastMsg.content));
        if (lastMsg.streaming) lastDiv.appendChild(el('span', 'cursor'));
        appendToolCalls(lastDiv, lastMsg);
        renderedLastContent = lastMsg.content;
        renderedLastToolCount = lastMsg.tools.length;
      }
    }
  }

  scrollToBottom();
}

function appendToolCalls(div: HTMLElement, msg: Message): void {
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
}

// ── Render: Input State ──────────────────────────────────────────────────────

function updateInputState(): void {
  const ta = document.getElementById('msg-input') as HTMLTextAreaElement | null;
  if (!ta) return;
  const running = isRunning();
  ta.disabled = running;
  ta.placeholder = running ? 'Agent is working...' : 'Send a message...';
  ta.classList.toggle('running', running);

  // Update button
  const btnArea = document.getElementById('input-btn-area');
  if (btnArea) {
    btnArea.textContent = '';
    if (running) {
      const btn = el('button', 'btn-cancel');
      btn.appendChild(el('span', 'icon icon-square'));
      btn.addEventListener('click', () => invoke('cancel', { session_id: state.activeId }));
      btnArea.appendChild(btn);
    } else {
      const btn = el('button', 'btn-send');
      btn.appendChild(el('span', 'icon icon-send'));
      btn.addEventListener('click', () => sendMessage());
      btnArea.appendChild(btn);
    }
  }
}

function invalidateRender(): void {
  renderedActiveId = null;
  renderedMsgCount = -1;
  renderedLastContent = '';
  renderedLastToolCount = -1;
}

// ── Mount ────────────────────────────────────────────────────────────────────

const appEl = document.getElementById('app')!;
const container = el('div', 'app');

// Sidebar
const sidebar = el('div', 'sidebar');
const toolbar = el('div', 'sidebar-toolbar');
const searchWrap = el('div', 'search-wrap');
searchWrap.appendChild(el('span', 'icon icon-search search-icon'));
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

// Main area — persistent structure
const mainArea = el('div', 'main');

// Header bar
const headerBar = el('div', 'header-bar');
headerBar.style.display = 'none';
mainArea.appendChild(headerBar);

// Empty state
const emptyState = el('div', 'empty-state', 'Create or select a conversation');
mainArea.appendChild(emptyState);

// Message log (persistent, content updated incrementally)
const msgLog = el('div', 'messages');
msgLog.id = 'msg-log';
msgLog.style.display = 'none';
mainArea.appendChild(msgLog);

// Input area (persistent, never recreated)
const inputArea = el('div', 'input-area');
const textarea = document.createElement('textarea') as HTMLTextAreaElement;
textarea.id = 'msg-input';
textarea.rows = 1;
textarea.placeholder = 'Send a message...';
textarea.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
});
textarea.addEventListener('input', () => {
  textarea.style.height = 'auto';
  textarea.style.height = Math.min(textarea.scrollHeight, 200) + 'px';
});
inputArea.appendChild(textarea);
const btnArea = el('div');
btnArea.id = 'input-btn-area';
const sendBtn = el('button', 'btn-send');
sendBtn.appendChild(el('span', 'icon icon-send'));
sendBtn.addEventListener('click', () => sendMessage());
btnArea.appendChild(sendBtn);
inputArea.appendChild(btnArea);
mainArea.appendChild(inputArea);

container.appendChild(mainArea);
appEl.appendChild(container);

// Sidebar re-render on interval (lightweight — only sidebar list)
setInterval(() => {
  renderSidebar();
  renderHeader();
  updateInputState();
}, 100);
