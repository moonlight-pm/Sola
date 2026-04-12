<script lang="ts">
  export interface TerminalTab {
    id: string;
    title: string;
    cwd: string;
    tmuxSession?: string;
    customTitle?: string;
    ptyId?: string;
  }

  interface Props {
    tabs: TerminalTab[];
    activeTabId: string | null;
    collapsed: boolean;
    width: number;
    onSelect: (id: string) => void;
    onClose: (id: string) => void;
    onCreate: () => void;
    onToggleCollapse: () => void;
    onResize: (width: number) => void;
    onReorder: (fromIndex: number, toIndex: number) => void;
    onRename: (id: string, title: string) => void;
  }

  let { tabs, activeTabId, collapsed, width, onSelect, onClose, onCreate, onToggleCollapse, onResize, onReorder, onRename }: Props = $props();

  const COLLAPSED_WIDTH = 36;
  const MIN_WIDTH = 80;
  const MAX_WIDTH = 250;

  // --- Sidebar resize ---
  let resizing = $state(false);
  let resizeStartX = $state(0);
  let resizeStartWidth = $state(0);

  function handleResizeStart(e: MouseEvent) {
    if (collapsed) return;
    e.preventDefault();
    resizing = true;
    resizeStartX = e.clientX;
    resizeStartWidth = width;
  }

  function handleResizeMove(e: MouseEvent) {
    if (!resizing) return;
    const delta = e.clientX - resizeStartX;
    const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, resizeStartWidth + delta));
    onResize(newWidth);
  }

  function handleResizeEnd() {
    resizing = false;
  }

  // --- Tab drag reorder (pointer-based, HTML5 DnD unreliable in WebKitGTK) ---
  let dragTabIndex = $state<number | null>(null);
  let dropTargetIndex = $state<number | null>(null);
  let dragStartY = 0;
  let isDragging = $state(false);
  let tabElements: HTMLElement[] = [];

  function handleDragMouseDown(e: MouseEvent, index: number) {
    if (collapsed || e.button !== 0) return;
    dragTabIndex = index;
    dragStartY = e.clientY;
    isDragging = false;
  }

  function handleDragMouseMove(e: MouseEvent) {
    if (dragTabIndex === null) return;
    if (!isDragging && Math.abs(e.clientY - dragStartY) > 5) {
      isDragging = true;
    }
    if (!isDragging) return;
    for (let i = 0; i < tabElements.length; i++) {
      const el = tabElements[i];
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (e.clientY >= rect.top && e.clientY < rect.bottom) {
        dropTargetIndex = i !== dragTabIndex ? i : null;
        return;
      }
    }
    dropTargetIndex = null;
  }

  function handleDragMouseUp() {
    if (isDragging && dragTabIndex !== null && dropTargetIndex !== null) {
      onReorder(dragTabIndex, dropTargetIndex);
    }
    dragTabIndex = null;
    dropTargetIndex = null;
    isDragging = false;
  }

  // --- Tab rename ---
  let renamingTabId = $state<string | null>(null);
  let renameValue = $state('');

  function startRename(tab: TerminalTab) {
    renamingTabId = tab.id;
    renameValue = tab.customTitle || cwdBasename(tab.cwd) || 'shell';
  }

  function commitRename() {
    if (renamingTabId) {
      onRename(renamingTabId, renameValue.trim());
      renamingTabId = null;
    }
  }

  function cancelRename() {
    renamingTabId = null;
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  // --- Tab click ---
  function handleTabMouseDown(e: MouseEvent, id: string) {
    if (e.button === 0) {
      e.preventDefault();
      onSelect(id);
    } else if (e.button === 1) {
      e.preventDefault();
      onClose(id);
    }
  }

  function cwdBasename(cwd: string): string {
    if (!cwd) return '';
    if (cwd === '/') return '/';
    const parts = cwd.replace(/\/$/, '').split('/');
    return parts[parts.length - 1] || '';
  }

  let sidebarWidth = $derived(collapsed ? COLLAPSED_WIDTH : width);
</script>

<svelte:window onmousemove={(e) => { handleResizeMove(e); handleDragMouseMove(e); }} onmouseup={() => { handleResizeEnd(); handleDragMouseUp(); }} />

<div
  class="sidebar"
  class:collapsed
  class:resizing
  style:width="{sidebarWidth}px"
>
  <button class="toggle-btn" onmousedown={(e) => e.preventDefault()} onclick={onToggleCollapse} aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
    {#if collapsed}
      <span class="toggle-arrow">&#9654;</span>
    {:else}
      <span class="toggle-arrow">&#9664;</span>
    {/if}
  </button>

  <div class="tab-list">
    {#each tabs as tab, i (tab.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="tab"
        class:active={tab.id === activeTabId}
        class:drag-over={dropTargetIndex === i && dragTabIndex !== null && dragTabIndex !== i}
        class:dragging-tab={isDragging && dragTabIndex === i}
        bind:this={tabElements[i]}
        onmousedown={(e) => { handleTabMouseDown(e, tab.id); handleDragMouseDown(e, i); }}
        ondblclick={() => { if (!collapsed) startRename(tab); }}
        role="tab"
        tabindex="0"
      >
        <span class="tab-number">{i + 1}</span>
        {#if !collapsed}
          {#if renamingTabId === tab.id}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="tab-rename-input"
              type="text"
              bind:value={renameValue}
              onblur={commitRename}
              onkeydown={handleRenameKeydown}
              onmousedown={(e) => e.stopPropagation()}
              autofocus
            />
          {:else}
            <div class="tab-info">
              <span class="tab-title">{tab.customTitle || cwdBasename(tab.cwd) || 'shell'}</span>
            </div>
            <button
              class="tab-close"
              onclick={(e) => { e.stopPropagation(); onClose(tab.id); }}
              onmousedown={(e) => { e.preventDefault(); e.stopPropagation(); }}
              aria-label="Close tab"
            >x</button>
          {/if}
        {/if}
      </div>
    {/each}
  </div>

  <button class="new-tab-btn" onmousedown={(e) => e.preventDefault()} onclick={onCreate} aria-label="New terminal tab">
    <span class="plus-icon">+</span>
    {#if !collapsed}
      <span class="new-tab-label">New Tab</span>
    {/if}
  </button>

  {#if !collapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="drag-handle" onmousedown={handleResizeStart}></div>
  {/if}
</div>

<style>
  .sidebar {
    position: relative;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border-subtle);
    flex-shrink: 0;
    overflow: hidden;
    user-select: none;
  }

  .sidebar.resizing {
    cursor: col-resize;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 32px;
    padding: 0;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .toggle-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .toggle-arrow {
    font-size: 12px;
    line-height: 1;
  }

  .tab-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    cursor: pointer;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
    min-height: 32px;
  }

  .tab:hover {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .tab.active {
    background: var(--bg-primary);
    color: var(--text-primary);
    border-left: 2px solid var(--cyan);
    padding-left: 6px;
  }

  .collapsed .tab {
    justify-content: center;
    padding: 6px 0;
  }

  .collapsed .tab.active {
    padding-left: 0;
    border-left: 2px solid var(--cyan);
  }

  .tab-number {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    font-size: 11px;
    font-weight: 600;
    flex-shrink: 0;
    border-radius: 3px;
    background: var(--border-subtle);
    color: var(--text-muted);
  }

  .tab.active .tab-number {
    background: var(--cyan-dim);
    color: var(--cyan);
  }

  .tab-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .tab-title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    line-height: 1.3;
  }

  .tab-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    opacity: 0;
    flex-shrink: 0;
  }

  .tab:hover .tab-close {
    opacity: 1;
  }

  .tab-close:hover {
    background: var(--red);
    color: var(--text-primary);
    opacity: 1;
  }

  .new-tab-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 32px;
    padding: 0 8px;
    background: none;
    border: none;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .new-tab-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .plus-icon {
    font-size: 16px;
    line-height: 1;
  }

  .new-tab-label {
    white-space: nowrap;
  }

  .drag-handle {
    position: absolute;
    top: 0;
    right: 0;
    width: 4px;
    height: 100%;
    cursor: col-resize;
    z-index: 10;
  }

  .drag-handle:hover {
    background: var(--cyan-dim);
  }

  .sidebar.resizing .drag-handle {
    background: var(--cyan);
  }

  .tab.dragging-tab {
    opacity: 0.4;
  }

  .tab.drag-over {
    border-top: 2px solid var(--cyan);
    padding-top: 4px;
  }

  .tab-rename-input {
    flex: 1;
    min-width: 0;
    background: var(--bg-primary);
    border: 1px solid var(--cyan);
    border-radius: 3px;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 1px 4px;
    outline: none;
  }
</style>
