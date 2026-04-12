import { reactive, html } from '@arrow-js/core';

export interface TerminalTab {
  id: string;
  title: string;
  cwd: string;
  tmuxSession?: string;
  customTitle?: string;
  ptyId?: string;
}

interface SidebarProps {
  tabs: () => TerminalTab[];
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

export function createSidebar(props: SidebarProps, target: HTMLElement) {
  const drag = reactive({
    resizing: false,
    resizeStartX: 0,
    resizeStartWidth: 0,
    tabIndex: null as number | null,
    dropIndex: null as number | null,
    startY: 0,
    active: false,
    renamingId: null as string | null,
    renameValue: '',
  });

  // Track tab elements for drag hit-testing
  let tabElements: HTMLElement[] = [];

  // --- Resize ---

  function handleResizeStart(e: MouseEvent) {
    if (props.collapsed()) return;
    e.preventDefault();
    drag.resizing = true;
    drag.resizeStartX = e.clientX;
    drag.resizeStartWidth = props.width();
  }

  // --- Drag reorder ---

  function handleDragMouseDown(e: MouseEvent, index: number) {
    if (props.collapsed() || e.button !== 0) return;
    drag.tabIndex = index;
    drag.startY = e.clientY;
    drag.active = false;
  }

  // --- Rename ---

  function startRename(tab: TerminalTab) {
    drag.renamingId = tab.id;
    drag.renameValue = tab.customTitle || cwdBasename(tab.cwd) || 'shell';
  }

  function commitRename() {
    if (drag.renamingId) {
      props.onRename(drag.renamingId, drag.renameValue.trim());
      drag.renamingId = null;
    }
  }

  function cancelRename() {
    drag.renamingId = null;
  }

  // --- Window-level mouse handlers ---

  window.addEventListener('mousemove', (e: MouseEvent) => {
    if (drag.resizing) {
      const delta = e.clientX - drag.resizeStartX;
      props.onResize(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, drag.resizeStartWidth + delta)));
    }
    if (drag.tabIndex !== null) {
      if (!drag.active && Math.abs(e.clientY - drag.startY) > 5) {
        drag.active = true;
      }
      if (drag.active) {
        let found = false;
        for (let i = 0; i < tabElements.length; i++) {
          const rect = tabElements[i]?.getBoundingClientRect();
          if (rect && e.clientY >= rect.top && e.clientY < rect.bottom) {
            drag.dropIndex = i !== drag.tabIndex ? i : null;
            found = true;
            break;
          }
        }
        if (!found) drag.dropIndex = null;
      }
    }
  });

  window.addEventListener('mouseup', () => {
    if (drag.resizing) drag.resizing = false;
    if (drag.active && drag.tabIndex !== null && drag.dropIndex !== null) {
      props.onReorder(drag.tabIndex, drag.dropIndex);
    }
    drag.tabIndex = null;
    drag.dropIndex = null;
    drag.active = false;
  });

  // --- Render ---

  function tabClass(tab: TerminalTab, i: number): string {
    const active = tab.id === props.activeTabId() ? ' active' : '';
    const dragOver = drag.dropIndex === i && drag.tabIndex !== null && drag.tabIndex !== i ? ' drag-over' : '';
    const dragging = drag.active && drag.tabIndex === i ? ' dragging-tab' : '';
    return `tab${active}${dragOver}${dragging}`;
  }

  function renderTab(tab: TerminalTab, i: number) {
    const collapsed = props.collapsed();

    if (collapsed) {
      return html`
        <div class="${() => tabClass(tab, i)}" role="tab" tabindex="0"
          @mousedown="${(e: MouseEvent) => {
            if (e.button === 0) { e.preventDefault(); props.onSelect(tab.id); }
            else if (e.button === 1) { e.preventDefault(); props.onClose(tab.id); }
            handleDragMouseDown(e, i);
          }}">
          <span class="tab-number">${i + 1}</span>
        </div>
      `.key(tab.id);
    }

    if (drag.renamingId === tab.id) {
      return html`
        <div class="${() => tabClass(tab, i)}" role="tab" tabindex="0"
          @mousedown="${(e: MouseEvent) => {
            if (e.button === 0) { e.preventDefault(); props.onSelect(tab.id); }
            handleDragMouseDown(e, i);
          }}">
          <span class="tab-number">${i + 1}</span>
          <input class="tab-rename-input" type="text" value="${drag.renameValue}"
            @input="${(e: Event) => { drag.renameValue = (e.target as HTMLInputElement).value; }}"
            @blur="${commitRename}"
            @keydown="${(e: KeyboardEvent) => {
              if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
              else if (e.key === 'Escape') { e.preventDefault(); cancelRename(); }
            }}"
            @mousedown="${(e: MouseEvent) => e.stopPropagation()}" />
        </div>
      `.key(tab.id);
    }

    return html`
      <div class="${() => tabClass(tab, i)}" role="tab" tabindex="0"
        @mousedown="${(e: MouseEvent) => {
          if (e.button === 0) { e.preventDefault(); props.onSelect(tab.id); }
          else if (e.button === 1) { e.preventDefault(); props.onClose(tab.id); }
          handleDragMouseDown(e, i);
        }}"
        @dblclick="${() => startRename(tab)}">
        <span class="tab-number">${i + 1}</span>
        <div class="tab-info">
          <span class="tab-title">${tab.customTitle || cwdBasename(tab.cwd) || 'shell'}</span>
        </div>
        <button class="tab-close" aria-label="Close tab"
          @click="${(e: MouseEvent) => { e.stopPropagation(); props.onClose(tab.id); }}"
          @mousedown="${(e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); }}">x</button>
      </div>
    `.key(tab.id);
  }

  const sidebarClass = () => {
    let cls = 'sidebar';
    if (props.collapsed()) cls += ' collapsed';
    if (drag.resizing) cls += ' resizing';
    return cls;
  };

  const sidebarWidth = () => `${props.collapsed() ? COLLAPSED_WIDTH : props.width()}px`;

  const template = html`
    <div class="${sidebarClass}" style="width: ${sidebarWidth}">
      <button class="toggle-btn"
        aria-label="${() => props.collapsed() ? 'Expand sidebar' : 'Collapse sidebar'}"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${props.onToggleCollapse}">
        <span class="toggle-arrow">${() => props.collapsed() ? '\u25B6' : '\u25C0'}</span>
      </button>

      <div class="tab-list">
        ${() => {
          const tabs = props.tabs();
          tabElements = [];
          return tabs.map((tab, i) => renderTab(tab, i));
        }}
      </div>

      <button class="new-tab-btn" aria-label="New terminal tab"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${props.onCreate}">
        <span class="plus-icon">+</span>
        ${() => props.collapsed() ? '' : html`<span class="new-tab-label">New Tab</span>`}
      </button>

      ${() => props.collapsed() ? '' : html`
        <div class="drag-handle" @mousedown="${handleResizeStart}"></div>
      `}
    </div>
  `;

  template(target);
}
