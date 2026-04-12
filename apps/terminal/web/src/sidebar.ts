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

export function createSidebar(props: SidebarProps, target: HTMLElement): () => void {
  let resizing = false;
  let resizeStartX = 0;
  let resizeStartWidth = 0;
  let dragTabIndex: number | null = null;
  let dropTargetIndex: number | null = null;
  let dragStartY = 0;
  let isDragging = false;
  let renamingTabId: string | null = null;
  let renameValue = '';

  const tabElements: HTMLElement[] = [];

  // --- Sidebar resize ---

  function handleResizeStart(e: MouseEvent) {
    if (props.collapsed()) return;
    e.preventDefault();
    resizing = true;
    resizeStartX = e.clientX;
    resizeStartWidth = props.width();
  }

  function handleResizeMove(e: MouseEvent) {
    if (!resizing) return;
    const delta = e.clientX - resizeStartX;
    const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, resizeStartWidth + delta));
    props.onResize(newWidth);
  }

  function handleResizeEnd() {
    resizing = false;
  }

  // --- Tab drag reorder ---

  function handleDragMouseDown(e: MouseEvent, index: number) {
    if (props.collapsed() || e.button !== 0) return;
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
        render();
        return;
      }
    }
    dropTargetIndex = null;
    render();
  }

  function handleDragMouseUp() {
    if (isDragging && dragTabIndex !== null && dropTargetIndex !== null) {
      props.onReorder(dragTabIndex, dropTargetIndex);
    }
    dragTabIndex = null;
    dropTargetIndex = null;
    isDragging = false;
  }

  // --- Tab rename ---

  function startRename(tab: TerminalTab) {
    renamingTabId = tab.id;
    renameValue = tab.customTitle || cwdBasename(tab.cwd) || 'shell';
    render();
  }

  function commitRename() {
    if (renamingTabId) {
      props.onRename(renamingTabId, renameValue.trim());
      renamingTabId = null;
      render();
    }
  }

  function cancelRename() {
    renamingTabId = null;
    render();
  }

  // --- Window-level mouse handlers ---

  function onWindowMouseMove(e: MouseEvent) {
    handleResizeMove(e);
    handleDragMouseMove(e);
  }

  function onWindowMouseUp() {
    handleResizeEnd();
    handleDragMouseUp();
  }

  window.addEventListener('mousemove', onWindowMouseMove);
  window.addEventListener('mouseup', onWindowMouseUp);

  // --- Render ---

  const sidebarEl = document.createElement('div');
  sidebarEl.className = 'sidebar';
  target.appendChild(sidebarEl);

  function render() {
    const collapsed = props.collapsed();
    const width = collapsed ? COLLAPSED_WIDTH : props.width();
    const tabs = props.tabs();
    const activeTabId = props.activeTabId();

    sidebarEl.className = 'sidebar' + (collapsed ? ' collapsed' : '') + (resizing ? ' resizing' : '');
    sidebarEl.style.width = `${width}px`;

    // Clear and rebuild
    while (sidebarEl.firstChild) sidebarEl.removeChild(sidebarEl.firstChild);

    // Toggle button
    const toggleBtn = document.createElement('button');
    toggleBtn.className = 'toggle-btn';
    toggleBtn.setAttribute('aria-label', collapsed ? 'Expand sidebar' : 'Collapse sidebar');
    const toggleArrow = document.createElement('span');
    toggleArrow.className = 'toggle-arrow';
    toggleArrow.textContent = collapsed ? '\u25B6' : '\u25C0';
    toggleBtn.appendChild(toggleArrow);
    toggleBtn.addEventListener('mousedown', (e) => e.preventDefault());
    toggleBtn.addEventListener('click', props.onToggleCollapse);
    sidebarEl.appendChild(toggleBtn);

    // Tab list
    const tabList = document.createElement('div');
    tabList.className = 'tab-list';
    tabElements.length = 0;

    tabs.forEach((tab, i) => {
      const tabEl = document.createElement('div');
      tabEl.className = 'tab'
        + (tab.id === activeTabId ? ' active' : '')
        + (dropTargetIndex === i && dragTabIndex !== null && dragTabIndex !== i ? ' drag-over' : '')
        + (isDragging && dragTabIndex === i ? ' dragging-tab' : '');
      tabEl.setAttribute('role', 'tab');
      tabEl.setAttribute('tabindex', '0');

      tabEl.addEventListener('mousedown', (e) => {
        if (e.button === 0) {
          e.preventDefault();
          props.onSelect(tab.id);
        } else if (e.button === 1) {
          e.preventDefault();
          props.onClose(tab.id);
        }
        handleDragMouseDown(e, i);
      });

      tabEl.addEventListener('dblclick', () => {
        if (!collapsed) startRename(tab);
      });

      // Tab number
      const numSpan = document.createElement('span');
      numSpan.className = 'tab-number';
      numSpan.textContent = String(i + 1);
      tabEl.appendChild(numSpan);

      if (!collapsed) {
        if (renamingTabId === tab.id) {
          const input = document.createElement('input');
          input.className = 'tab-rename-input';
          input.type = 'text';
          input.value = renameValue;
          input.addEventListener('input', (e) => {
            renameValue = (e.target as HTMLInputElement).value;
          });
          input.addEventListener('blur', commitRename);
          input.addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              commitRename();
            } else if (e.key === 'Escape') {
              e.preventDefault();
              cancelRename();
            }
          });
          input.addEventListener('mousedown', (e) => e.stopPropagation());
          tabEl.appendChild(input);
          requestAnimationFrame(() => input.focus());
        } else {
          const info = document.createElement('div');
          info.className = 'tab-info';
          const title = document.createElement('span');
          title.className = 'tab-title';
          title.textContent = tab.customTitle || cwdBasename(tab.cwd) || 'shell';
          info.appendChild(title);
          tabEl.appendChild(info);

          const closeBtn = document.createElement('button');
          closeBtn.className = 'tab-close';
          closeBtn.setAttribute('aria-label', 'Close tab');
          closeBtn.textContent = 'x';
          closeBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            props.onClose(tab.id);
          });
          closeBtn.addEventListener('mousedown', (e) => {
            e.preventDefault();
            e.stopPropagation();
          });
          tabEl.appendChild(closeBtn);
        }
      }

      tabElements.push(tabEl);
      tabList.appendChild(tabEl);
    });

    sidebarEl.appendChild(tabList);

    // New tab button
    const newBtn = document.createElement('button');
    newBtn.className = 'new-tab-btn';
    newBtn.setAttribute('aria-label', 'New terminal tab');
    const plusIcon = document.createElement('span');
    plusIcon.className = 'plus-icon';
    plusIcon.textContent = '+';
    newBtn.appendChild(plusIcon);
    if (!collapsed) {
      const label = document.createElement('span');
      label.className = 'new-tab-label';
      label.textContent = 'New Tab';
      newBtn.appendChild(label);
    }
    newBtn.addEventListener('mousedown', (e) => e.preventDefault());
    newBtn.addEventListener('click', props.onCreate);
    sidebarEl.appendChild(newBtn);

    // Drag handle
    if (!collapsed) {
      const handle = document.createElement('div');
      handle.className = 'drag-handle';
      handle.addEventListener('mousedown', handleResizeStart);
      sidebarEl.appendChild(handle);
    }
  }

  // Initial render
  render();

  // Return re-render function; attach cleanup as property
  const api = () => { render(); };
  (api as any).destroy = () => {
    window.removeEventListener('mousemove', onWindowMouseMove);
    window.removeEventListener('mouseup', onWindowMouseUp);
    sidebarEl.remove();
  };

  return api;
}
