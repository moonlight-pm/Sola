<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import { CanvasAddon } from '@xterm/addon-canvas';
  import '@xterm/xterm/css/xterm.css';
  import { invoke, on } from './ws';

  interface Props {
    tabId: string;
    tmuxSession?: string;
    initialCwd?: string;
    focused?: boolean;
    onExit?: () => void;
    onTitleChange?: (title: string) => void;
    onCwdChange?: (cwd: string) => void;
    onPtyReady?: (ptyId: string) => void;
  }

  let { tabId, tmuxSession, initialCwd, focused = false, onExit, onTitleChange, onCwdChange, onPtyReady }: Props = $props();

  let terminalEl: HTMLDivElement;
  let terminal: Terminal;
  let fitAddon: FitAddon;
  let ptyId: string | null = null;
  let unsubData: (() => void) | null = null;
  let unsubExit: (() => void) | null = null;
  let unsubScrollback: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let destroyed = false;

  // Focus xterm when this tab becomes the active tab
  $effect(() => {
    if (focused && terminal) terminal.focus();
  });

  /** Explicitly close the pty -- called by App on close signal. */
  export function closePty() {
    if (ptyId) {
      invoke('close_pty', { pty_id: ptyId }).catch(() => {});
      ptyId = null;
    }
  }

  /** Re-fit the terminal to its container and notify the PTY of the new size. */
  export function refit() {
    if (fitAddon && terminalEl?.clientWidth > 0) {
      fitAddon.fit();
      if (ptyId) {
        invoke('resize_pty', {
          pty_id: ptyId,
          cols: terminal.cols,
          rows: terminal.rows,
        });
      }
    }
  }

  /** Wait until the container element has non-zero dimensions. */
  function waitForLayout(): Promise<void> {
    if (terminalEl.clientWidth > 0 && terminalEl.clientHeight > 0) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      const ro = new ResizeObserver(() => {
        if (terminalEl.clientWidth > 0 && terminalEl.clientHeight > 0) {
          ro.disconnect();
          resolve();
        }
      });
      ro.observe(terminalEl);
    });
  }

  /** Encode a string to base64 (handles binary-safe encoding). */
  function strToBase64(str: string): string {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    let binary = '';
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }

  /** Create and configure the xterm.js Terminal instance. */
  function createTerminal(): Terminal {
    const term = new Terminal({
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

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());

    term.open(terminalEl);
    try {
      term.loadAddon(new CanvasAddon());
    } catch (e) {
      console.warn('Canvas renderer unavailable, using DOM fallback:', e);
    }
    fitAddon.fit();

    // OSC 0/2: window title (shows running command)
    term.onTitleChange((title: string) => {
      onTitleChange?.(title);
    });

    // OSC 7: working directory (file://host/path)
    (term as any).parser.registerOscHandler(7, (data: string) => {
      try {
        const url = new URL(data);
        onCwdChange?.(decodeURIComponent(url.pathname));
      } catch {
        // Ignore malformed OSC 7
      }
      return true;
    });

    return term;
  }

  /** Subscribe to pty output events and wire terminal input. */
  function wireDataChannels(term: Terminal, id: string) {
    // Scrollback from tmux reattach -- write before live data
    unsubScrollback = on('pty:scrollback', (event: any) => {
      if (event.pty_id === id) {
        const bytes = Uint8Array.from(atob(event.data), c => c.charCodeAt(0));
        term.write(bytes);
      }
    });

    // PTY output -> terminal (base64 -> Uint8Array so xterm handles UTF-8)
    unsubData = on('pty:data', (event: any) => {
      if (event.pty_id === id) {
        const bytes = Uint8Array.from(atob(event.data), c => c.charCodeAt(0));
        term.write(bytes);
      }
    });

    // PTY exited (shell closed) -> clean up
    unsubExit = on('pty:exit', (event: any) => {
      if (event.pty_id === id) {
        invoke('close_pty', { pty_id: id }).catch(() => {});
        ptyId = null;
        onExit?.();
      }
    });

    // Terminal input -> PTY (base64-encoded for WebSocket transport)
    term.onData((data: string) => {
      if (ptyId) {
        invoke('write_pty', { pty_id: ptyId, data: strToBase64(data) });
      }
    });
  }

  /** Set up resize observer that fits xterm and resizes the pty. */
  function wireResize(term: Terminal) {
    resizeObserver = new ResizeObserver(() => {
      if (terminalEl.clientWidth > 0 && terminalEl.clientHeight > 0) {
        fitAddon.fit();
        if (ptyId) {
          invoke('resize_pty', {
            pty_id: ptyId,
            cols: term.cols,
            rows: term.rows,
          });
        }
      }
    });
    resizeObserver.observe(terminalEl);
  }

  /** Spawn a fresh pty. Returns false on failure. */
  async function spawnPty(term: Terminal): Promise<boolean> {
    try {
      const result = await invoke('spawn_pty', {
        cols: term.cols,
        rows: term.rows,
        ...(tmuxSession ? { tmuxSession } : {}),
        ...(initialCwd ? { cwd: initialCwd } : {}),
      }) as { pty_id: string; tmux_session: string; title?: string };
      ptyId = result.pty_id;
      onPtyReady?.(result.pty_id);
      return true;
    } catch (e) {
      term.writeln(`\x1b[31mFailed to spawn PTY: ${e}\x1b[0m`);
      return false;
    }
  }

  onMount(async () => {
    await waitForLayout();
    if (destroyed) return;

    terminal = createTerminal();

    if (!await spawnPty(terminal)) return;
    if (destroyed) return;

    wireDataChannels(terminal, ptyId!);
    wireResize(terminal);

    if (focused) terminal.focus();
  });

  onDestroy(() => {
    destroyed = true;
    resizeObserver?.disconnect();
    unsubData?.();
    unsubExit?.();
    unsubScrollback?.();
    if (ptyId) {
      invoke('close_pty', { pty_id: ptyId }).catch(() => {});
      ptyId = null;
    }
    try {
      terminal?.dispose();
    } catch {
      // xterm.js can throw during dispose
    }
  });
</script>

<div class="terminal-container" bind:this={terminalEl}></div>

<style>
  .terminal-container {
    width: 100%;
    height: 100%;
    padding: 6px;
    box-sizing: border-box;
  }
</style>
