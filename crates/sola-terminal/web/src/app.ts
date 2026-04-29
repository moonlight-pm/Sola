import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import { TerminalPane } from './terminal-pane.js';
import { createSidebar, type TabItem } from './components/sidebar.js';

interface Tab extends TabItem {
  tmuxSession?: string;
  ptyId?: string;
}

interface TabSnapshot {
  id: string;
  tmux_session: string;
  cwd?: string;
}

interface RestoredState {
  tabs: TabSnapshot[];
  config: {
    sidebar_width: number;
    sidebar_collapsed: boolean;
  };
}

const initial: RestoredState = (window as any).RESTORED_STATE ?? {
  tabs: [],
  config: { sidebar_width: 220, sidebar_collapsed: false },
};

// --- Reactive state ---

const state = createStore({
  tabs: [] as Tab[],
  activeTabId: null as string | null,
  sidebarCollapsed: initial.config.sidebar_collapsed,
  sidebarWidth: initial.config.sidebar_width,
});

// --- Terminal pane tracking (imperative, not reactive) ---

const panes = new Map<string, TerminalPane>();
const paneContainers = new Map<string, HTMLElement>();
let terminalArea: HTMLElement;

// --- Tab management ---

function createTab(tmuxSession?: string, cwd?: string, id?: string): string {
  // The same id is used JS-side, Rust-side, and on the bus. For fresh
  // tabs we mint a uuid client-side; for restores the caller passes
  // the persisted id. Rust treats `pty_id` as a hint: if its mirror
  // already has the id (replay path) it attaches to the existing
  // tmux session and skips the topic emit; otherwise it creates fresh.
  const tabId = id ?? crypto.randomUUID();
  const tab: Tab = { id: tabId, title: '', cwd: cwd || '', tmuxSession };
  state.tabs = [...state.tabs, tab];
  state.activeTabId = tabId;

  const container = document.createElement('div');
  container.className = 'terminal-pane active';
  container.dataset.tabId = tabId;
  terminalArea.appendChild(container);
  paneContainers.set(tabId, container);

  updatePaneVisibility();

  const pane = new TerminalPane(container, {
    tabId,
    tmuxSession,
    initialCwd: cwd,
    onExit: () => removeTab(tabId),
    onTitleChange: (title) => {
      state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, title } : t);
    },
    onPtyReady: (ptyId) => {
      state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, ptyId } : t);
    },
  });
  panes.set(tabId, pane);
  pane.init().then(() => {
    if (state.activeTabId === tabId) pane.focus();
  });

  return tabId;
}

function closeTab(tabId: string) {
  panes.get(tabId)?.closePty();
  removeTab(tabId);
}

function removeTab(tabId: string) {
  const idx = state.tabs.findIndex(t => t.id === tabId);
  if (idx === -1) return;

  panes.get(tabId)?.destroy();
  panes.delete(tabId);
  paneContainers.get(tabId)?.remove();
  paneContainers.delete(tabId);

  const newTabs = state.tabs.filter(t => t.id !== tabId);
  state.tabs = newTabs;

  if (newTabs.length === 0) {
    state.activeTabId = null;
    return;
  }

  if (state.activeTabId === tabId) {
    const newIdx = Math.min(idx, newTabs.length - 1);
    state.activeTabId = newTabs[newIdx].id;
    updatePaneVisibility();
    requestAnimationFrame(() => {
      panes.get(state.activeTabId!)?.refit();
      panes.get(state.activeTabId!)?.focus();
    });
  }
}

function switchTab(tabId: string) {
  if (state.activeTabId === tabId) return;
  state.activeTabId = tabId;
  updatePaneVisibility();
  requestAnimationFrame(() => {
    panes.get(tabId)?.refit();
    panes.get(tabId)?.focus();
  });
}

function updatePaneVisibility() {
  for (const [id, container] of paneContainers) {
    container.classList.toggle('active', id === state.activeTabId);
  }
}

function handleReorder(fromIndex: number, toIndex: number) {
  const reordered = [...state.tabs];
  const [moved] = reordered.splice(fromIndex, 1);
  reordered.splice(toIndex, 0, moved);
  state.tabs = reordered;
  invoke('reorder_tabs', {
    pty_ids: reordered.filter(t => t.ptyId).map(t => t.ptyId),
  });
}

