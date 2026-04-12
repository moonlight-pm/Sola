<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { connect, invoke, on } from './ws';
  import Terminal from './Terminal.svelte';
  import TerminalSidebar from './TerminalSidebar.svelte';
  import type { TerminalTab } from './TerminalSidebar.svelte';

  interface RestoredTab {
    tmuxSession: string;
    customTitle?: string;
    cwd?: string;
  }

  // --- Tab state ---
  let tabs = $state<TerminalTab[]>([]);
  let activeTabId = $state<string | null>(null);
  let nextTabNum = 1;

  // --- Sidebar state (persisted to localStorage) ---
  let sidebarCollapsed = $state(localStorage.getItem('terminal-sidebar-collapsed') === 'true');
  let sidebarWidth = $state(parseInt(localStorage.getItem('terminal-sidebar-width') || '160', 10));

  // --- Terminal refs ---
  let terminalRefs: Record<string, ReturnType<typeof Terminal>> = {};

  // --- Event listener cleanup ---
  let unsubscribers: (() => void)[] = [];

  // --- Tab management functions ---

  function createTab(tmuxSession?: string, customTitle?: string, cwd?: string): string {
    const tabId = `tab-${nextTabNum}`;
    nextTabNum++;
    tabs = [...tabs, { id: tabId, title: '', cwd: cwd || '', tmuxSession, customTitle }];
    activeTabId = tabId;
    return tabId;
  }

  function closeTab(tabId: string) {
    terminalRefs[tabId]?.closePty();
    removeTab(tabId);
  }

  function removeTab(tabId: string) {
    const idx = tabs.findIndex(t => t.id === tabId);
    if (idx === -1) return;

    delete terminalRefs[tabId];

    const newTabs = tabs.filter(t => t.id !== tabId);
    tabs = newTabs;

    if (newTabs.length === 0) {
      activeTabId = null;
      return;
    }

    if (activeTabId === tabId) {
      const newIdx = Math.min(idx, newTabs.length - 1);
      switchTab(newTabs[newIdx].id);
    }
  }

  function switchTab(tabId: string) {
    if (activeTabId === tabId) return;
    activeTabId = tabId;
    requestAnimationFrame(() => {
      terminalRefs[tabId]?.refit();
    });
  }

  function switchTabByIndex(index: number) {
    if (index >= 0 && index < tabs.length) {
      switchTab(tabs[index].id);
    }
  }

  function handleTabExit(tabId: string) {
    removeTab(tabId);
  }

  function handleTitleChange(tabId: string, title: string) {
    tabs = tabs.map(t => t.id === tabId ? { ...t, title } : t);
  }

  function handleCwdChange(tabId: string, cwd: string) {
    tabs = tabs.map(t => t.id === tabId ? { ...t, cwd } : t);
  }

  function handleReorder(fromIndex: number, toIndex: number) {
    const reordered = [...tabs];
    const [moved] = reordered.splice(fromIndex, 1);
    reordered.splice(toIndex, 0, moved);
    tabs = reordered;
    invoke('reorder_tabs', {
      pty_ids: reordered.filter(t => t.ptyId).map(t => t.ptyId),
    });
  }

  function handleRename(tabId: string, title: string) {
    const tab = tabs.find(t => t.id === tabId);
    const customTitle = title || undefined;
    tabs = tabs.map(t => t.id === tabId ? { ...t, customTitle } : t);
    if (tab?.tmuxSession) {
      invoke('rename_tab', {
        tmux_session: tab.tmuxSession,
        title,
      });
    }
  }

  // --- Sidebar callbacks ---

  function handleToggleCollapse() {
    sidebarCollapsed = !sidebarCollapsed;
    localStorage.setItem('terminal-sidebar-collapsed', String(sidebarCollapsed));
    requestAnimationFrame(() => {
      if (activeTabId) terminalRefs[activeTabId]?.refit();
    });
  }

  function handleSidebarResize(width: number) {
    sidebarWidth = width;
    localStorage.setItem('terminal-sidebar-width', String(width));
    requestAnimationFrame(() => {
      if (activeTabId) terminalRefs[activeTabId]?.refit();
    });
  }

  // --- Key handler for Super+1-9 tab switching ---
  function handleKeyDown(e: KeyboardEvent) {
    if (e.metaKey && e.key >= '1' && e.key <= '9') {
      e.preventDefault();
      switchTabByIndex(parseInt(e.key) - 1);
    }
  }

  // --- Lifecycle ---

  onMount(async () => {
    const port = (window as any).WS_PORT as number;
    const restoredTabs = ((window as any).RESTORED_TABS || []) as RestoredTab[];

    await connect(port);

    // Restore tabs from session, or create 1 new tab
    if (restoredTabs.length > 0) {
      for (const rt of restoredTabs) {
        createTab(rt.tmuxSession, rt.customTitle, rt.cwd);
      }
    } else {
      createTab();
    }

    // Listen for new_tab events from the bus (e.g. Super+T)
    unsubscribers.push(on('new_tab', () => {
      const activeCwd = tabs.find(t => t.id === activeTabId)?.cwd;
      createTab(undefined, undefined, activeCwd || undefined);
    }));
  });

  onDestroy(() => {
    for (const unsub of unsubscribers) {
      unsub();
    }
    unsubscribers = [];
  });
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="terminal-window">
  <TerminalSidebar
    {tabs}
    {activeTabId}
    collapsed={sidebarCollapsed}
    width={sidebarWidth}
    onSelect={switchTab}
    onClose={closeTab}
    onCreate={() => {
      const activeCwd = tabs.find(t => t.id === activeTabId)?.cwd;
      createTab(undefined, undefined, activeCwd || undefined);
    }}
    onToggleCollapse={handleToggleCollapse}
    onResize={handleSidebarResize}
    onReorder={handleReorder}
    onRename={handleRename}
  />
  <div class="terminal-area">
    {#each tabs as tab (tab.id)}
      <div class="terminal-pane" class:active={tab.id === activeTabId}>
        <Terminal
          bind:this={terminalRefs[tab.id]}
          tabId={tab.id}
          tmuxSession={tab.tmuxSession}
          initialCwd={tab.cwd || undefined}
          focused={tab.id === activeTabId}
          onExit={() => handleTabExit(tab.id)}
          onTitleChange={(title: string) => handleTitleChange(tab.id, title)}
          onCwdChange={(cwd: string) => handleCwdChange(tab.id, cwd)}
          onPtyReady={(ptyId: string) => {
            tabs = tabs.map(t => t.id === tab.id ? { ...t, ptyId } : t);
          }}
        />
      </div>
    {/each}
  </div>
</div>

<style>
  .terminal-window {
    display: flex;
    width: 100%;
    height: 100%;
  }

  .terminal-area {
    flex: 1;
    min-width: 0;
    position: relative;
  }

  .terminal-pane {
    position: absolute;
    inset: 0;
    display: none;
  }

  .terminal-pane.active {
    display: block;
  }
</style>
