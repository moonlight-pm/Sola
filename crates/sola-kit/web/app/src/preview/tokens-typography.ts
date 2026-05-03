import { component, html } from '@arrow-js/core';
import { themeState, setTypography } from '../token-edit.js';
import { fontPicker } from '../font-picker.js';

interface TypeField {
  field: string;
  var: string;
  label: string;
  kind: 'sans' | 'mono' | 'size';
}

const TYPE_FIELDS: TypeField[] = [
  { field: 'font_sans',     var: '--font-sans',     label: 'Sans family', kind: 'sans' },
  { field: 'font_mono',     var: '--font-mono',     label: 'Mono family', kind: 'mono' },
  { field: 'text_caption',  var: '--text-caption',  label: 'Caption',     kind: 'size' },
  { field: 'text_body',     var: '--text-body',     label: 'Body',        kind: 'size' },
  { field: 'text_body_lg',  var: '--text-body-lg',  label: 'Large Body',  kind: 'size' },
  { field: 'text_heading',  var: '--text-heading',  label: 'Heading',     kind: 'size' },
  { field: 'text_display',  var: '--text-display',  label: 'Display',     kind: 'size' },
];

const SPECIMEN = 'The quick brown fox jumps over the lazy dog';

interface FontList { sans: string[]; mono: string[] }

function fonts(): FontList {
  return ((window as unknown as { RESTORED_STATE?: { fonts?: FontList } }).RESTORED_STATE?.fonts) ?? { sans: [], mono: [] };
}

export const typographyView = component(() =>
  html`<div class="kit-typography">
    ${TYPE_FIELDS.map(f => typeRow({ field: f }))}
  </div>`
);

const typeRow = component((props: { field: TypeField }) => {
  const value = () => themeState.current?.typography?.[props.field.field] ?? '';
  const previewStyle = () => {
    const v = themeState.current?.typography?.[props.field.field] ?? 'inherit';
    return props.field.kind === 'size' ? `font-size: ${v}` : `font-family: ${v}`;
  };
  return html`
    <div class="kit-type-card">
      <div class="kit-type-card-head">
        <span class="kit-type-card-label">${() => props.field.label}</span>
        <span class="kit-type-card-var">${() => props.field.var}</span>
      </div>
      <div class="kit-type-card-control">
        ${() => props.field.kind === 'size'
          ? html`<input class="kit-field" value="${value}"
              @input="${(e: Event) => setTypography(props.field.field, (e.target as HTMLInputElement).value)}">`
          : fontPicker({
              id: `font:${props.field.field}`,
              value,
              options: () => props.field.kind === 'mono' ? fonts().mono : fonts().sans,
              onChange: (v: string) => setTypography(props.field.field, v),
            })}
      </div>
      <div class="kit-type-card-preview" style="${previewStyle}">${SPECIMEN}</div>
    </div>
  `;
});
