import { reactive, html } from '@arrow-js/core';
import { connect, invoke, on } from './ws';
import { TerminalPane } from './terminal';
import { createSidebar, type TerminalTab } from './sidebar';

interface RestoredTab {
  tmuxSession: string;
  customTitle?: string;
  cwd?: string;
}

// --- Reactive state ---
const state = reactive({
  tabs: [] as TerminalTab[],
  activeTabId: null as string | null,
  sidebarCollapsed: localStorage.getItem('terminal-sidebar-collapsed') === 'true',
  sidebarWidth: parseInt(localStorage.getItem('terminal-sidebar-width') || '160', 10),
});

let nextTabNum = 1;

// --- Terminal pane tracking (imperative — xterm.js needs real DOM) ---
const panes = new Map<string, TerminalPane>();
const paneContainers = new Map<string, HTMLElement>();
let terminalArea: HTMLElement;

// --- Tab management ---

function createTab(tmuxSession?: string, customTitle?: string, cwd?: string): string {
  const tabId = `tab-${nextTabNum}`;
  nextTabNum++;

  state.tabs = [...state.tabs, { id: tabId, title: '', cwd: cwd || '', tmuxSession, customTitle }];
  state.activeTabId = tabId;

  // Create DOM container for this terminal pane
  const container = document.createElement('div');
  container.className = 'terminal-pane active';
  terminalArea.appendChild(container);
  paneContainers.set(tabId, container);
  updatePaneVisibility();

  // Create and init the terminal
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
  localStorage.setItem('terminal-sidebar-collapsed', String(state.sidebarCollapsed));
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

function handleSidebarResize(width: number) {
  state.sidebarWidth = width;
  localStorage.setItem('terminal-sidebar-width', String(width));
  requestAnimationFrame(() => {
    if (state.activeTabId) panes.get(state.activeTabId)?.refit();
  });
}

// --- App entry point ---

export async function createApp(root: HTMLElement) {
  // Terminal area (imperative — xterm panes are managed as DOM elements)
  terminalArea = document.createElement('div');
  terminalArea.className = 'terminal-area';

  // Mount the app shell with Arrow.js
  html`
    <div class="terminal-window">
      <div id="sidebar-mount"></div>
      ${terminalArea}
    </div>
  `(root);

  // Mount sidebar into its slot
  const sidebarMount = root.querySelector('#sidebar-mount')!;
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
  }, sidebarMount as HTMLElement);

  // Key handler: Super+1-9 for tab switching
  window.addEventListener('keydown', (e: KeyboardEvent) => {
    if (e.metaKey && e.key >= '1' && e.key <= '9') {
      e.preventDefault();
      const idx = parseInt(e.key) - 1;
      if (idx < state.tabs.length) switchTab(state.tabs[idx].id);
    }
  });

  // Connect WebSocket and init
  const port = (window as any).WS_PORT as number;
  const restoredTabs = ((window as any).RESTORED_TABS || []) as RestoredTab[];

  await connect(port);

  if (restoredTabs.length > 0) {
    for (const rt of restoredTabs) {
      createTab(rt.tmuxSession, rt.customTitle, rt.cwd);
    }
  } else {
    createTab();
  }

  // Listen for new_tab from bus (Super+T)
  on('new_tab', () => {
    const activeCwd = state.tabs.find(t => t.id === state.activeTabId)?.cwd;
    createTab(undefined, undefined, activeCwd || undefined);
  });
}
