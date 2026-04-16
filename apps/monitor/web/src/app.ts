import { html, reactive } from '@arrow-js/core';
import { on } from '@sola/ipc';

// --- Types ---

interface BusMessage {
  msgId: string;
  timestamp: number;
  topic: string;
  sticky: boolean;
  source: string;
  payload: any;
  rawHex: string | null;
}

// --- Topic categories ---

const TOPIC_CATEGORIES: Record<string, string> = {
  Apps: 'lifecycle',
  LaunchApp: 'lifecycle',
  Shutdown: 'lifecycle',
  Composition: 'composition',
  Frame: 'composition',
  Focus: 'composition',
  SetWindowPolicy: 'window',
  OutputGeometry: 'window',
  MouseEntered: 'input',
  ShellKeyBindings: 'input',
  SetAppMenu: 'menu',
  MenuAction: 'menu',
  OpenUrl: 'browser',
};

function categoryOf(topic: string): string {
  return TOPIC_CATEGORIES[topic] || 'unknown';
}

// --- State ---

const MAX_MESSAGES = 5000;

const state = reactive({
  messages: [] as BusMessage[],
  filteredMessages: [] as BusMessage[],
  selectedId: null as string | null,
  selectedMessage: null as BusMessage | null,
  paused: false,
  filter: '',
  topicFilter: '',
  count: 0,
  autoScroll: true,
  stickyMessages: [] as BusMessage[],
  expandedStickyKey: null as string | null,
});

let pauseBuffer: BusMessage[] = [];
const seenTopics = new Set<string>();
const stickyMap = new Map<string, BusMessage>();

// --- Filtering ---

function applyFilter() {
  const filterLower = state.filter.toLowerCase();
  const topicFilter = state.topicFilter;

  state.filteredMessages = state.messages.filter((msg) => {
    if (topicFilter && msg.topic !== topicFilter) return false;
    if (filterLower) {
      const topicMatch = msg.topic.toLowerCase().includes(filterLower);
      const sourceMatch = msg.source.toLowerCase().includes(filterLower);
      const payloadMatch = msg.payload
        ? JSON.stringify(msg.payload).toLowerCase().includes(filterLower)
        : false;
      if (!topicMatch && !sourceMatch && !payloadMatch) return false;
    }
    return true;
  });
}

// --- Time formatting ---

function formatTime(ms: number): string {
  const d = new Date(ms);
  const h = String(d.getHours()).padStart(2, '0');
  const m = String(d.getMinutes()).padStart(2, '0');
  const s = String(d.getSeconds()).padStart(2, '0');
  const ms_ = String(d.getMilliseconds()).padStart(3, '0');
  return `${h}:${m}:${s}.${ms_}`;
}

// --- JSON syntax highlighting (Arrow.js templates) ---

function highlightedPreview(msg: BusMessage): any[] {
  if (msg.payload == null) {
    if (msg.rawHex) return [`[hex: ${msg.rawHex.slice(0, 40)}\u2026]`];
    return [];
  }
  return tokenizeJson(JSON.stringify(msg.payload));
}

function highlightedJson(obj: any): any[] {
  return tokenizeJson(JSON.stringify(obj, null, 2));
}

