import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore, persist, save } from '@sola/store';
import { TerminalPane } from './terminal-pane.js';
import { createSidebar, type TabItem } from './components/sidebar.js';

interface RestoredTab {
  tmuxSession: string;
  customTitle?: string;
  cwd?: string;
}

interface Tab extends TabItem {
  tmuxSession?: string;
  ptyId?: string;
}

// --- Reactive state ---

const state = createStore({
  tabs: [] as Tab[],
  activeTabId: null as string | null,
  sidebarCollapsed: false,
  sidebarWidth: 160,
});

persist(state, 'terminal-sidebar', ['sidebarCollapsed', 'sidebarWidth']);

// --- Terminal pane tracking (imperative, not reactive) ---

let nextTabNum = 1;
const panes = new Map<string, TerminalPane>();
const paneContainers = new Map<string, HTMLElement>();
let terminalArea: HTMLElement;

// --- Tab management ---

function createTab(tmuxSession?: string, customTitle?: string, cwd?: string): string {
  const tabId = `tab-${nextTabNum++}`;
  const tab: Tab = { id: tabId, title: '', cwd: cwd || '', tmuxSession, customTitle };
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
    onCwdChange: (newCwd) => {
      state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, cwd: newCwd } : t);
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

function handleRename(tabId: string, title: string) {
  const tab = state.tabs.find(t => t.id === tabId);
  const customTitle = title || undefined;
  state.tabs = state.tabs.map(t => t.id === tabId ? { ...t, customTitle } : t);
  if (tab?.tmuxSession) {
    invoke('rename_tab', { tmux_session: tab.tmuxSession, title });
  }
}

function handleToggleCollapse() {
  state.sidebarCollapsed = !state.sidebarCollapsed;
  save(state, 'terminal-sidebar', ['sidebarCollapsed', 'sidebarWidth']);
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

function handleSidebarResize(width: number) {
  state.sidebarWidth = width;
  save(state, 'terminal-sidebar', ['sidebarCollapsed', 'sidebarWidth']);
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
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
      createTab(undefined, undefined, activeCwd || undefined);
    },
    onToggleCollapse: handleToggleCollapse,
    onResize: handleSidebarResize,
    onReorder: handleReorder,
    onRename: handleRename,
  }, sidebarTarget);

  // Restore tabs
  const restoredTabs = ((window as any).RESTORED_TABS || []) as RestoredTab[];

  if (restoredTabs.length > 0) {
    for (const rt of restoredTabs) {
      createTab(rt.tmuxSession, rt.customTitle, rt.cwd);
    }
  } else {
    createTab();
  }

  // Bus events
  on('new_tab', () => {
    const activeCwd = state.tabs.find(t => t.id === state.activeTabId)?.cwd;
    createTab(undefined, undefined, activeCwd || undefined);
  });

  on('select_tab', ({ index }: { index: number }) => {
    if (index >= 0 && index < state.tabs.length) {
      switchTab(state.tabs[index].id);
    }
  });
}
