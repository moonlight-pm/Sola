import { html } from '@arrow-js/core';
import type { MessageBody } from '../types.js';
import { invoke } from '@sola/ipc';

export interface MessageViewConfig {
  body: () => MessageBody | null;
  onNew: () => void;
  onReply: (all: boolean) => void;
  onDelete: () => void;
}

function sanitizeHtml(html: string): string {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<form[\s\S]*?<\/form>/gi, '')
    .replace(/\bon\w+\s*=\s*"[^"]*"/gi, '')
    .replace(/\bon\w+\s*=\s*'[^']*'/gi, '');
}

function buildSrcdoc(body: MessageBody): string | null {
  if (!body.html) return null;
  const safe = sanitizeHtml(body.html);
  // The closing </script> tag is split to avoid ending the template literal early
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><style>
    body { font-family: 'Instrument Sans', -apple-system, BlinkMacSystemFont, sans-serif; font-size: 14px;
           color: #1a1a1a; background: #ffffff; margin: 12px; line-height: 1.5;
           -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
    a { color: #0066cc; cursor: pointer; }
    img { max-width: 100%; height: auto; }
    blockquote { border-left: 2px solid #ccc; margin: 8px 0; padding-left: 12px; color: #555; }
    pre, code { background: #f5f5f5; padding: 2px 4px; border-radius: 3px; font-size: 13px; }
  </style></head><body>${safe}
  <` + `script>
    document.addEventListener('click', function(e) {
      var a = e.target.closest('a[href]');
      if (!a) return;
      var href = a.getAttribute('href');
      if (!href || href.charAt(0) === '#') return;
      e.preventDefault();
      parent.postMessage({type: 'open-url', url: href}, '*');
      if (document.activeElement) document.activeElement.blur();
    });
    document.addEventListener('keydown', function(e) {
      try {
        parent.window.dispatchEvent(new KeyboardEvent('keydown', {
          key: e.key, code: e.code,
          ctrlKey: e.ctrlKey, altKey: e.altKey,
          metaKey: e.metaKey, shiftKey: e.shiftKey,
          bubbles: true, cancelable: true
        }));
      } catch(ex) {}
    });
  <` + `/script></body></html>`;
}

export function createMessageView(cfg: MessageViewConfig, target: HTMLElement): void {
  // Attach message listener once, globally for this component instance.
  window.addEventListener('message', (e: MessageEvent) => {
    if (e.data?.type === 'open-url' && e.data.url) {
      invoke('open_url', { url: e.data.url });
    } else if (e.data?.type === 'keydown' && e.data.key) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: e.data.key, bubbles: true, cancelable: true }));
    }
  });

  html`
    <div class="message-view">
      <div class="toolbar">
        <button class="tool-btn" @click="${() => cfg.onNew()}" title="New message">
          <span class="tool-icon">+</span>
          <span class="tool-label">New</span>
        </button>
        <div class="tool-sep"></div>
        <button
          class="tool-btn"
          disabled="${() => !cfg.body() ? 'disabled' : false}"
          @click="${() => cfg.onReply(false)}"
          title="Reply"
        >
          <span class="tool-icon">&larr;</span>
          <span class="tool-label">Reply</span>
        </button>
        <button
          class="tool-btn"
          disabled="${() => !cfg.body() ? 'disabled' : false}"
          @click="${() => cfg.onReply(true)}"
          title="Reply all"
        >
          <span class="tool-icon">&laquo;</span>
          <span class="tool-label">All</span>
        </button>
        <div class="tool-sep"></div>
        <button
          class="tool-btn danger"
          disabled="${() => !cfg.body() ? 'disabled' : false}"
          @click="${() => cfg.onDelete()}"
          title="Delete"
        >
          <span class="tool-icon">&times;</span>
          <span class="tool-label">Delete</span>
        </button>
      </div>

      ${() => {
        const msg = cfg.body();
        if (!msg) {
          return html`
            <div class="no-message">
              <div class="no-msg-icon">&#9993;</div>
              <div class="no-msg-text">Select a message to read</div>
            </div>
          `.key('no-message');
        }
        const srcdoc = buildSrcdoc(msg);
        return html`
          <div class="msg-header">
            <div class="header-row">
              <span class="header-label">From</span>
              <span class="header-value">${msg.from}</span>
            </div>
            <div class="header-row">
              <span class="header-label">To</span>
              <span class="header-value">${msg.to}</span>
            </div>
            ${msg.cc
              ? html`
                  <div class="header-row">
                    <span class="header-label">CC</span>
                    <span class="header-value">${msg.cc}</span>
                  </div>
                `
              : html``}
            <div class="header-row subject-row">
              <span class="header-label">Subj</span>
              <span class="header-value subject">${msg.subject}</span>
            </div>
          </div>
          <div class="msg-body">
            ${srcdoc
              ? html`<iframe
                  srcdoc="${srcdoc}"
                  title="email-body"
                  sandbox="allow-same-origin allow-scripts"
                ></iframe>`
              : html`<pre class="text-body">${msg.text}</pre>`}
          </div>
        `.key(`msg-${msg.uid}`);
      }}
    </div>
  `(target);
}