function tokenizeJson(json: string, maxChars?: number): any[] {
  const result: any[] = [];
  const re = /("(?:\\.|[^"\\])*")\s*(:)|("(?:\\.|[^"\\])*")|(true|false)|(null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
  let last = 0;
  let chars = 0;
  let match;
  while ((match = re.exec(json)) !== null) {
    if (match.index > last) {
      const plain = json.slice(last, match.index);
      chars += plain.length;
      if (maxChars && chars > maxChars) { result.push('\u2026'); return result; }
      result.push(plain);
    }
    const tokenLen = match[0].length;
    chars += tokenLen;
    if (maxChars && chars > maxChars) { result.push('\u2026'); return result; }
    if (match[1] && match[2]) {
      result.push(html`<span class="json-key">${match[1]}</span>`);
      result.push(':');
    } else if (match[3]) {
      result.push(html`<span class="json-string">${match[3]}</span>`);
    } else if (match[4]) {
      result.push(html`<span class="json-bool">${match[4]}</span>`);
    } else if (match[5]) {
      result.push(html`<span class="json-null">${match[5]}</span>`);
    } else if (match[6]) {
      result.push(html`<span class="json-number">${match[6]}</span>`);
    }
    last = re.lastIndex;
  }
  if (last < json.length) {
    result.push(json.slice(last));
  }
  return result;
}

// --- Message handling ---

function addMessage(msg: BusMessage) {
  seenTopics.add(msg.topic);
  updateTopicDropdown();

  if (msg.sticky) {
    const key = `${msg.topic}:${msg.source}`;
    stickyMap.set(key, msg);
    state.stickyMessages = Array.from(stickyMap.values());
  }

  if (state.paused) {
    pauseBuffer.push(msg);
    return;
  }

  state.messages = [...state.messages.slice(-(MAX_MESSAGES - 1)), msg];
  state.count = state.messages.length;
  applyFilter();
}

// --- Actions ---

function togglePause() {
  state.paused = !state.paused;
  if (!state.paused && pauseBuffer.length > 0) {
    state.messages = [...state.messages, ...pauseBuffer].slice(-MAX_MESSAGES);
    pauseBuffer = [];
    state.count = state.messages.length;
    applyFilter();
  }
}

function clearMessages() {
  state.messages = [];
  state.filteredMessages = [];
  state.count = 0;
  state.selectedId = null;
  state.selectedMessage = null;
}

function selectMessage(msg: BusMessage | null) {
  if (msg) {
    state.selectedId = msg.msgId;
    state.selectedMessage = msg;
  } else {
    state.selectedId = null;
    state.selectedMessage = null;
  }
}

// --- Scroll management ---

let listEl: HTMLElement | null = null;

function scrollToBottom() {
  if (listEl && state.autoScroll) {
    listEl.scrollTop = listEl.scrollHeight;
  }
}

function onListScroll() {
  if (!listEl) return;
  const atBottom = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 40;
  state.autoScroll = atBottom;
}

function jumpToBottom() {
  state.autoScroll = true;
  scrollToBottom();
}

// --- Topic dropdown ---

let selectEl: HTMLSelectElement | null = null;
const addedTopics = new Set<string>();

function updateTopicDropdown() {
  if (!selectEl) return;
  for (const topic of seenTopics) {
    if (!addedTopics.has(topic)) {
      addedTopics.add(topic);
      const option = document.createElement('option');
      option.value = topic;
      option.textContent = topic;
      selectEl.appendChild(option);
    }
  }
}

// --- Rendering ---

export async function createApp(root: HTMLElement) {
  on('bus_message', (msg: BusMessage) => {
    addMessage(msg);
    requestAnimationFrame(scrollToBottom);
  });

  const template = html`
    <div class="toolbar">
      <input
        type="text"
        placeholder="Filter messages\u2026"
        @input="${(e: Event) => {
          state.filter = (e.target as HTMLInputElement).value;
          applyFilter();
        }}"
      />
      <select
        id="topic-select"
        @change="${(e: Event) => {
          state.topicFilter = (e.target as HTMLSelectElement).value;
          applyFilter();
        }}"
      >
        <option value="">All topics</option>
      </select>
      <button
        class="${() => (state.paused ? 'active' : '')}"
        @click="${togglePause}"
      >
        ${() => (state.paused ? `Resume (${pauseBuffer.length})` : 'Pause')}
      </button>
      <button @click="${clearMessages}">Clear</button>
      <div class="spacer"></div>
      <span class="count">${() => state.count} msgs</span>
    </div>

    <div class="main-area">
      <div class="messages-panel">
        <div class="table-header">
          <span>Time</span>
          <span>Topic</span>
          <span>Source</span>
          <span>S</span>
          <span>Preview</span>
        </div>

        <div
          class="message-list"
          id="message-list"
          @scroll="${onListScroll}"
        >
          ${() =>
            state.filteredMessages.map(
              (msg) => {
                const selected = state.selectedId === msg.msgId;
                return html`
                  <div
                    class="${`message-row${selected ? ' selected' : ''}`}"
                    data-category="${categoryOf(msg.topic)}"
                    @click="${() => selectMessage(selected ? null : msg)}"
                  >
                    <span class="cell time">${formatTime(msg.timestamp)}</span>
                    <span class="cell topic">${msg.topic}</span>
                    <span class="cell source">${msg.source || '\u2014'}</span>
                    <span class="cell sticky">${msg.sticky ? html`<span class="dot"></span>` : ''}</span>
                    <span class="${() => state.selectedId === msg.msgId ? 'cell preview expanded' : 'cell preview'}">${() => state.selectedId === msg.msgId && msg.payload != null ? highlightedJson(msg.payload) : highlightedPreview(msg)}</span>
                  </div>
                `;
              }
            )}
        </div>
      </div>

      <div class="resize-handle" id="resize-handle"></div>
      <div class="sticky-panel" id="sticky-panel">
        <div class="sticky-header">Sticky State</div>
        <div class="sticky-list">
          ${() =>
            state.stickyMessages.map(
              (msg) => {
                const key = `${msg.topic}:${msg.source}`;
                const expanded = state.expandedStickyKey === key;
                return html`
                  <div class="sticky-entry" data-category="${categoryOf(msg.topic)}">
                    <div
                      class="${`sticky-item${expanded ? ' expanded' : ''}`}"
                      @click="${() => { state.expandedStickyKey = expanded ? null : key; }}"
                    >
                      <span class="sticky-item-topic">${msg.topic}</span>
                      <span class="sticky-item-source">${msg.source || ''}</span>
                    </div>
                    ${() => state.expandedStickyKey === key && msg.payload != null
                      ? html`<div class="sticky-detail">${highlightedJson(msg.payload)}</div>`
                      : ''}
                  </div>
                `;
              }
            )}
        </div>
      </div>
    </div>

    <div
      class="${() => `auto-scroll-indicator${!state.autoScroll ? ' visible' : ''}`}"
      @click="${jumpToBottom}"
    >
      \u2193 Auto-scroll paused \u2014 click to resume
    </div>
  `;

  template(root);

  listEl = document.getElementById('message-list');
  selectEl = document.getElementById('topic-select') as HTMLSelectElement;

  // Resize handle for sticky panel
  const handle = document.getElementById('resize-handle');
  const panel = document.getElementById('sticky-panel');
  if (handle && panel) {
    let dragging = false;
    handle.addEventListener('mousedown', (e) => {
      dragging = true;
      e.preventDefault();
    });
    window.addEventListener('mousemove', (e) => {
      if (!dragging) return;
      const width = window.innerWidth - e.clientX;
      panel.style.width = `${Math.max(120, Math.min(width, 600))}px`;
    });
    window.addEventListener('mouseup', () => { dragging = false; });
  }
}
