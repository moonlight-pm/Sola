import { html } from '@arrow-js/core';

export interface TabItem {
  id: string;
  url: string;
  title: string;
  loading: boolean;
}

export interface TabSidebarConfig {
  tabs: () => TabItem[];
  activeTabId: () => string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
}

function displayTitle(tab: TabItem): string {
  if (tab.title) return tab.title;
  if (tab.url && tab.url !== 'about:blank') {
    try { return new URL(tab.url).hostname; } catch { return tab.url; }
  }
  return 'New Tab';
}

export function createTabSidebar(config: TabSidebarConfig, target: HTMLElement): void {
  function tabClass(tab: TabItem): string {
    return tab.id === config.activeTabId() ? 'tab-item active' : 'tab-item';
  }

  function tabContent(tab: TabItem) {
    return html`
      <span class="tab-item-title">${() => displayTitle(tab)}</span>
      <button class="tab-item-close" title="Close tab"
        @click="${(e: MouseEvent) => { e.stopPropagation(); config.onClose(tab.id); }}"
        @mousedown="${(e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); }}"
      ><span class="icon icon-x"></span></button>
    `;
  }

  html`
    <div class="tab-sidebar">
      <div class="tab-sidebar-header"></div>
      <div class="tab-list">
        ${() => config.tabs().map(tab => html`
          <div class="${() => tabClass(tab)}"
            @click="${() => config.onSelect(tab.id)}"
          >
            ${() => tabContent(tab)}
          </div>
        `)}
      </div>
      <button class="new-tab-btn"
        @mousedown="${(e: MouseEvent) => e.preventDefault()}"
        @click="${config.onCreate}"
      >
        <span class="icon icon-plus"></span>
        <span>New Tab</span>
      </button>
    </div>
  `(target);
}
