import { html } from '@arrow-js/core';
import { themeState, setTypography } from '../token-edit.js';

const TYPE_FIELDS: Array<{ field: string; var: string; label: string }> = [
  { field: 'font_sans',     var: '--font-sans',     label: 'Sans family' },
  { field: 'font_mono',     var: '--font-mono',     label: 'Mono family' },
  { field: 'text_caption',  var: '--text-caption',  label: 'Caption (11)' },
  { field: 'text_body',     var: '--text-body',     label: 'Body (12)' },
  { field: 'text_body_lg',  var: '--text-body-lg',  label: 'Body L (13)' },
  { field: 'text_heading',  var: '--text-heading',  label: 'Heading (16)' },
  { field: 'text_display',  var: '--text-display',  label: 'Display (20)' },
];

export function renderTypography() {
  return html`
    <div class="kit-typography">
      ${TYPE_FIELDS.map(f => html`
        <div class="kit-type-row">
          <div class="kit-type-label">${f.label} <span class="kit-type-var">${f.var}</span></div>
          <input class="kit-field" value="${() => themeState.current?.typography?.[f.field] ?? ''}"
            @input="${(e: Event) => setTypography(f.field, (e.target as HTMLInputElement).value)}">
          <div class="kit-type-sample" style="${() => f.field.startsWith('font_')
            ? `font-family: ${themeState.current?.typography?.[f.field] ?? 'inherit'};`
            : `font-size: ${themeState.current?.typography?.[f.field] ?? 'inherit'};`}">The quick brown fox</div>
        </div>
      `)}
    </div>
  `;
}
