import { html, watch } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import type { Folder, MessageSummary, MessageBody, MailRule } from './types.js';
import { smartMailboxNames } from './types.js';
import { createFolderList } from './components/folder-list.js';
import { createMessageList } from './components/message-list.js';
import { createMessageView } from './components/message-view.js';
import { createComposeView } from './components/compose-view.js';
import { createToast } from './components/toast.js';

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
    replyAll: false,
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

  // ---------------------------------------------------------------------------
  // invokeT: invoke with automatic toast on error
  // ---------------------------------------------------------------------------
  async function invokeT<T = any>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await invoke(cmd, args) as T;
    } catch (e: any) {
      state.toastError = String(e?.message ?? e);
      throw e;
    }
  }

  // ---------------------------------------------------------------------------
  // loadFolder
  // ---------------------------------------------------------------------------
  async function loadFolder(name: string): Promise<void> {
    state.searchActive = false;
    state.searchQuery = '';
    state.searchTotal = 0;
    state.selectedUid = null;
    state.folderLoading = true;
    try {
      const res = await invokeT('mail_list_messages', { folder: name, offset: 0, limit: 50 });
      state.messages = res.messages ?? [];
      state.totalMessages = res.total ?? 0;
      if (name === 'INBOX') {
        state.inboxMessages = state.messages;
      }
      lastFetch = Date.now();
    } catch {
      // invokeT already set toastError
    } finally {
      state.folderLoading = false;
    }
  }

  // ---------------------------------------------------------------------------
  // searchMessages / clearSearch
  // ---------------------------------------------------------------------------
  async function searchMessages(query: string): Promise<void> {
    if (!query.trim()) return;
    state.searchActive = true;
    state.folderLoading = true;
    try {
      const res = await invokeT('mail_search', { query });
      state.messages = res.messages ?? [];
      state.searchTotal = res.total ?? 0;
    } catch {
      // invokeT already set toastError
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

  // ---------------------------------------------------------------------------
  // loadMore
  // ---------------------------------------------------------------------------
  async function loadMore(): Promise<void> {
    if (state.isLoadingMore) return;
    if (state.searchActive) return;
    state.isLoadingMore = true;
    try {
      const offset = state.messages.length;
      const res = await invokeT('mail_list_messages', { folder: state.selectedFolder, offset, limit: 50 });
      state.messages = [...state.messages, ...(res.messages ?? [])];
      state.totalMessages = res.total ?? state.totalMessages;
    } catch {
      // invokeT already set toastError
    } finally {
      state.isLoadingMore = false;
    }
  }

  // ---------------------------------------------------------------------------
  // bulkMove / emptyFolder
  // ---------------------------------------------------------------------------
  async function bulkMove(dest: string): Promise<void> {
    if (state.bulkInProgress) return;
    state.bulkInProgress = true;
    try {
      const source = state.selectedFolder.startsWith('smart:') ? 'INBOX' : state.selectedFolder;
      const uids = state.messages.map(m => m.uid);
      for (const uid of uids) {
        await invokeT('mail_move', { uid, folder: source, dest });
      }
      await loadFolder(state.selectedFolder);
    } catch {
      // invokeT already set toastError
    } finally {
      state.bulkInProgress = false;
    }
  }

  async function emptyFolder(): Promise<void> {
    if (state.bulkInProgress) return;
    state.bulkInProgress = true;
    try {
      await invokeT('mail_empty_folder', { folder: state.selectedFolder });
      await loadFolder(state.selectedFolder);
    } catch {
      // invokeT already set toastError
    } finally {
      state.bulkInProgress = false;
    }
  }

  // ---------------------------------------------------------------------------
  // selectMessage: fetch body + mark read
  // ---------------------------------------------------------------------------
  async function selectMessage(uid: number): Promise<void> {
    state.selectedUid = uid;
    state.composing = false;
    const folder = state.selectedFolder.startsWith('smart:') ? 'INBOX' : state.selectedFolder;
    try {
      const body = await invokeT<MessageBody>('mail_fetch_body', { uid, folder });
      state.messageBody = body;

      const msg = state.messages.find(m => m.uid === uid);
      if (msg && !msg.seen) {
        invokeT('mail_mark_read', { uid, folder }).catch(() => {});
        // Mark seen locally and decrement folder unread count
        state.messages = state.messages.map(m => m.uid === uid ? { ...m, seen: true } : m);
        state.folders = state.folders.map(f =>
          f.name === folder ? { ...f, unread: Math.max(0, f.unread - 1) } : f
        );
      }
    } catch {
      // invokeT already set toastError
    }
  }

  // ---------------------------------------------------------------------------
  // moveAndAdvance
  // ---------------------------------------------------------------------------
  async function moveAndAdvance(uid: number, dest: string): Promise<void> {
    const folder = state.selectedFolder.startsWith('smart:') ? 'INBOX' : state.selectedFolder;
    const idx = state.messages.findIndex(m => m.uid === uid);
    state.lastMove = { uid, fromFolder: folder, toFolder: dest };
    try {
      await invokeT('mail_move', { uid, folder, dest });
      const updated = state.messages.filter(m => m.uid !== uid);
      state.messages = updated;
      if (updated.length === 0) {
        state.selectedUid = null;
        state.messageBody = null;
      } else {
        const nextIdx = idx > 0 ? idx - 1 : 0;
        await selectMessage(updated[nextIdx].uid);
      }
    } catch {
      // invokeT already set toastError
    }
  }

  // ---------------------------------------------------------------------------
  // undoLastMove
  // ---------------------------------------------------------------------------
  async function undoLastMove(): Promise<void> {
    const lm = state.lastMove;
    if (!lm) return;
    state.lastMove = null;
    try {
      await invokeT('mail_move', { uid: lm.uid, folder: lm.toFolder, dest: lm.fromFolder });
      await loadFolder(state.selectedFolder);
    } catch {
      // invokeT already set toastError
    }
  }

  // ---------------------------------------------------------------------------
  // selectPrev / selectNext
  // ---------------------------------------------------------------------------
  function selectPrev(): void {
    if (state.messages.length === 0 || state.selectedUid === null) return;
    const idx = state.messages.findIndex(m => m.uid === state.selectedUid);
    if (idx <= 0) return;
    selectMessage(state.messages[idx - 1].uid);
  }

  function selectNext(): void {
    if (state.messages.length === 0 || state.selectedUid === null) return;
    const idx = state.messages.findIndex(m => m.uid === state.selectedUid);
    if (idx === -1 || idx >= state.messages.length - 1) return;
    selectMessage(state.messages[idx + 1].uid);
  }

  // ---------------------------------------------------------------------------
  // refreshFolder (IDLE + focus refresh — swallows errors)
  // ---------------------------------------------------------------------------
  async function refreshFolder(): Promise<void> {
    if (state.searchActive) return;
    try {
      const foldersRes = await invoke('mail_list_folders');
      state.folders = foldersRes.folders ?? state.folders;
      state.smartCounts = foldersRes.smart_counts ?? state.smartCounts;
      await loadFolder(state.selectedFolder);
    } catch {
      // swallow — IDLE refresh failures shouldn't toast
    }
  }

  // ---------------------------------------------------------------------------
  // hasMore / isSmartMailbox helpers
  // ---------------------------------------------------------------------------
  function isSmartMailbox(): boolean {
    return state.selectedFolder.startsWith('smart:');
  }

  function hasMore(): boolean {
    if (state.searchActive) return false;
    return state.messages.length < state.totalMessages;
  }

  // ---------------------------------------------------------------------------
  // Keyboard shortcuts
  // ---------------------------------------------------------------------------
  window.addEventListener('keydown', (e: KeyboardEvent) => {
    if (state.composing) return;
    const t = e.target as HTMLElement;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return;
    if (e.ctrlKey || e.altKey || e.metaKey) return;

    const uid = state.selectedUid;
    if (uid == null) return;
    switch (e.key) {
      case 'j': e.preventDefault(); moveAndAdvance(uid, 'Junk'); break;
      case 'i': e.preventDefault(); moveAndAdvance(uid, 'INBOX'); break;
      case 'a': e.preventDefault(); moveAndAdvance(uid, 'Archive'); break;
      case 'd': e.preventDefault(); moveAndAdvance(uid, 'Trash'); break;
      case 'u': e.preventDefault(); undoLastMove(); break;
      case 'w': e.preventDefault(); selectPrev(); break;
      case 's': e.preventDefault(); selectNext(); break;
    }
  });

  // ---------------------------------------------------------------------------
  // Focus-based refresh
  // ---------------------------------------------------------------------------
  let lastFetch = Date.now();
  window.addEventListener('focus', () => {
    if (Date.now() - lastFetch > 60_000) {
      lastFetch = Date.now();
      refreshFolder();
    }
  });

  // ---------------------------------------------------------------------------
  // Mount flags
  // ---------------------------------------------------------------------------
  let folderListMounted = false;
  let messageListMounted = false;
  let messageViewMounted = false;
  let toastMounted = false;
  let composeMounted = false;

  // ---------------------------------------------------------------------------
  // Main template
  // ---------------------------------------------------------------------------
  html`
    <div class="mail-app">
      ${() => state.fatalError
        ? html`<div class="fatal">${() => state.fatalError}</div>`
        : state.loading
          ? html`<div class="loading">Connecting\u2026</div>`
          : html`
              <div class="main">
                <div id="toast-target"></div>
                <div id="folder-list-target"></div>
                <div id="message-list-target"></div>
                ${() => state.composing
                  ? html`<div id="compose-view-target"></div>`
                  : html`<div id="message-view-target"></div>`}
              </div>
            `}
    </div>
  `(root);

  // ---------------------------------------------------------------------------
  // Mount helpers
  // ---------------------------------------------------------------------------
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
          state.messageBody = null;
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
        onSelect: (uid: number) => { selectMessage(uid); },
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

  function mountMessageView(): void {
    if (messageViewMounted) return;
    const target = root.querySelector<HTMLElement>('#message-view-target');
    if (!target) return;
    messageViewMounted = true;
    createMessageView(
      {
        body: () => state.messageBody,
        onNew: () => {
          state.replyTo = null;
          state.replyAll = false;
          state.composing = true;
        },
        onReply: (all: boolean) => {
          state.replyTo = state.messageBody;
          state.replyAll = all;
          state.composing = true;
        },
        onDelete: () => {
          if (state.selectedUid != null) {
            moveAndAdvance(state.selectedUid, 'Trash');
          }
        },
      },
      target,
    );
  }

  function mountToast(): void {
    if (toastMounted) return;
    const target = root.querySelector<HTMLElement>('#toast-target');
    if (!target) return;
    toastMounted = true;
    createToast(
      {
        message: () => state.toastError,
        onDismiss: () => { state.toastError = null; },
      },
      target,
    );
  }

  // Compose toggles the target div between #message-view-target and
  // #compose-view-target, so the target needs (re)mounting on every
  // transition. Arrow's reactive setter emits on every write (no equality
  // check), so we guard against no-op transitions to avoid appending a
  // duplicate component into the same target.
  let prevComposing = state.composing;
  watch(() => {
    const composing = state.composing;
    if (composing === prevComposing) return;
    prevComposing = composing;
    if (composing) {
      requestAnimationFrame(() => {
        const target = root.querySelector<HTMLElement>('#compose-view-target');
        if (!target) return;
        composeMounted = true;
        createComposeView(
          {
            fromAddresses: () => state.fromAddresses,
            replyTo: () => state.replyTo,
            replyAll: () => state.replyAll,
            onSend: async (msg) => {
              await invokeT('mail_send', msg as Record<string, unknown>);
              state.composing = false;
              state.replyTo = null;
            },
            onClose: () => {
              state.composing = false;
              state.replyTo = null;
            },
          },
          target,
        );
      });
    } else {
      composeMounted = false;
      messageViewMounted = false;
      requestAnimationFrame(() => mountMessageView());
    }
  });

  // ---------------------------------------------------------------------------
  // Connect + initial mount
  // ---------------------------------------------------------------------------
  try {
    const res = await invoke('mail_connect');
    state.folders = res.folders ?? [];
    state.smartCounts = res.smart_counts ?? [];
    state.fromAddresses = res.from_addresses ?? [];
    state.rules = res.rules ?? [];
    state.loading = false;
    lastFetch = Date.now();
    requestAnimationFrame(() => {
      mountFolderList();
      mountMessageList();
      mountMessageView();
      mountToast();
      loadFolder('INBOX');
    });
  } catch (e: any) {
    state.fatalError = String(e?.message ?? e);
    state.loading = false;
  }

  on('mail:new', () => { refreshFolder(); });
}
