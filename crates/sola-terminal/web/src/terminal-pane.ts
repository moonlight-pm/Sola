import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { invoke, on } from '@sola/ipc';

interface TerminalPaneOptions {
  tabId: string;
  tmuxSession?: string;
  initialCwd?: string;
  onExit: () => void;
  onTitleChange: (title: string) => void;
  onPtyReady: (ptyId: string) => void;
}

export class TerminalPane {
  private terminal: Terminal;
  private fitAddon: FitAddon;
  private ptyId: string | null = null;
  private container: HTMLElement;
  private options: TerminalPaneOptions;
  private unsubData: (() => void) | null = null;
  private unsubExit: (() => void) | null = null;
  private unsubScrollback: (() => void) | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private destroyed = false;

  constructor(container: HTMLElement, options: TerminalPaneOptions) {
    this.container = container;
    this.options = options;

    this.terminal = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "'IosevkaTermSlab', monospace",
      theme: {
        background: '#0a0b0d',
        foreground: '#f0f2f5',
        cursor: '#00a8ff',
        selectionBackground: 'rgba(0, 168, 255, 0.3)',
      },
    });

    this.fitAddon = new FitAddon();
    this.terminal.loadAddon(this.fitAddon);
    this.terminal.loadAddon(new WebLinksAddon((_event: MouseEvent, uri: string) => {
      invoke('open_url', { url: uri, activate: true });
    }));

    // OSC 0/2: window title
    this.terminal.onTitleChange((title: string) => {
      this.options.onTitleChange(title);
    });
  }

  async init(): Promise<void> {
    // xterm.js measures the character cell once at open(); if our
    // @font-face hasn't loaded yet it locks the cell to the fallback
    // (proportional) font and never re-measures. Wait for the actual
    // family to be available before opening.
    await document.fonts.load('14px IosevkaTermSlab');
    await this.waitForLayout();
    if (this.destroyed) return;

    this.terminal.open(this.container);
    this.fitAddon.fit();

    if (!await this.spawnPty()) return;
    if (this.destroyed) return;

    this.wireDataChannels();
    this.wireResize();
  }

  closePty(): void {
    if (this.ptyId) {
      invoke('close_pty', { pty_id: this.ptyId }).catch(() => {});
      this.ptyId = null;
    }
  }

  refit(): void {
    if (this.fitAddon && this.container.clientWidth > 0) {
      this.fitAddon.fit();
      if (this.ptyId) {
        invoke('resize_pty', {
          pty_id: this.ptyId,
          cols: this.terminal.cols,
          rows: this.terminal.rows,
        });
      }
    }
  }

  focus(): void {
    this.terminal.focus();
  }

  getSelection(): string | null {
    const sel = this.terminal.getSelection();
    return sel && sel.length > 0 ? sel : null;
  }

  paste(text: string): void {
    this.terminal.paste(text);
  }

  destroy(): void {
    this.destroyed = true;
    this.resizeObserver?.disconnect();
    this.unsubData?.();
    this.unsubExit?.();
    this.unsubScrollback?.();
    if (this.ptyId) {
      invoke('close_pty', { pty_id: this.ptyId }).catch(() => {});
      this.ptyId = null;
    }
    try {
      this.terminal.dispose();
    } catch {
      // xterm.js can throw during dispose
    }
  }

  private waitForLayout(): Promise<void> {
    if (this.container.clientWidth > 0 && this.container.clientHeight > 0) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      const ro = new ResizeObserver(() => {
        if (this.container.clientWidth > 0 && this.container.clientHeight > 0) {
          ro.disconnect();
          resolve();
        }
      });
      ro.observe(this.container);
    });
  }

  private strToBase64(str: string): string {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    let binary = '';
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }

  private wireDataChannels(): void {
    const id = this.ptyId!;

    this.unsubScrollback = on('pty:scrollback', (event: any) => {
      if (event.pty_id === id) {
        const bytes = Uint8Array.from(atob(event.data), c => c.charCodeAt(0));
        this.terminal.write(bytes);
      }
    });

    this.unsubData = on('pty:data', (event: any) => {
      if (event.pty_id === id) {
        const bytes = Uint8Array.from(atob(event.data), c => c.charCodeAt(0));
        this.terminal.write(bytes);
      }
    });

    this.unsubExit = on('pty:exit', (event: any) => {
      if (event.pty_id === id) {
        invoke('close_pty', { pty_id: id }).catch(() => {});
        this.ptyId = null;
        this.options.onExit();
      }
    });

    this.terminal.onData((data: string) => {
      if (this.ptyId) {
        invoke('write_pty', { pty_id: this.ptyId, data: this.strToBase64(data) });
      }
    });
  }

  private wireResize(): void {
    this.resizeObserver = new ResizeObserver(() => {
      if (this.container.clientWidth > 0 && this.container.clientHeight > 0) {
        this.fitAddon.fit();
        if (this.ptyId) {
          invoke('resize_pty', {
            pty_id: this.ptyId,
            cols: this.terminal.cols,
            rows: this.terminal.rows,
          });
        }
      }
    });
    this.resizeObserver.observe(this.container);
  }

  private async spawnPty(): Promise<boolean> {
    try {
      // The tab id is the canonical pty id on both sides; Rust's mirror
      // lookup decides whether this becomes a fresh spawn or attaches
      // to the tmux session of an already-persisted entry.
      const result = await invoke('spawn_pty', {
        cols: this.terminal.cols,
        rows: this.terminal.rows,
        pty_id: this.options.tabId,
        ...(this.options.tmuxSession ? { tmuxSession: this.options.tmuxSession } : {}),
        ...(this.options.initialCwd ? { cwd: this.options.initialCwd } : {}),
      }) as { pty_id: string; tmux_session: string; title?: string };
      this.ptyId = result.pty_id;
      this.options.onPtyReady(result.pty_id);
      return true;
    } catch (e) {
      this.terminal.writeln(`\x1b[31mFailed to spawn PTY: ${e}\x1b[0m`);
      return false;
    }
  }
}
