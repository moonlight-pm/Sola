import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { CanvasAddon } from '@xterm/addon-canvas';
import { invoke, on } from './ws';

interface TerminalPaneOptions {
  tabId: string;
  tmuxSession?: string;
  initialCwd?: string;
  onExit: () => void;
  onTitleChange: (title: string) => void;
  onCwdChange: (cwd: string) => void;
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
      fontFamily: "'Fira Code', 'JetBrains Mono', 'SF Mono', monospace",
      theme: {
        background: '#0a0b0d',
        foreground: '#f0f2f5',
        cursor: '#00a8ff',
        selectionBackground: 'rgba(0, 168, 255, 0.3)',
      },
    });

    this.fitAddon = new FitAddon();
    this.terminal.loadAddon(this.fitAddon);
    this.terminal.loadAddon(new WebLinksAddon());

    // OSC 0/2: window title
    this.terminal.onTitleChange((title: string) => {
      this.options.onTitleChange(title);
    });

    // OSC 7: working directory
    (this.terminal as any).parser.registerOscHandler(7, (data: string) => {
      try {
        const url = new URL(data);
        const cwd = decodeURIComponent(url.pathname);
        this.options.onCwdChange(cwd);
        if (this.ptyId) {
          invoke('update_cwd', { pty_id: this.ptyId, cwd });
        }
      } catch {
        // Ignore malformed OSC 7
      }
      return true;
    });
  }

  async init(): Promise<void> {
    await this.waitForLayout();
    if (this.destroyed) return;

    this.terminal.open(this.container);
    try {
      this.terminal.loadAddon(new CanvasAddon());
    } catch (e) {
      console.warn('Canvas renderer unavailable, using DOM fallback:', e);
    }
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
      const result = await invoke('spawn_pty', {
        cols: this.terminal.cols,
        rows: this.terminal.rows,
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
