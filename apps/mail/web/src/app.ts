import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import type { Folder, MessageSummary, MessageBody, MailRule } from './types.js';
import { smartMailboxNames } from './types.js';
import { createFolderList } from './components/folder-list.js';
import { createMessageList } from './components/message-list.js';

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
    state.searchActive = false;
    state.searchQuery = '';
    state.searchTotal = 0;
    state.selectedUid = null;
    state.folderLoading = true;
    try {
      const res = await invoke('mail_list_messages', { folder: name, offset: 0, limit: 50 });
      state.messages = res.messages ?? [];
      state.totalMessages = res.total ?? 0;
      if (name === 'INBOX') {
        state.inboxMessages = state.messages;
      }
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.folderLoading = false;
    }
  }

  async function searchMessages(query: string): Promise<void> {
    if (!query.trim()) return;
    state.searchActive = true;
    state.folderLoading = true;
    try {
      const res = await invoke('mail_search', { query });
      state.messages = res.messages ?? [];
      state.searchTotal = res.total ?? 0;
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.folderLoading = false;
    }
  }

  function clearSearch(): void {
    state.searchActive = false;
    state.searchQuery = '';
    state.searchTotal = 0;
    loadFolder(state.selectedFolder);
  }

  async function loadMore(): Promise<void> {
    if (state.isLoadingMore) return;
    // Search results come back all at once — no pagination on the backend
    if (state.searchActive) return;
    state.isLoadingMore = true;
    try {
      const offset = state.messages.length;
      const res = await invoke('mail_list_messages', { folder: state.selectedFolder, offset, limit: 50 });
      state.messages = [...state.messages, ...(res.messages ?? [])];
      state.totalMessages = res.total ?? state.totalMessages;
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.isLoadingMore = false;
    }
  }

  async function bulkMove(dest: string): Promise<void> {
    if (state.bulkInProgress) return;
    state.bulkInProgress = true;
    try {
      const uids = state.messages.map(m => m.uid);
      for (const uid of uids) {
        await invoke('mail_move', { uid, folder: state.selectedFolder, dest });
      }
      await loadFolder(state.selectedFolder);
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.bulkInProgress = false;
    }
  }

  async function emptyFolder(): Promise<void> {
    if (state.bulkInProgress) return;
    state.bulkInProgress = true;
    try {
      await invoke('mail_empty_folder', { folder: state.selectedFolder });
      await loadFolder(state.selectedFolder);
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
    } finally {
      state.bulkInProgress = false;
    }
  }

  function isSmartMailbox(): boolean {
    return state.selectedFolder.startsWith('smart:');
  }

  function hasMore(): boolean {
    if (state.searchActive) return false;
    return state.messages.length < state.totalMessages;
  }

  let folderListMounted = false;
  let messageListMounted = false;

  html`
    <div class="mail-app">
      ${() => state.fatalError
        ? html`<div class="fatal">${() => state.fatalError}</div>`
        : state.loading
          ? html`<div class="loading">Connecting\u2026</div>`
          : html`
              <div class="main">
                <div id="folder-list-target"></div>
                <div id="message-list-target"></div>
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

  function mountMessageList(): void {
    if (messageListMounted) return;
    const target = root.querySelector<HTMLElement>('#message-list-target');
    if (!target) return;
    messageListMounted = true;
    createMessageList(
      {
        messages: () => state.messages,
        selectedUid: () => state.selectedUid,
        hasMore,
        isLoadingMore: () => state.isLoadingMore,
        folderLoading: () => state.folderLoading,
        searchActive: () => state.searchActive,
        searchTotal: () => state.searchTotal,
        folderName: () => state.selectedFolder,
        isSmartMailbox,
        isBulkOperating: () => state.bulkInProgress,
        onSelect: (uid: number) => { state.selectedUid = uid; /* message-view comes in Task 13 */ },
        onSearch: (q: string) => { state.searchQuery = q; searchMessages(q); },
        onClearSearch: clearSearch,
        onLoadMore: loadMore,
        onArchiveAll: () => bulkMove('Archive'),
        onTrashAll: () => bulkMove('Trash'),
        onEmptyFolder: emptyFolder,
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
    requestAnimationFrame(() => {
      mountFolderList();
      mountMessageList();
      loadFolder('INBOX');
    });
  } catch (e: any) {
    state.fatalError = String(e?.message ?? e);
    state.loading = false;
  }

  on('mail:new', () => { /* Task 13 */ });
}
