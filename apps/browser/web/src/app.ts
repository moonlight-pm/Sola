import { reactive, html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { renderTabs } from './tabs.js';
import { renderAddressBar } from './address.js';

// --- App State ---
export const state = reactive({
  tabs: [] as Array<{ id: string; url: string; title: string; loading: boolean }>,
  activeTabId: null as string | null,
  addressValue: '',
  addressFocused: false,
  suggestions: [] as Array<{ url: string; title: string; visits: number }>,
  downloads: [] as Array<{ id: string; filename: string; progress: number }>,
});

// --- Actions ---
export async function createTab(url?: string, activate: boolean = true): Promise<void> {
  const result = await invoke('create_tab', { url, activate });
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

export async function closeTab(tabId: string): Promise<void> {
  await invoke('close_tab', { tabId });
  state.tabs = state.tabs.filter(t => t.id !== tabId);
  if (state.activeTabId === tabId) {
    const remaining = state.tabs;
    if (remaining.length > 0) {
      await switchTab(remaining[remaining.length - 1].id);
    }
  }
}

export async function switchTab(tabId: string): Promise<void> {
  await invoke('switch_tab', { tabId });
  state.activeTabId = tabId;
  const tab = state.tabs.find(t => t.id === tabId);
  if (tab) state.addressValue = tab.url;
}

export async function navigate(input: string): Promise<void> {
  const url = looksLikeUrl(input)
    ? (input.startsWith('http') ? input : `https://${input}`)
    : `https://kagi.com/search?q=${encodeURIComponent(input)}`;
  await invoke('navigate', { url });
}

export async function goBack(): Promise<void> { await invoke('go_back'); }
export async function goForward(): Promise<void> { await invoke('go_forward'); }
export async function reload(): Promise<void> { await invoke('reload'); }

export async function searchHistory(query: string): Promise<void> {
  if (!query || query.length < 2) {
    state.suggestions = [];
    return;
  }
  const results = await invoke('history_search', { query });
  state.suggestions = results || [];
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
  // Tab WebView already created by Rust -- just update frontend state
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

// --- Render ---
let rendered = false;
function render(): void {
  if (rendered) return;
  rendered = true;

  const app = document.getElementById('app')!;

  // Create container elements for each section
  const sidebarEl = document.createElement('div');
  sidebarEl.className = 'sidebar-mount';
  const topbarEl = document.createElement('div');
  topbarEl.className = 'topbar-mount';

  app.appendChild(sidebarEl);
  app.appendChild(topbarEl);

  // Mount Arrow templates into their containers
  renderTabs()(sidebarEl);

  html`
    <div class="top-bar">
      <button class="nav-btn" @click="${goBack}">&#9664;</button>
      <button class="nav-btn" @click="${goForward}">&#9654;</button>
      <button class="nav-btn" @click="${reload}">&#8635;</button>
      ${renderAddressBar()}
    </div>
  `(topbarEl);
}

// --- Init ---
async function init(): Promise<void> {
  const session = await invoke('ready');
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
