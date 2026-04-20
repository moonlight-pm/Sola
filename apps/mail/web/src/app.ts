import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import type { Folder, MessageSummary, MessageBody, MailRule } from './types.js';
import { smartMailboxNames } from './types.js';
import { createFolderList } from './components/folder-list.js';

export async function createApp(root: HTMLElement): Promise<void> {
  const state = createStore({
    folders: [] as Folder[],
    smartCounts: [] as Folder[],
    selectedFolder: 'INBOX',
    messages: [] as MessageSummary[],
    inboxMessages: [] as MessageSummary[],
    totalMessages: 0,
    selectedUid: null as number | null,
    messageBody: null as MessageBody | null,
    composing: false,
    replyTo: null as MessageBody | null,
    fromAddresses: [] as string[],
    rules: [] as MailRule[],
    loading: true,
    fatalError: null as string | null,
    toastError: null as string | null,
    searchQuery: '',
    searchActive: false,
    searchTotal: 0,
    isLoadingMore: false,
    bulkInProgress: false,
    folderLoading: false,
    lastMove: null as { uid: number; fromFolder: string; toFolder: string } | null,
  });

  async function loadFolder(name: string): Promise<void> {
    if (name.startsWith('smart:')) return;
    state.folderLoading = true;
    try {
      const res = await invoke('mail_list_messages', { folder: name, offset: 0, limit: 50 });
      state.messages = res.messages ?? [];
      state.totalMessages = res.total ?? 0;
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.folderLoading = false;
    }
  }

  let folderListMounted = false;

  html`
    <div class="mail-app">
      ${() => state.fatalError
        ? html`<div class="fatal">${() => state.fatalError}</div>`
        : state.loading
          ? html`<div class="loading">Connecting\u2026</div>`
          : html`
              <div class="main">
                <div id="folder-list-target"></div>
              </div>
            `}
    </div>
  `(root);

  // Mount folder-list once the target is in the DOM (after connect resolves).
  function mountFolderList(): void {
    if (folderListMounted) return;
    const target = root.querySelector<HTMLElement>('#folder-list-target');
    if (!target) return;
    folderListMounted = true;
    createFolderList(
      {
        folders: () => state.folders,
        smartCounts: () => state.smartCounts,
        smartMailboxNames: () => smartMailboxNames(state.rules),
        selected: () => state.selectedFolder,
        onSelect: (name: string) => {
          state.selectedFolder = name;
          loadFolder(name);
        },
      },
      target,
    );
  }

  try {
    const res = await invoke('mail_connect');
    state.folders = res.folders ?? [];
    state.smartCounts = res.smart_counts ?? [];
    state.fromAddresses = res.from_addresses ?? [];
    state.rules = res.rules ?? [];
    state.loading = false;
    requestAnimationFrame(mountFolderList);
  } catch (e: any) {
    state.fatalError = String(e?.message ?? e);
    state.loading = false;
  }

  on('mail:new', () => { /* Task 13 */ });
}
