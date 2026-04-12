import { html, reactive, watch } from '@arrow-js/core';

export interface TabItem {
  id: string;
  title: string;
  cwd: string;
  customTitle?: string;
}

export interface SidebarConfig {
  tabs: () => TabItem[];
  activeTabId: () => string | null;
  collapsed: () => boolean;
  width: () => number;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
  onToggleCollapse: () => void;
  onResize: (width: number) => void;
  onReorder: (fromIndex: number, toIndex: number) => void;
  onRename: (id: string, title: string) => void;
}

const COLLAPSED_WIDTH = 36;
const MIN_WIDTH = 80;
const MAX_WIDTH = 250;

function cwdBasename(cwd: string): string {
  if (!cwd) return '';
  if (cwd === '/') return '/';
  const parts = cwd.replace(/\/$/, '').split('/');
  return parts[parts.length - 1] || '';
}

function displayTitle(tab: TabItem): string {
  return tab.customTitle || cwdBasename(tab.cwd) || 'shell';
}

export function createSidebar(config: SidebarConfig, target: HTMLElement): void {
  // Local UI state for drag and rename interactions
  const ui = reactive({
    dragTabIndex: null as number | null,
    dropTargetIndex: null as number | null,
    isDragging: false,
    renamingTabId: null as string | null,
    renameValue: '',
    resizing: false,
  });

  // Non-reactive imperative state for mouse tracking
  let resizeStartX = 0;
  let resizeStartWidth = 0;
  let dragStartY = 0;

  // --- Sidebar resize ---

  function handleResizeStart(e: MouseEvent) {
    if (config.collapsed()) return;
    e.preventDefault();
    ui.resizing = true;
    resizeStartX = e.clientX;
    resizeStartWidth = config.width();
  }

  // --- Tab drag reorder ---

  function handleDragMouseDown(e: MouseEvent, index: number) {
    if (config.collapsed() || e.button !== 0) return;
    ui.dragTabIndex = index;
    dragStartY = e.clientY;
    ui.isDragging = false;
  }

  // --- Tab rename ---

  function startRename(tab: TabItem) {
    ui.renamingTabId = tab.id;
    ui.renameValue = displayTitle(tab);
  }

  function commitRename() {
    if (ui.renamingTabId) {
      config.onRename(ui.renamingTabId, ui.renameValue.trim());
      ui.renamingTabId = null;
    }
  }

  function cancelRename() {
    ui.renamingTabId = null;
  }

  // --- Window-level mouse handlers ---

  function onMouseMove(e: MouseEvent) {
    // Resize
    if (ui.resizing) {
      const delta = e.clientX - resizeStartX;
      const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, resizeStartWidth + delta));
      config.onResize(newWidth);
    }
    // Drag
    if (ui.dragTabIndex !== null) {
      if (!ui.isDragging && Math.abs(e.clientY - dragStartY) > 5) {
        ui.isDragging = true;
      }
      if (ui.isDragging) {
        const tabEls = target.querySelectorAll('.tab');
        let found = false;
        tabEls.forEach((el, i) => {
          const rect = el.getBoundingClientRect();
          if (e.clientY >= rect.top && e.clientY < rect.bottom && i !== ui.dragTabIndex) {
            ui.dropTargetIndex = i;
            found = true;
          }
        });
        if (!found) ui.dropTargetIndex = null;
      }
    }
  }

  function onMouseUp() {
    if (ui.resizing) ui.resizing = false;
    if (ui.isDragging && ui.dragTabIndex !== null && ui.dropTargetIndex !== null) {
      config.onReorder(ui.dragTabIndex, ui.dropTargetIndex);
    }
    ui.dragTabIndex = null;
    ui.dropTargetIndex = null;
    ui.isDragging = false;
  }

  window.addEventListener('mousemove', onMouseMove);
  window.addEventListener('mouseup', onMouseUp);

  // Focus rename input after it appears
  watch(() => {
    if (ui.renamingTabId) {
      requestAnimationFrame(() => {
        target.querySelector<HTMLInputElement>('.tab-rename-input')?.focus();
      });
    }
  });

  // --- Template helpers ---

  function sidebarClass(): string {
    const parts = ['sidebar'];
    if (config.collapsed()) parts.push('collapsed');
    if (ui.resizing) parts.push('resizing');
    return parts.join(' ');
  }

  function sidebarWidth(): string {
    return `${config.collapsed() ? COLLAPSED_WIDTH : config.width()}px`;
  }

  function tabClass(tab: TabItem, index: number): string {
    const parts = ['tab'];
    if (tab.id === config.activeTabId()) parts.push('active');
    if (ui.dropTargetIndex === index && ui.dragTabIndex !== null && ui.dragTabIndex !== index) {
      parts.push('drag-over');
    }
    if (ui.isDragging && ui.dragTabIndex === index) parts.push('dragging-tab');
    return parts.join(' ');
  }

  function tabContent(tab: TabItem) {
    if (config.collapsed()) return html``;

    if (ui.renamingTabId === tab.id) {
      return html`<input
        class="tab-rename-input"
        type="text"
        value="${ui.renameValue}"
        @input="${(e: Event) => { ui.renameValue = (e.target as HTMLInputElement).value; }}"
        @blur="${commitRename}"
        @keydown="${(e: KeyboardEvent) => {
          if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
          else if (e.key === 'Escape') { e.preventDefault(); cancelRename(); }
        }}"
        @mousedown="${(e: MouseEvent) => e.stopPropagation()}"
      />`;
    }

    return html`
      <div class="tab-info">
        <span class="tab-title">${() => displayTitle(tab)}</span>
      </div>
      <button class="tab-close" aria-label="Close tab"
        @click="${(e: MouseEvent) => { e.stopPropagation(); config.onClose(tab.id); }}"
        @mousedown="${(e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); }}"
      >x</button>
    `;
  }

  // --- Mount template ---

  html`
    <div class="${() => sidebarClass()}" style="${() => `width: ${sidebarWidth()}`}">
      <button class="toggle-btn"
        aria-label="${() => config.collapsed() ? 'Expand sidebar' : 'Collapse sidebar'}"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${config.onToggleCollapse}"
      >
        <span class="toggle-arrow">${() => config.collapsed() ? '\u25B6' : '\u25C0'}</span>
      </button>

      <div class="tab-list">
        ${() => config.tabs().map((tab, i) => html`
          <div class="${() => tabClass(tab, i)}" role="tab" tabindex="0"
            @mousedown="${(e: MouseEvent) => {
              if (e.button === 0) { e.preventDefault(); config.onSelect(tab.id); }
              else if (e.button === 1) { e.preventDefault(); config.onClose(tab.id); }
              handleDragMouseDown(e, i);
            }}"
            @dblclick="${() => { if (!config.collapsed()) startRename(tab); }}"
          >
            <span class="tab-number">${() => String(i + 1)}</span>
            ${() => tabContent(tab)}
          </div>
        `)}
      </div>

      <button class="new-tab-btn" aria-label="New terminal tab"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${config.onCreate}"
      >
        <span class="plus-icon">+</span>
        ${() => config.collapsed() ? html`` : html`<span class="new-tab-label">New Tab</span>`}
      </button>

      ${() => config.collapsed() ? html`` : html`
        <div class="drag-handle" @mousedown="${handleResizeStart}"></div>
      `}
    </div>
  `(target);
}
