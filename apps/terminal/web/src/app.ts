import { invoke, on } from './ipc.js';
import { TerminalPane } from './terminal.js';
import { createSidebar, type TerminalTab } from './sidebar.js';

interface RestoredTab {
  tmuxSession: string;
  customTitle?: string;
  cwd?: string;
}

// --- State ---
let tabs: TerminalTab[] = [];
let activeTabId: string | null = null;
let nextTabNum = 1;
let sidebarCollapsed = localStorage.getItem('terminal-sidebar-collapsed') === 'true';
let sidebarWidth = parseInt(localStorage.getItem('terminal-sidebar-width') || '160', 10);

// --- Terminal pane tracking ---
const panes = new Map<string, TerminalPane>();
const paneContainers = new Map<string, HTMLElement>();

// References set during createApp
let terminalArea: HTMLElement;
let rerenderSidebar: () => void;
let unsubscribers: (() => void)[] = [];

// --- Tab management ---

function createTab(tmuxSession?: string, customTitle?: string, cwd?: string): string {
  const tabId = `tab-${nextTabNum}`;
  nextTabNum++;
  tabs = [...tabs, { id: tabId, title: '', cwd: cwd || '', tmuxSession, customTitle }];
  activeTabId = tabId;

  // Create DOM container for this terminal pane
  const container = document.createElement('div');
  container.className = 'terminal-pane active';
  container.dataset.tabId = tabId;
  terminalArea.appendChild(container);
  paneContainers.set(tabId, container);

  // Hide all other panes
  updatePaneVisibility();

  // Create and init the terminal pane
  const pane = new TerminalPane(container, {
    tabId,
    tmuxSession,
    initialCwd: cwd,
    onExit: () => removeTab(tabId),
    onTitleChange: (title) => {
      tabs = tabs.map(t => t.id === tabId ? { ...t, title } : t);
      rerenderSidebar();
    },
    onCwdChange: (cwd) => {
      tabs = tabs.map(t => t.id === tabId ? { ...t, cwd } : t);
      rerenderSidebar();
    },
    onPtyReady: (ptyId) => {
      tabs = tabs.map(t => t.id === tabId ? { ...t, ptyId } : t);
    },
  });
  panes.set(tabId, pane);
  pane.init().then(() => {
    if (activeTabId === tabId) pane.focus();
  });

  rerenderSidebar();
  return tabId;
}

function closeTab(tabId: string) {
  panes.get(tabId)?.closePty();
  removeTab(tabId);
}

function removeTab(tabId: string) {
  const idx = tabs.findIndex(t => t.id === tabId);
  if (idx === -1) return;

  // Destroy pane and remove container
  panes.get(tabId)?.destroy();
  panes.delete(tabId);
  paneContainers.get(tabId)?.remove();
  paneContainers.delete(tabId);

  const newTabs = tabs.filter(t => t.id !== tabId);
  tabs = newTabs;

  if (newTabs.length === 0) {
    activeTabId = null;
    rerenderSidebar();
    return;
  }

  if (activeTabId === tabId) {
    const newIdx = Math.min(idx, newTabs.length - 1);
    activeTabId = newTabs[newIdx].id;
    updatePaneVisibility();
    requestAnimationFrame(() => {
      panes.get(activeTabId!)?.refit();
      panes.get(activeTabId!)?.focus();
    });
  }

  rerenderSidebar();
}

function switchTab(tabId: string) {
  if (activeTabId === tabId) return;
  activeTabId = tabId;
  updatePaneVisibility();
  rerenderSidebar();
  requestAnimationFrame(() => {
    panes.get(tabId)?.refit();
    panes.get(tabId)?.focus();
  });
}

function switchTabByIndex(index: number) {
  if (index >= 0 && index < tabs.length) {
    switchTab(tabs[index].id);
  }
}

function updatePaneVisibility() {
  for (const [id, container] of paneContainers) {
    if (id === activeTabId) {
      container.classList.add('active');
    } else {
      container.classList.remove('active');
    }
  }
}

function handleReorder(fromIndex: number, toIndex: number) {
  const reordered = [...tabs];
  const [moved] = reordered.splice(fromIndex, 1);
  reordered.splice(toIndex, 0, moved);
  tabs = reordered;
  rerenderSidebar();
  invoke('reorder_tabs', {
    pty_ids: reordered.filter(t => t.ptyId).map(t => t.ptyId),
  });
}

function handleRename(tabId: string, title: string) {
  const tab = tabs.find(t => t.id === tabId);
  const customTitle = title || undefined;
  tabs = tabs.map(t => t.id === tabId ? { ...t, customTitle } : t);
  rerenderSidebar();
  if (tab?.tmuxSession) {
    invoke('rename_tab', {
      tmux_session: tab.tmuxSession,
      title,
    });
  }
}

function handleToggleCollapse() {
  sidebarCollapsed = !sidebarCollapsed;
  localStorage.setItem('terminal-sidebar-collapsed', String(sidebarCollapsed));
  rerenderSidebar();
  requestAnimationFrame(() => {
    if (activeTabId) panes.get(activeTabId)?.refit();
  });
}

function handleSidebarResize(width: number) {
  sidebarWidth = width;
  localStorage.setItem('terminal-sidebar-width', String(width));
  rerenderSidebar();
  requestAnimationFrame(() => {
    if (activeTabId) panes.get(activeTabId)?.refit();
  });
}

function handleKeyDown(e: KeyboardEvent) {
  if (e.metaKey && e.key >= '1' && e.key <= '9') {
    e.preventDefault();
    switchTabByIndex(parseInt(e.key) - 1);
  }
}

// --- App entry point ---

export async function createApp(root: HTMLElement) {
  // Build layout
  const windowEl = document.createElement('div');
  windowEl.className = 'terminal-window';
  root.appendChild(windowEl);

  // Sidebar target
  const sidebarTarget = document.createElement('div');
  sidebarTarget.style.display = 'contents';
  windowEl.appendChild(sidebarTarget);

  // Terminal area
  terminalArea = document.createElement('div');
  terminalArea.className = 'terminal-area';
  windowEl.appendChild(terminalArea);

  // Create sidebar
  rerenderSidebar = createSidebar({
    tabs: () => tabs,
    activeTabId: () => activeTabId,
    collapsed: () => sidebarCollapsed,
    width: () => sidebarWidth,
    onSelect: switchTab,
    onClose: closeTab,
    onCreate: () => {
      const activeCwd = tabs.find(t => t.id === activeTabId)?.cwd;
      createTab(undefined, undefined, activeCwd || undefined);
    },
    onToggleCollapse: handleToggleCollapse,
    onResize: handleSidebarResize,
    onReorder: handleReorder,
    onRename: handleRename,
  }, sidebarTarget);

  // Key handler
  window.addEventListener('keydown', handleKeyDown);

  // Init from restored state
  const restoredTabs = ((window as any).RESTORED_TABS || []) as RestoredTab[];

  if (restoredTabs.length > 0) {
    for (const rt of restoredTabs) {
      createTab(rt.tmuxSession, rt.customTitle, rt.cwd);
    }
  } else {
    createTab();
  }

  // Listen for new_tab events from the bus
  unsubscribers.push(on('new_tab', () => {
    const activeCwd = tabs.find(t => t.id === activeTabId)?.cwd;
    createTab(undefined, undefined, activeCwd || undefined);
  }));
}
