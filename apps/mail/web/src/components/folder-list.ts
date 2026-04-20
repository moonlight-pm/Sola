import { html } from '@arrow-js/core';
import type { Folder } from '../types.js';
import './folder-list.css';

export interface FolderListConfig {
  folders: () => Folder[];
  smartCounts: () => Folder[];
  smartMailboxNames: () => string[];
  selected: () => string;
  onSelect: (folder: string) => void;
}

const FOLDER_LABELS: Record<string, string> = {
  INBOX: 'Inbox',
};

const FOLDER_ORDER: Record<string, number> = {
  INBOX: 0,
  Sent: 1,
  Drafts: 2,
  Archive: 3,
  Junk: 4,
  Trash: 5,
};

function label(name: string): string {
  return FOLDER_LABELS[name] ?? name;
}

function folderClass(name: string, selected: string): string {
  return name === selected ? 'folder-item active' : 'folder-item';
}

export function createFolderList(cfg: FolderListConfig, target: HTMLElement): void {
  html`
    <div class="folder-list">
      <div class="folder-header">FOLDERS</div>
      ${() => [...cfg.folders()]
        .sort((a, b) => {
          const oa = FOLDER_ORDER[a.name] ?? 99;
          const ob = FOLDER_ORDER[b.name] ?? 99;
          if (oa !== ob) return oa - ob;
          return a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
        })
        .map(f => html`
          <button
            class="${() => folderClass(f.name, cfg.selected())}"
            @click="${() => cfg.onSelect(f.name)}"
          >
            <span class="folder-name">${label(f.name)}</span>
            ${() => f.total > 0
              ? html`<span class="folder-count">${() => f.unread > 0 ? `${f.unread}/` : ''}${() => String(f.total)}</span>`
              : html``}
          </button>
        `)}
      ${() => cfg.smartMailboxNames().length > 0
        ? html`
            <div class="folder-header">SMART FOLDERS</div>
            ${() => cfg.smartMailboxNames().map(smName => {
              const counts = cfg.smartCounts().find(c => c.name === smName);
              return html`
                <button
                  class="${() => folderClass(`smart:${smName}`, cfg.selected())}"
                  @click="${() => cfg.onSelect(`smart:${smName}`)}"
                >
                  <span class="folder-name">${smName}</span>
                  ${() => counts && counts.total > 0
                    ? html`<span class="folder-count">${() => counts.unread > 0 ? `${counts.unread}/` : ''}${() => String(counts.total)}</span>`
                    : html``}
                </button>
              `;
            })}
          `
        : html``}
    </div>
  `(target);
}
