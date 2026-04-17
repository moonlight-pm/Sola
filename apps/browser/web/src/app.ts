import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import { createTabSidebar, type TabItem } from './tabs.js';
import { createTopBar } from './address.js';
import { createToasts } from './toasts.js';

// --- Reactive state ---

const state = createStore({
  tabs: [] as TabItem[],
  activeTabId: null as string | null,
  addressValue: '',
  addressFocusNonce: 0,
  suggestions: [] as Array<{ url: string; title: string; visits: number }>,
  downloads: [] as Array<{ id: string; filename: string; progress: number }>,
});

// --- Tab management (synchronous, fire-and-forget IPC) ---

let nextTabNum = 1;

function createTab(url?: string, activate: boolean = true): string {
  const tabId = `tab-${nextTabNum++}`;
  state.tabs = [...state.tabs, { id: tabId, url: url || '', title: '', loading: true }];
  if (activate) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
  invoke('create_tab', { tabId, url, activate });
  if (!url && activate) state.addressFocusNonce++;
  return tabId;
}

function closeTab(tabId: string): void {
  const idx = state.tabs.findIndex(t => t.id === tabId);
  if (idx === -1) return;

  const remaining = state.tabs.filter(t => t.id !== tabId);
  state.tabs = remaining;

  if (state.activeTabId === tabId) {
    if (remaining.length > 0) {
      const newIdx = Math.min(idx, remaining.length - 1);
      switchTab(remaining[newIdx].id);
    } else {
      state.activeTabId = null;
      state.addressValue = '';
    }
  }
  invoke('close_tab', { tabId });
}

function switchTab(tabId: string): void {
  if (state.activeTabId === tabId) return;
  state.activeTabId = tabId;
  const tab = state.tabs.find(t => t.id === tabId);
  if (tab) state.addressValue = tab.url;
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

function parseTabNum(id: string): number {
  const m = id.match(/^tab-(\d+)$/);
  return m ? Number(m[1]) : 0;
}

// --- Events from Rust ---

on('tab_title_changed', ({ tabId, title }: any) => {
  state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, title } : t);
});

on('tab_url_changed', ({ tabId, url }: any) => {
  state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, url } : t);
  if (tabId === state.activeTabId) state.addressValue = url;
});

on('tab_load_changed', ({ tabId, loading }: any) => {
  state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, loading } : t);
});

on('bus_new_tab', (data: any) => {
  const { tabId, url, activate } = data;
  state.tabs = [...state.tabs, { id: tabId, url: url || '', title: '', loading: true }];
  if (activate !== false) {
    state.activeTabId = tabId;
    state.addressValue = url || '';
  }
  if (!url && activate !== false) state.addressFocusNonce++;
});

on('tab_closed', ({ tabId, nextTabId }: any) => {
  state.tabs = state.tabs.filter(t => t.id !== tabId);
  if (state.activeTabId === tabId) {
    state.activeTabId = nextTabId || (state.tabs.length > 0 ? state.tabs[state.tabs.length - 1].id : null);
    const tab = state.tabs.find(t => t.id === state.activeTabId);
    if (tab) state.addressValue = tab.url;
  }
});

on('bus_focus_address', () => { state.addressFocusNonce++; });

on('download_started', ({ id, filename }: any) => {
  state.downloads = [...state.downloads, { id, filename, progress: 0 }];
});

on('download_progress', ({ id, progress }: any) => {
  state.downloads = state.downloads.map(d => d.id === id ? { ...d, progress } : d);
});

on('download_finished', ({ id }: any) => {
  setTimeout(() => {
    state.downloads = state.downloads.filter(d => d.id !== id);
  }, 3000);
});

// --- App entry point ---

export async function createApp(root: HTMLElement): Promise<void> {
  // #app is the CSS grid container. Mount each region into its own
  // `display: contents` target so the actual .tab-sidebar / .top-bar /
  // toast elements are the real grid children. No reactive bindings
  // live at root level — Arrow.js does not mix well with externally-
  // appended siblings to its own reactive placeholders.
  const sidebarTarget = document.createElement('div');
  sidebarTarget.style.display = 'contents';
  const topbarTarget = document.createElement('div');
  topbarTarget.style.display = 'contents';
  const toastsTarget = document.createElement('div');
  toastsTarget.style.display = 'contents';
  root.append(sidebarTarget, topbarTarget, toastsTarget);

  createTabSidebar({
    tabs: () => state.tabs,
    activeTabId: () => state.activeTabId,
    onSelect: switchTab,
    onClose: closeTab,
    onCreate: () => createTab(),
  }, sidebarTarget);

  createTopBar({
    value: () => state.addressValue,
    suggestions: () => state.suggestions,
    enabled: () => state.activeTabId !== null,
    focusNonce: () => state.addressFocusNonce,
    onBack: goBack,
    onForward: goForward,
    onReload: doReload,
    onNavigate: navigate,
    onInput: (value) => { state.addressValue = value; searchHistory(value); },
    onBlur: () => { state.suggestions = []; },
  }, topbarTarget);

  createToasts({ downloads: () => state.downloads }, toastsTarget);

  // Restore session
  const session = await invoke('ready');
  if (session.tabs && session.tabs.length > 0) {
    state.tabs = session.tabs;
    state.activeTabId = session.activeTabId || session.tabs[0].id;
    state.addressValue = state.tabs.find(t => t.id === state.activeTabId)?.url || '';
    // Bump nextTabNum past any restored tab numbers so new ids don't collide.
    for (const t of state.tabs) {
      nextTabNum = Math.max(nextTabNum, parseTabNum(t.id) + 1);
    }
  }
  if (state.tabs.length === 0) {
    createTab('about:blank');
  }
}
