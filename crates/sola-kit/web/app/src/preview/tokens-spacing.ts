import { component, html } from '@arrow-js/core';
import { themeState, setSpacing, setRadius } from '../token-edit.js';

const SPACE_FIELDS = ['xs', 'sm', 'md', 'lg', 'xl', 'xxl'];
const RADIUS_FIELDS = ['sm', 'md', 'lg'];

export const spacingView = component(() =>
  html`<div class="kit-spacing">
    <div class="kit-section-title-sm">Spacing</div>
    ${SPACE_FIELDS.map(k => html`
      <div class="kit-type-row">
        <div class="kit-type-label">--space-${k}</div>
        <input class="kit-field" value="${() => themeState.current?.spacing?.[k] ?? ''}"
          @input="${(e: Event) => setSpacing(k, (e.target as HTMLInputElement).value)}">
        <div class="kit-space-sample" style="${() => `width: ${themeState.current?.spacing?.[k] ?? '0'}; height: 12px; background: var(--accent);`}"></div>
      </div>
    `)}
    <div class="kit-section-title-sm" style="margin-top: var(--space-md)">Radius</div>
    ${RADIUS_FIELDS.map(k => html`
      <div class="kit-type-row">
        <div class="kit-type-label">--radius-${k}</div>
        <input class="kit-field" value="${() => themeState.current?.radius?.[k] ?? ''}"
          @input="${(e: Event) => setRadius(k, (e.target as HTMLInputElement).value)}">
        <div class="kit-radius-sample" style="${() => `width: 32px; height: 32px; background: var(--accent-dim); border-radius: ${themeState.current?.radius?.[k] ?? '0'};`}"></div>
      </div>
    `)}
  </div>`
);
