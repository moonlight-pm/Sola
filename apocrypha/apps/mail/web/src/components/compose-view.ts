import { html, reactive } from '@arrow-js/core';
import type { MessageBody } from '../types.js';

export interface ComposeViewConfig {
  fromAddresses: () => string[];
  replyTo: () => MessageBody | null;
  replyAll: () => boolean;
  onSend: (msg: {
    from: string;
    to: string;
    cc?: string;
    subject: string;
    body: string;
    in_reply_to?: string;
  }) => Promise<void>;
  onClose: () => void;
}

const STORAGE_KEY = 'sola:mail:from';

function extractEmail(addr: string): string {
  const m = addr.match(/<([^>]+)>/);
  return m ? m[1].trim() : addr.trim();
}

export function createComposeView(cfg: ComposeViewConfig, target: HTMLElement): void {
  const replyTo = cfg.replyTo();
  const replyAll = cfg.replyAll();
  const fromAddresses = cfg.fromAddresses();

  // Determine initial from address
  let initialFrom = '';
  if (replyTo) {
    const recipientList = [replyTo.to, replyTo.cc]
      .filter(Boolean)
      .join(',')
      .split(',')
      .map(r => extractEmail(r).toLowerCase());
    const matched = fromAddresses.find(addr => recipientList.includes(addr.toLowerCase()));
    initialFrom = matched || localStorage.getItem(STORAGE_KEY) || fromAddresses[0] || '';
  } else {
    initialFrom = localStorage.getItem(STORAGE_KEY) || fromAddresses[0] || '';
  }

  // Prefill reply fields
  let initialTo = '';
  let initialCc = '';
  let initialSubject = '';
  let initialBody = '';

  if (replyTo) {
    initialTo = extractEmail(replyTo.from);
    initialSubject = replyTo.subject.startsWith('Re:')
      ? replyTo.subject
      : `Re: ${replyTo.subject}`;
    initialBody = `\n\n--- ${replyTo.from} wrote ---\n${replyTo.text}`;

    if (replyAll && replyTo.cc) {
      const selfEmails = new Set(fromAddresses.map(a => a.toLowerCase()));
      const ccParts: string[] = [];
      const originalTo = replyTo.to.split(',').map(a => a.trim()).filter(Boolean);
      const originalCc = replyTo.cc.split(',').map(a => a.trim()).filter(Boolean);
      for (const addr of [...originalTo, ...originalCc]) {
        const email = extractEmail(addr).toLowerCase();
        if (!selfEmails.has(email)) {
          ccParts.push(email);
        }
      }
      initialCc = ccParts.join(', ');
    }
  }

  if (initialFrom) {
    localStorage.setItem(STORAGE_KEY, initialFrom);
  }

  const local = reactive({
    from: initialFrom,
    to: initialTo,
    cc: initialCc,
    subject: initialSubject,
    sending: false,
    fromOpen: false,
  });

  function handleSend(): void {
    if (!local.to.trim() || local.sending) return;
    local.sending = true;
    const bodyEl = target.querySelector<HTMLTextAreaElement>('.compose-body');
    const msg: Parameters<typeof cfg.onSend>[0] = {
      from: local.from,
      to: local.to.trim(),
      subject: local.subject.trim(),
      body: bodyEl?.value ?? '',
    };
    if (local.cc.trim()) msg.cc = local.cc.trim();
    if (replyTo?.message_id) msg.in_reply_to = replyTo.message_id;
    cfg.onSend(msg).catch(() => {
      local.sending = false;
    });
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      cfg.onClose();
    }
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      handleSend();
    }
  }

  html`
    <div class="compose-view" @keydown="${handleKeydown}">
      <div class="compose-toolbar">
        <span class="compose-title">${replyTo ? 'Reply' : 'New Message'}</span>
        <div class="compose-actions">
          <button
            class="tool-btn send"
            disabled="${() => !local.to.trim() || local.sending ? 'disabled' : false}"
            @click="${handleSend}"
          >${() => local.sending ? 'Sending...' : 'Send'}</button>
          <button class="tool-btn" @click="${() => cfg.onClose()}">Cancel</button>
        </div>
      </div>

      <div class="compose-fields">
        <div class="field-row">
          <span class="field-label">From</span>
          ${fromAddresses.length > 1
            ? html`
                <div class="from-dropdown">
                  <button
                    class="from-trigger"
                    @click="${() => { local.fromOpen = !local.fromOpen; }}"
                  >
                    <span class="from-value">${() => local.from || 'Select address...'}</span>
                    <span class="from-chevron">&#9662;</span>
                  </button>
                  ${() => local.fromOpen
                    ? html`
                        <div class="from-menu">
                          ${fromAddresses.map(addr => html`
                            <button
                              class="from-menu-item"
                              @click="${() => {
                                local.from = addr;
                                local.fromOpen = false;
                                localStorage.setItem(STORAGE_KEY, addr);
                              }}"
                            >${addr}</button>
                          `)}
                        </div>
                      `
                    : html``}
                </div>
              `
            : html`<span class="field-value">${initialFrom}</span>`}
        </div>
        <div class="field-row">
          <label class="field-label" for="compose-to">To</label>
          <input
            class="field-input"
            id="compose-to"
            type="text"
            .value="${() => local.to}"
            @input="${(e: Event) => { local.to = (e.target as HTMLInputElement).value; }}"
            placeholder="recipient@example.com"
            spellcheck="false"
          />
        </div>
        <div class="field-row">
          <label class="field-label" for="compose-cc">CC</label>
          <input
            class="field-input"
            id="compose-cc"
            type="text"
            .value="${() => local.cc}"
            @input="${(e: Event) => { local.cc = (e.target as HTMLInputElement).value; }}"
            placeholder="optional"
            spellcheck="false"
          />
        </div>
        <div class="field-row">
          <label class="field-label" for="compose-subject">Subj</label>
          <input
            class="field-input"
            id="compose-subject"
            type="text"
            .value="${() => local.subject}"
            @input="${(e: Event) => { local.subject = (e.target as HTMLInputElement).value; }}"
            placeholder="Subject"
            spellcheck="false"
          />
        </div>
      </div>

      <textarea class="compose-body" placeholder="Write your message..."></textarea>

      <div class="compose-hint">
        <kbd>Ctrl+Enter</kbd> send &nbsp;&middot;&nbsp; <kbd>Esc</kbd> cancel
      </div>
    </div>
  `(target);

  // Set textarea initial value and focus the To field after mount.
  requestAnimationFrame(() => {
    const bodyEl = target.querySelector<HTMLTextAreaElement>('.compose-body');
    if (bodyEl) bodyEl.value = initialBody;
    const toEl = target.querySelector<HTMLInputElement>('#compose-to');
    toEl?.focus();
  });
}
