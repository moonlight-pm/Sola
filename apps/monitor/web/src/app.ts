import { html, reactive } from '@arrow-js/core';
import { on } from '@sola/ipc';

// --- Types ---

interface BusMessage {
  id: string;
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
});

let pauseBuffer: BusMessage[] = [];
const seenTopics = new Set<string>();

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

function previewPayload(msg: BusMessage): string {
  if (msg.payload != null) {
    const str = JSON.stringify(msg.payload);
    return str.length > 80 ? str.slice(0, 80) + '\u2026' : str;
  }
  if (msg.rawHex) return `[hex: ${msg.rawHex.slice(0, 40)}\u2026]`;
  return '';
}

// --- Message handling ---

function addMessage(msg: BusMessage) {
  seenTopics.add(msg.topic);
  updateTopicDropdown();

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
    state.selectedId = msg.id;
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
          (msg) => html`
            <div
              class="${`message-row${state.selectedId === msg.id ? ' selected' : ''}`}"
              data-category="${categoryOf(msg.topic)}"
              @click="${() => selectMessage(msg)}"
            >
              <span class="cell time">${formatTime(msg.timestamp)}</span>
              <span class="cell topic">${msg.topic}</span>
              <span class="cell source">${msg.source || '\u2014'}</span>
              <span class="cell sticky">${msg.sticky ? html`<span class="dot"></span>` : ''}</span>
              <span class="cell preview">${previewPayload(msg)}</span>
            </div>
          `
        )}
    </div>

    <div class="${() => `detail-pane${state.selectedMessage ? '' : ' hidden'}`}">
      <div class="detail-header">
        <span
          class="detail-topic"
          style="${() => `color: var(--topic-${state.selectedMessage ? categoryOf(state.selectedMessage.topic) : 'unknown'})`}"
        >
          ${() => state.selectedMessage?.topic || ''}
        </span>
        <span class="detail-meta">
          ${() => {
            const m = state.selectedMessage;
            if (!m) return '';
            const parts: string[] = [];
            if (m.source) parts.push(m.source);
            if (m.sticky) parts.push('sticky');
            parts.push(m.id);
            return parts.join(' \u00b7 ');
          }}
        </span>
        <button class="detail-close" @click="${() => selectMessage(null)}">\u00d7</button>
      </div>
      <div class="detail-body">
        ${() => {
          const m = state.selectedMessage;
          if (!m) return '';
          if (m.payload != null) return JSON.stringify(m.payload, null, 2);
          if (m.rawHex) return `[raw hex]\n${m.rawHex}`;
          return '(no payload)';
        }}
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
}
