import { html } from '@arrow-js/core';

export function mount(target: HTMLElement) {
  html`
    <div class="kit-shell">
      <h1>sola-kit</h1>
      <p>storybook scaffolding</p>
    </div>
  `(target);
}
