import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import { createTabSidebar, type TabItem } from './tabs.js';
import { createAddressBar } from './address.js';

// --- Reactive state ---

const state = createStore({
  tabs: [] as TabItem[],
  activeTabId: null as string | null,
  addressValue: '',
  suggestions: [] as Array<{ url: string; title: string; visits: number }>,
  downloads: [] as Array<{ id: string; filename: string; progress: number }>,
});

// --- Tab management (synchronous, fire-and-forget IPC) ---

let nextTabNum = 1;

function createTab(url?: string, activate: boolean = true): string {
  const tabId = `tab-${nextTabNum++}`;
  const newTab = { id: tabId, url: url || '', title: '', loading: true };
  state.tabs = [...state.tabs, newTab];
  if (activate) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
  console.log('[browser] createTab', tabId, 'total tabs:', state.tabs.length);
  // Fire-and-forget: tell Rust to create the WebView
  invoke('create_tab', { tabId, url, activate });
  return tabId;
}

function closeTab(tabId: string): void {
  const idx = state.tabs.findIndex(t => t.id === tabId);
  if (idx === -1) return;

  state.tabs = state.tabs.filter(t => t.id !== tabId);

  if (state.activeTabId === tabId) {
    if (state.tabs.length > 0) {
      const newIdx = Math.min(idx, state.tabs.length - 1);
      switchTab(state.tabs[newIdx].id);
    } else {
      state.activeTabId = null;
      state.addressValue = '';
    }
  }
  // Fire-and-forget
  invoke('close_tab', { tabId });
}

function switchTab(tabId: string): void {
  if (state.activeTabId === tabId) return;
  state.activeTabId = tabId;
  const tab = state.tabs.find(t => t.id === tabId);
  if (tab) state.addressValue = tab.url;
  // Fire-and-forget
  invoke('switch_tab', { tabId });
}

function navigate(input: string): void {
  const url = looksLikeUrl(input)
    ? (input.startsWith('http') ? input : `https://${input}`)
    : `https://kagi.com/search?q=${encodeURIComponent(input)}`;
  state.addressValue = url;
  state.suggestions = [];
  invoke('navigate', { url });
}

function goBack(): void { invoke('go_back'); }
function goForward(): void { invoke('go_forward'); }
function doReload(): void { invoke('reload'); }

function searchHistory(value: string): void {
  if (!value || value.length < 2) {
    state.suggestions = [];
    return;
  }
  invoke('history_search', { query: value }).then((results: any) => {
    state.suggestions = results || [];
  });
}

function looksLikeUrl(input: string): boolean {
  return /^https?:\/\//.test(input)
    || /^localhost(:\d+)?/.test(input)
    || /^[\w-]+\.[\w.-]+/.test(input);
}

// --- Events from Rust ---

on('tab_title_changed', ({ tabId, title }: any) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, title } : t
  );
});

on('tab_url_changed', ({ tabId, url }: any) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, url } : t
  );
  if (tabId === state.activeTabId) {
    state.addressValue = url;
  }
});

on('tab_load_changed', ({ tabId, loading }: any) => {
  state.tabs = state.tabs.map(t =>
    t.id === tabId ? { ...t, loading } : t
  );
});

on('bus_new_tab', ({ tabId, url, activate }: any) => {
  // Bus-initiated tab — Rust already created the WebView
  state.tabs = [...state.tabs, {
    id: tabId,
    url: url || '',
    title: '',
    loading: true,
  }];
  if (activate !== false) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
});

on('tab_closed', ({ tabId, nextTabId }: any) => {
  // Bus-initiated close (Super+W) — Rust already switched the WebView
  state.tabs = state.tabs.filter(t => t.id !== tabId);
  if (state.activeTabId === tabId) {
    state.activeTabId = nextTabId || (state.tabs.length > 0 ? state.tabs[state.tabs.length - 1].id : null);
    const tab = state.tabs.find(t => t.id === state.activeTabId);
    if (tab) state.addressValue = tab.url;
  }
});

on('bus_focus_address', () => {
  const input = document.querySelector('.address-input') as HTMLInputElement | null;
  if (input) {
    input.focus();
    input.select();
  }
});

on('download_started', ({ id, filename }: any) => {
  state.downloads = [...state.downloads, { id, filename, progress: 0 }];
});

on('download_progress', ({ id, progress }: any) => {
  state.downloads = state.downloads.map(d =>
    d.id === id ? { ...d, progress } : d
  );
});

on('download_finished', ({ id }: any) => {
  setTimeout(() => {
    state.downloads = state.downloads.filter(d => d.id !== id);
  }, 3000);
});

// --- App entry point ---

export async function createApp(root: HTMLElement): Promise<void> {
  // Create mount targets
  const sidebarTarget = document.createElement('div');
  sidebarTarget.style.display = 'contents';
  const topbarTarget = document.createElement('div');
  topbarTarget.style.display = 'contents';

  // Layout shell
  html`
    <div class="top-bar">
      <button class="nav-btn" @click="${goBack}"><span class="icon icon-arrow-left"></span></button>
      <button class="nav-btn" @click="${goForward}"><span class="icon icon-arrow-right"></span></button>
      <button class="nav-btn" @click="${doReload}"><span class="icon icon-rotate-cw"></span></button>
    </div>
    ${() => state.downloads.map(d =>
      html`<div class="download-toast">${() => d.filename} — ${() => Math.round(d.progress * 100)}%</div>`
    )}
  `(root);

  // Insert component mount targets
  root.prepend(sidebarTarget);
  const topBar = root.querySelector('.top-bar')!;
  topbarTarget.style.flex = '1';
  topBar.appendChild(topbarTarget);

  // Mount components
  createTabSidebar({
    tabs: () => state.tabs,
    activeTabId: () => state.activeTabId,
    onSelect: switchTab,
    onClose: closeTab,
    onCreate: () => createTab(),
  }, sidebarTarget);

  createAddressBar({
    value: () => state.addressValue,
    suggestions: () => state.suggestions,
    onNavigate: navigate,
    onInput: (value) => {
      state.addressValue = value;
      searchHistory(value);
    },
    onBlur: () => { state.suggestions = []; },
  }, topbarTarget);

  // Restore session
  const session = await invoke('ready');
  if (session.tabs && session.tabs.length > 0) {
    state.tabs = session.tabs;
    state.activeTabId = session.activeTabId || session.tabs[0].id;
    state.addressValue = state.tabs.find(t => t.id === state.activeTabId)?.url || '';
  }
  if (state.tabs.length === 0) {
    createTab('about:blank');
  }
}
