import { html, reactive, watch } from '@arrow-js/core';
import type { MessageSummary } from '../types.js';

export interface MessageListConfig {
  messages: () => MessageSummary[];
  selectedUid: () => number | null;
  hasMore: () => boolean;
  isLoadingMore: () => boolean;
  folderLoading: () => boolean;
  searchActive: () => boolean;
  searchTotal: () => number;
  folderName: () => string;
  isSmartMailbox: () => boolean;
  isBulkOperating: () => boolean;
  onSelect: (uid: number) => void;
  onSearch: (query: string) => void;
  onClearSearch: () => void;
  onLoadMore: () => void;
  onArchiveAll: () => void;
  onTrashAll: () => void;
  onEmptyFolder: () => void;
}

function senderName(from: string): string {
  const match = from.match(/^([^<]+)</);
  return (match ? match[1].trim() : from).slice(0, 24);
}

function formatDate(raw: string): string {
  if (!raw) return '';
  try {
    const d = new Date(raw);
    const now = new Date();
    const diff = now.getTime() - d.getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return 'now';
    if (mins < 60) return `${mins}m`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h`;
    const days = Math.floor(hours / 24);
    if (days < 7) return `${days}d`;
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  } catch {
    return raw.slice(0, 10);
  }
}

export function createMessageList(cfg: MessageListConfig, target: HTMLElement): void {
  const local = reactive({ input: '', autoLoadCount: 0 });

  // Reset autoLoadCount when message list is replaced (folder change or empty)
  watch(() => {
    if (cfg.messages().length === 0) {
      local.autoLoadCount = 0;
    }
  });

  // Auto-load more when messages don't fill the viewport (cap at 3)
  watch(() => {
    const _msgs = cfg.messages().length;
    const _more = cfg.hasMore();
    const _loading = cfg.isLoadingMore();
    if (!_more || _loading) return;
    if (local.autoLoadCount >= 3) return;
    requestAnimationFrame(() => {
      const scrollEl = target.querySelector<HTMLDivElement>('.list-scroll');
      if (scrollEl && scrollEl.scrollHeight <= scrollEl.clientHeight) {
        local.autoLoadCount++;
        cfg.onLoadMore();
      }
    });
  });

  function handleScroll(e: Event): void {
    const el = e.currentTarget as HTMLDivElement;
    if (!cfg.hasMore() || cfg.isLoadingMore()) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 100) {
      cfg.onLoadMore();
    }
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && local.input.trim()) {
      e.preventDefault();
      e.stopPropagation();
      cfg.onSearch(local.input);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      local.input = '';
      cfg.onClearSearch();
    }
  }

  function handleClear(): void {
    local.input = '';
    cfg.onClearSearch();
  }

  function headerCount(): string {
    const msgs = cfg.messages();
    const total = cfg.searchTotal();
    if (cfg.searchActive() && msgs.length < total) {
      return `${msgs.length} of ${total}`;
    }
    return String(cfg.searchActive() ? total : msgs.length);
  }

  function rowClass(uid: number): string {
    const msgs = cfg.messages();
    const msg = msgs.find(m => m.uid === uid);
    const active = cfg.selectedUid() === uid ? ' active' : '';
    const unread = msg && !msg.seen ? ' unread' : '';
    return `message-row${active}${unread}`;
  }

  html`
    <div class="message-list">
      <div class="list-header">
        <span class="header-label">${() => cfg.searchActive() ? 'SEARCH RESULTS' : 'MESSAGES'}</span>
        <span class="header-count">${headerCount}</span>
      </div>
      ${() => cfg.isSmartMailbox() && cfg.messages().length > 0
        ? html`
            <div class="action-bar">
              <button
                class="action-btn"
                disabled="${() => cfg.isBulkOperating() ? 'disabled' : false}"
                @click="${() => cfg.onArchiveAll()}"
              >Archive all</button>
              <button
                class="action-btn danger"
                disabled="${() => cfg.isBulkOperating() ? 'disabled' : false}"
                @click="${() => cfg.onTrashAll()}"
              >Trash all</button>
            </div>
          `
        : (cfg.folderName() === 'Trash' || cfg.folderName() === 'Junk') && cfg.messages().length > 0
          ? html`
              <div class="action-bar">
                <button
                  class="action-btn danger"
                  disabled="${() => cfg.isBulkOperating() ? 'disabled' : false}"
                  @click="${() => cfg.onEmptyFolder()}"
                >Permanently delete all</button>
              </div>
            `
          : html``}
      <div class="search-bar">
        <div class="search-input-wrap" @keydown="${handleKeydown}">
          <input
            type="text"
            class="search-input"
            placeholder="Search mail..."
            .value="${() => local.input}"
            @input="${(e: Event) => { local.input = (e.target as HTMLInputElement).value; }}"
          />
          ${() => local.input || cfg.searchActive()
            ? html`<button class="search-clear" @click="${handleClear}">&times;</button>`
            : html``}
        </div>
      </div>
      <div class="list-scroll" @scroll="${handleScroll}">
        ${() => cfg.messages().map(msg => html`
          <button
            class="${() => rowClass(msg.uid)}"
            @click="${() => cfg.onSelect(msg.uid)}"
          >
            <div class="msg-top">
              <span class="msg-from">${() => senderName(cfg.folderName() === 'Sent' ? msg.to : msg.from)}</span>
              ${() => msg.forwarded_for
                ? html`<span class="msg-fwd" title="${msg.forwarded_for}">\u21B3</span>`
                : html``}
              <span class="msg-date">${formatDate(msg.date)}</span>
            </div>
            <div class="msg-subject">${() => msg.subject || '(no subject)'}</div>
          </button>
        `)}
        ${() => cfg.folderLoading()
          ? html`
              <div class="folder-loading">
                <div class="spinner-dot"></div>
                <span class="loading-text">Loading...</span>
              </div>
            `
          : cfg.messages().length === 0
            ? html`<div class="empty">No messages</div>`
            : html``}
        ${() => cfg.isLoadingMore()
          ? html`
              <div class="load-more-spinner">
                <div class="spinner-dot"></div>
              </div>
            `
          : html``}
      </div>
    </div>
  `(target);
}