function handleToggleCollapse() {
  state.sidebarCollapsed = !state.sidebarCollapsed;
  invoke('set_sidebar', {
    width: state.sidebarWidth,
    collapsed: state.sidebarCollapsed,
  });
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

function handleSidebarResize(width: number) {
  state.sidebarWidth = width;
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

function handleSidebarResizeEnd() {
  invoke('set_sidebar', {
    width: state.sidebarWidth,
    collapsed: state.sidebarCollapsed,
  });
}

// --- App entry point ---

export async function createApp(root: HTMLElement) {
  // Layout via Arrow.js template
  const sidebarTarget = document.createElement('div');
  sidebarTarget.style.display = 'contents';

  terminalArea = document.createElement('div');
  terminalArea.className = 'terminal-area';

  html`<div class="terminal-window"></div>`(root);
  const windowEl = root.querySelector('.terminal-window')!;
  windowEl.appendChild(sidebarTarget);
  windowEl.appendChild(terminalArea);

  // Sidebar
  createSidebar({
    tabs: () => state.tabs,
    activeTabId: () => state.activeTabId,
    collapsed: () => state.sidebarCollapsed,
    width: () => state.sidebarWidth,
    onSelect: switchTab,
    onClose: closeTab,
    onCreate: () => {
      const activeCwd = state.tabs.find(t => t.id === state.activeTabId)?.cwd;
      createTab(undefined, activeCwd || undefined);
    },
    onToggleCollapse: handleToggleCollapse,
    onResize: handleSidebarResize,
    onResizeEnd: handleSidebarResizeEnd,
    onReorder: handleReorder,
  }, sidebarTarget);

  // Bus events
  on('new_tab', () => {
    const activeCwd = state.tabs.find(t => t.id === state.activeTabId)?.cwd;
    createTab(undefined, activeCwd || undefined);
  });

  on('select_tab', ({ index }: { index: number }) => {
    if (index >= 0 && index < state.tabs.length) {
      switchTab(state.tabs[index].id);
    }
  });

  on('close_tab', () => {
    if (state.activeTabId) closeTab(state.activeTabId);
  });

  // Server-side cwd update (tmux poll). Tabs whose shell never emits
  // OSC 7 still get a live cwd via this path. Field is `pty_id`, not
  // `id`, because the IPC recv routes any top-level `id` as an invoke
  // response.
  on('cwd_update', ({ pty_id, cwd }: { pty_id: string; cwd: string }) => {
    state.tabs = state.tabs.map(t => t.id === pty_id ? { ...t, cwd } : t);
  });

  on('copy', () => {
    if (!state.activeTabId) return;
    const text = panes.get(state.activeTabId)?.getSelection();
    if (!text) return;
    navigator.clipboard.writeText(text).catch((e) => {
      console.error('copy failed', e);
    });
  });

  on('paste', (msg: { text?: string }) => {
    if (!state.activeTabId || !msg.text) return;
    const pane = panes.get(state.activeTabId);
    if (pane) pane.paste(msg.text);
  });

  // The bus is the source of truth for the tab set: every `state`
  // event delivers the current TerminalSession map. Reconcile by id
  // — add what's in the payload but not local, remove what's local
  // but not in the payload. Idempotent, so it doesn't matter which
  // event arrives first or how many we get during sticky replay.
  on('state', (payload: { tabs: TabSnapshot[]; config: { sidebar_width: number; sidebar_collapsed: boolean } }) => {
    state.sidebarWidth = payload.config.sidebar_width;
    state.sidebarCollapsed = payload.config.sidebar_collapsed;
    requestAnimationFrame(() => {
      if (state.activeTabId) panes.get(state.activeTabId)?.refit();
    });

    const incomingIds = new Set(payload.tabs.map(t => t.id));
    const existingIds = new Set(state.tabs.map(t => t.id));

    for (const t of [...state.tabs]) {
      if (!incomingIds.has(t.id)) removeTab(t.id);
    }

    for (const t of payload.tabs) {
      if (!existingIds.has(t.id)) {
        createTab(t.tmux_session, t.cwd, t.id);
      }
    }
  });
}
