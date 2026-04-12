import { reactive, html } from './arrow.js';
import { renderTabs } from './tabs.js';
import { renderAddressBar } from './address.js';

// window.sola is injected by Rust via UserContentManager init script.
// It provides: sola.invoke(command, args), sola.on(event, cb), sola._emit(event, data)

// --- App State ---
export const state = reactive({
  tabs: [],
  activeTabId: null,
  addressValue: '',
  addressFocused: false,
  suggestions: [],
  downloads: [],
});

// --- Actions ---
export async function createTab(url, activate = true) {
  const result = await sola.invoke('create_tab', { url, activate });
  state.tabs = [...state.tabs, {
    id: result.tabId,
    url: url || '',
    title: 'New Tab',
    loading: false,
  }];
  if (activate) {
    state.activeTabId = result.tabId;
    state.addressValue = url || '';
  }
}

export async function closeTab(tabId) {
  await sola.invoke('close_tab', { tabId });
  state.tabs = state.tabs.filter(t => t.id !== tabId);
  if (state.activeTabId === tabId) {
    const remaining = state.tabs;
    if (remaining.length > 0) {
      await switchTab(remaining[remaining.length - 1].id);
    }
  }
}

export async function switchTab(tabId) {
  await sola.invoke('switch_tab', { tabId });
  state.activeTabId = tabId;
  const tab = state.tabs.find(t => t.id === tabId);
  if (tab) state.addressValue = tab.url;
}

export async function navigate(input) {
  const url = looksLikeUrl(input)
    ? (input.startsWith('http') ? input : `https://${input}`)
    : `https://kagi.com/search?q=${encodeURIComponent(input)}`;
  await sola.invoke('navigate', { url });
}

export async function goBack() { await sola.invoke('go_back'); }
export async function goForward() { await sola.invoke('go_forward'); }
export async function reload() { await sola.invoke('reload'); }

export async function searchHistory(query) {
  if (!query || query.length < 2) {
    state.suggestions = [];
    return;
  }
  const results = await sola.invoke('history_search', { query });
  state.suggestions = results || [];
}

function looksLikeUrl(input) {
  return /^https?:\/\//.test(input)
    || /^localhost(:\d+)?/.test(input)
    || /^[\w-]+\.[\w.-]+/.test(input);
}

// --- Events from Rust ---
sola.on('tab_title_changed', ({ tabId, title }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, title } : t
  );
});

sola.on('tab_url_changed', ({ tabId, url }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, url } : t
  );
  if (tabId === state.activeTabId) {
    state.addressValue = url;
  }
});

sola.on('tab_load_changed', ({ tabId, loading }) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, loading } : t
  );
});

sola.on('bus_new_tab', ({ tabId, url, activate }) => {
  // Tab WebView already created by Rust — just update frontend state
  state.tabs = [...state.tabs, {
    id: tabId,
    url: url || '',
    title: 'New Tab',
    loading: true,
  }];
  if (activate !== false) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
});

sola.on('bus_focus_address', () => {
  const input = document.querySelector('.address-input');
  if (input) {
    input.focus();
    input.select();
  }
});

sola.on('download_started', ({ id, filename }) => {
  state.downloads = [...state.downloads, { id, filename, progress: 0 }];
});

sola.on('download_progress', ({ id, progress }) => {
  state.downloads = state.downloads.map(d =>
    d.id === id ? { ...d, progress } : d
  );
});

sola.on('download_finished', ({ id }) => {
  setTimeout(() => {
    state.downloads = state.downloads.filter(d => d.id !== id);
  }, 3000);
});

// --- Render ---
function renderDownloads() {
  return html`${() => state.downloads.map(d =>
    html`<div class="download-toast">${() => d.filename} — ${() => Math.round(d.progress * 100)}%</div>`
  )}`;
}

function render() {
  const app = document.getElementById('app');
  app.append(
    renderTabs(),
    html`<div class="top-bar">
      <button class="nav-btn" @click="${goBack}">&#9664;</button>
      <button class="nav-btn" @click="${goForward}">&#9654;</button>
      <button class="nav-btn" @click="${reload}">&#8635;</button>
      ${renderAddressBar()}
    </div>`,
    renderDownloads(),
  );
}

// --- Init ---
async function init() {
  const session = await sola.invoke('ready');
  if (session.tabs && session.tabs.length > 0) {
    state.tabs = session.tabs;
    state.activeTabId = session.activeTabId || session.tabs[0].id;
    state.addressValue = state.tabs.find(t => t.id === state.activeTabId)?.url || '';
  }
  render();
  if (state.tabs.length === 0) {
    await createTab('about:blank');
  }
}

init();
