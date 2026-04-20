import { html } from '@arrow-js/core';
import { invoke, on } from '@sola/ipc';
import { createStore } from '@sola/store';
import type { Folder, MessageSummary, MessageBody, MailRule } from './types.js';

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

  html`
    <div class="mail-app">
      ${() => state.fatalError
        ? html`<div class="fatal">${() => state.fatalError}</div>`
        : state.loading
          ? html`<div class="loading">Connecting\u2026</div>`
          : html`<div class="main">folders: ${() => state.folders.length}</div>`}
    </div>
  `(root);

  try {
    const res = await invoke('mail_connect');
    state.folders = res.folders ?? [];
    state.smartCounts = res.smart_counts ?? [];
    state.fromAddresses = res.from_addresses ?? [];
    state.rules = res.rules ?? [];
    state.loading = false;
  } catch (e: any) {
    state.fatalError = String(e?.message ?? e);
    state.loading = false;
  }

  on('mail:new', () => { /* Task 13 */ });
}
