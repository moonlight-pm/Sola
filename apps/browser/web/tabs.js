import { html } from './arrow.js';
import { state, createTab, closeTab, switchTab } from './app.js';

export function renderTabs() {
  return html`
    <div class="tab-sidebar">
      <div class="tab-sidebar-header">
        <span style="font-weight: 600; font-size: 12px;">Tabs</span>
        <button class="new-tab-btn" @click="${() => createTab('about:blank')}" title="New Tab">+</button>
      </div>
      <div class="tab-list">
        ${() => state.tabs.map(tab =>
          html`<div
            class="${() => `tab-item ${tab.id === state.activeTabId ? 'active' : ''}`}"
            @click="${() => switchTab(tab.id)}"
          >
            <span class="tab-item-title">${() => tab.title || tab.url || 'New Tab'}</span>
            <button class="tab-item-close" @click="${(e) => { e.stopPropagation(); closeTab(tab.id); }}" title="Close tab">&times;</button>
          </div>`
        )}
      </div>
    </div>
  `;
}
