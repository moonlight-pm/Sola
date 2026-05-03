import { component, html, reactive } from '@arrow-js/core';
import { themeState, colorEditor } from '../token-edit.js';
import type { CatalogEntry } from '../sidebar.js';

const COLOR_FIELDS: Array<{ field: string; var: string }> = [
  { field: 'bg_primary',     var: '--bg-primary' },
  { field: 'bg_secondary',   var: '--bg-secondary' },
  { field: 'bg_tertiary',    var: '--bg-tertiary' },
  { field: 'bg_hover',       var: '--bg-hover' },
  { field: 'border',         var: '--border' },
  { field: 'border_subtle',  var: '--border-subtle' },
  { field: 'text_primary',   var: '--text-primary' },
  { field: 'text_secondary', var: '--text-secondary' },
  { field: 'text_tertiary',  var: '--text-tertiary' },
  { field: 'text_muted',     var: '--text-muted' },
  { field: 'text_accent',    var: '--text-accent' },
  { field: 'accent',         var: '--accent' },
  { field: 'accent_dim',     var: '--accent-dim' },
  { field: 'danger',         var: '--danger' },
  { field: 'success',        var: '--success' },
];

interface ColorsProps {
  catalog: CatalogEntry[];
}

export const colorsView = component((props: ColorsProps) => {
  const local = reactive({ openVar: '--accent' });
  return html`
    <div class="kit-colors">
      <div class="kit-colors-list">
        ${COLOR_FIELDS.map(f => html`
          <button
            class="kit-color-row"
            data-active="${() => local.openVar === f.var ? 'active' : false}"
            @click="${() => { local.openVar = f.var; }}"
          >
            <span class="kit-color-swatch" style="${() => `background: ${themeState.current?.colors?.[f.field] ?? ''}`}"></span>
            <span class="kit-color-name">${f.var}</span>
            <span class="kit-color-value">${() => themeState.current?.colors?.[f.field] ?? ''}</span>
          </button>
        `)}
      </div>
      <div class="kit-colors-detail">
        ${() => {
          const entry = COLOR_FIELDS.find(f => f.var === local.openVar);
          if (!entry) return null;
          const used = props.catalog.filter(e => e.tokens.includes(entry.var));
          return html`
            ${() => colorEditor({ field: entry.field, varName: entry.var, used })}
            <div class="kit-affected">
              <div class="kit-section-title-sm">Used in</div>
              ${() => used.length === 0
                ? html`<div class="kit-empty">No components use this token.</div>`
                : html`<ul class="kit-affected-list">${used.map(c => html`<li>${c.name}</li>`)}</ul>`}
            </div>
          `;
        }}
      </div>
    </div>
  `;
});
