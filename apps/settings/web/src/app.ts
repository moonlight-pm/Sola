import { html, reactive } from '@arrow-js/core';

export async function createApp(root: HTMLElement): Promise<void> {
  const state = reactive({ ready: true });
  html`<div style="padding: 20px;">Settings — loading…${() => (state.ready ? ' ready' : '')}</div>`(root);
}
