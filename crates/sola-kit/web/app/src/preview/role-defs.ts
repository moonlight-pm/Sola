// Per-component role definitions. Each entry describes the *roles* the
// component plays (groups + roles), with a default token mapping per
// role. Components not listed here fall back to the chip view in
// component-view.ts.

import type { ComponentRoles } from './role-view.js';

export const ROLE_DEFS: Record<string, ComponentRoles> = {
  button: {
    groups: [
      {
        id: 'shape',
        label: 'Shape',
        description: 'Geometry of the button outline — corner rounding and inset on each axis. Same values across all variants.',
        roles: [
          {
            alias: '--kit-btn-radius',
            label: 'Corner radius',
            description: 'Border-radius applied to every variant. Set to None for square corners.',
            kind: 'radius',
            defaultToken: '--radius-sm',
            allowNone: true,
          },
          {
            alias: '--kit-btn-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on the left and right of the label.',
            kind: 'spacing',
            defaultToken: '--space-md',
          },
          {
            alias: '--kit-btn-pad-y',
            label: 'Vertical padding',
            description: 'Inset above and below the label. Drives the overall button height.',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
        ],
      },
      {
        id: 'type',
        label: 'Type',
        description: 'Label sizing. Family is inherited from the surrounding text.',
        roles: [
          {
            alias: '--kit-btn-size',
            label: 'Label size',
            description: 'Font size of the button text.',
            kind: 'text-size',
            defaultToken: '--text-body',
          },
        ],
      },
      {
        id: 'default',
        label: 'Default variant',
        description: 'Background and text color when no variant prop is passed (the most common state).',
        roles: [
          {
            alias: '--kit-btn-bg',
            label: 'Background',
            description: 'Resting background of the default button.',
            kind: 'color',
            defaultToken: '--bg-tertiary',
          },
          {
            alias: '--kit-btn-fg',
            label: 'Text',
            description: 'Resting label color of the default button.',
            kind: 'color',
            defaultToken: '--text-secondary',
          },
          {
            alias: '--kit-btn-fg-hover',
            label: 'Hover text',
            description: 'Label color when the cursor is over the button.',
            kind: 'color',
            defaultToken: '--text-primary',
          },
        ],
      },
      {
        id: 'primary',
        label: 'Primary variant',
        description: 'Variant used for the dominant call-to-action on a screen.',
        roles: [
          {
            alias: '--kit-btn-primary-bg',
            label: 'Background',
            description: 'Background of the primary CTA. Defaults to a tinted accent fill.',
            kind: 'color',
            defaultToken: '--accent-dim',
          },
          {
            alias: '--kit-btn-primary-fg',
            label: 'Text',
            description: 'Label color of the primary CTA.',
            kind: 'color',
            defaultToken: '--accent',
          },
        ],
      },
      {
        id: 'danger',
        label: 'Danger variant',
        description: 'Destructive-action button. Background is transparent so it sits unobtrusively in toolbars until interacted with.',
        roles: [
          {
            alias: '--kit-btn-danger-fg',
            label: 'Text',
            description: 'Label color of the danger button.',
            kind: 'color',
            defaultToken: '--danger',
          },
        ],
      },
      {
        id: 'add',
        label: 'Add variant',
        description: 'Full-width "+ add item" affordance, dashed outline by default.',
        roles: [
          {
            alias: '--kit-btn-add-border',
            label: 'Border',
            description: 'Color of the dashed outline.',
            kind: 'color',
            defaultToken: '--border-subtle',
          },
          {
            alias: '--kit-btn-add-fg',
            label: 'Text',
            description: 'Label color when not hovered.',
            kind: 'color',
            defaultToken: '--text-secondary',
          },
        ],
      },
    ],
  },

  field: {
    groups: [
      {
        id: 'shape',
        label: 'Shape',
        description: 'Padding and corner rounding of the input box.',
        roles: [
          {
            alias: '--kit-field-radius',
            label: 'Corner radius',
            description: 'Border-radius of the input outline.',
            kind: 'radius',
            defaultToken: '--radius-sm',
            allowNone: true,
          },
          {
            alias: '--kit-field-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on the left and right of the value text.',
            kind: 'spacing',
            defaultToken: '--space-sm',
          },
          {
            alias: '--kit-field-pad-y',
            label: 'Vertical padding',
            description: 'Inset above and below the value text. Drives input height.',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
        ],
      },
      {
        id: 'type',
        label: 'Type',
        description: 'Text style for the field value. Defaults to mono so values like hex codes and paths line up.',
        roles: [
          {
            alias: '--kit-field-font',
            label: 'Family',
            description: 'Font family for the value text. Switch to sans for plain-text fields.',
            kind: 'font',
            defaultToken: '--font-mono',
          },
          {
            alias: '--kit-field-size',
            label: 'Value size',
            description: 'Font size of the value text.',
            kind: 'text-size',
            defaultToken: '--text-body',
          },
        ],
      },
      {
        id: 'surface',
        label: 'Surface',
        description: 'Background, text, and border colors at rest.',
        roles: [
          {
            alias: '--kit-field-bg',
            label: 'Background',
            description: 'Resting background of the input.',
            kind: 'color',
            defaultToken: '--bg-primary',
          },
          {
            alias: '--kit-field-fg',
            label: 'Text',
            description: 'Resting color of the value text.',
            kind: 'color',
            defaultToken: '--text-primary',
          },
          {
            alias: '--kit-field-border',
            label: 'Border',
            description: 'Resting outline color.',
            kind: 'color',
            defaultToken: '--border-subtle',
          },
        ],
      },
      {
        id: 'states',
        label: 'States',
        description: 'Outline colors that override the resting border in specific states.',
        roles: [
          {
            alias: '--kit-field-border-focus',
            label: 'Focused border',
            description: 'Outline color when the input has keyboard focus.',
            kind: 'color',
            defaultToken: '--accent',
          },
          {
            alias: '--kit-field-border-error',
            label: 'Error border',
            description: 'Outline color when the field is in an error state (data-error="error").',
            kind: 'color',
            defaultToken: '--danger',
          },
        ],
      },
    ],
  },

  badge: {
    groups: [
      {
        id: 'shape',
        label: 'Shape',
        description: 'Padding and rounding. Vertical padding is hard-coded at 1px so the badge always sits flush with adjacent text.',
        roles: [
          {
            alias: '--kit-badge-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on each side of the label.',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
          {
            alias: '--kit-badge-radius',
            label: 'Corner radius',
            description: 'Border-radius — same value across all variants.',
            kind: 'radius',
            defaultToken: '--radius-sm',
            allowNone: true,
          },
        ],
      },
      {
        id: 'type',
        label: 'Type',
        description: 'Label sizing. Family is inherited from surrounding text; weight is hard-coded at 500.',
        roles: [
          {
            alias: '--kit-badge-size',
            label: 'Label size',
            description: 'Font size of the badge text.',
            kind: 'text-size',
            defaultToken: '--text-caption',
          },
        ],
      },
      {
        id: 'default',
        label: 'Default variant',
        description: 'Neutral background fill. Used for non-semantic markers like counts or generic tags.',
        roles: [
          {
            alias: '--kit-badge-bg',
            label: 'Background',
            description: 'Fill color of the default badge.',
            kind: 'color',
            defaultToken: '--bg-tertiary',
          },
          {
            alias: '--kit-badge-fg',
            label: 'Text',
            description: 'Label color of the default badge.',
            kind: 'color',
            defaultToken: '--text-secondary',
          },
        ],
      },
      {
        id: 'accent',
        label: 'Accent variant',
        description: 'Highlight badge — used for "new", "live", or active markers.',
        roles: [
          {
            alias: '--kit-badge-accent-bg',
            label: 'Background',
            description: 'Tinted accent fill.',
            kind: 'color',
            defaultToken: '--accent-dim',
          },
          {
            alias: '--kit-badge-accent-fg',
            label: 'Text',
            description: 'Label color on the accent fill.',
            kind: 'color',
            defaultToken: '--accent',
          },
        ],
      },
      {
        id: 'danger',
        label: 'Danger variant',
        description: 'Background uses a fixed rgba tint of the danger color; only the foreground is overridable here.',
        roles: [
          {
            alias: '--kit-badge-danger-fg',
            label: 'Text',
            description: 'Label color of the danger badge.',
            kind: 'color',
            defaultToken: '--danger',
          },
        ],
      },
      {
        id: 'success',
        label: 'Success variant',
        description: 'Background uses a fixed rgba tint of the success color; only the foreground is overridable here.',
        roles: [
          {
            alias: '--kit-badge-success-fg',
            label: 'Text',
            description: 'Label color of the success badge.',
            kind: 'color',
            defaultToken: '--success',
          },
        ],
      },
    ],
  },

  // List has no theme-tweakable roles. Its only style decision — a 1px
  // hairline gap between rows — is intentionally not parameterised
  // (see the comment in kit.css). An empty roles list keeps the role
  // view rendering instead of falling back to chips.
  list: {
    groups: [
      {
        id: 'note',
        label: 'No tweakable roles',
        description: 'List is intentionally minimal — a vertical stack with a 1px hairline gap between rows. Theming a list usually means theming Row.',
        roles: [],
      },
    ],
  },

  row: {
    groups: [
      {
        id: 'shape',
        label: 'Shape',
        description: 'Padding, radius, and the gap between leading slot / label / actions.',
        roles: [
          {
            alias: '--kit-row-pad-y',
            label: 'Vertical padding',
            description: 'Inset above and below the row content.',
            kind: 'spacing',
            defaultToken: '--space-sm',
          },
          {
            alias: '--kit-row-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on the left and right of the row content.',
            kind: 'spacing',
            defaultToken: '--space-md',
          },
          {
            alias: '--kit-row-radius',
            label: 'Corner radius',
            description: 'Rounding of the row\'s background pill.',
            kind: 'radius',
            defaultToken: '--radius-md',
            allowNone: true,
          },
          {
            alias: '--kit-row-gap',
            label: 'Inner gap',
            description: 'Horizontal gap between the leading slot, the label/info column, and the trailing actions.',
            kind: 'spacing',
            defaultToken: '--space-sm',
          },
        ],
      },
      {
        id: 'surface',
        label: 'Surface',
        description: 'Background fill of each row.',
        roles: [
          {
            alias: '--kit-row-bg',
            label: 'Background',
            description: 'Fill color of the row pill.',
            kind: 'color',
            defaultToken: '--bg-secondary',
          },
        ],
      },
      {
        id: 'label',
        label: 'Label',
        description: 'The primary, prominent text of the row. Weight is hard-coded at 500.',
        roles: [
          {
            alias: '--kit-row-label-size',
            label: 'Label size',
            description: 'Font size of the row label.',
            kind: 'text-size',
            defaultToken: '--text-body',
          },
          {
            alias: '--kit-row-label-fg',
            label: 'Label color',
            description: 'Color of the row label text.',
            kind: 'color',
            defaultToken: '--text-primary',
          },
        ],
      },
      {
        id: 'detail',
        label: 'Detail',
        description: 'Secondary, subdued text below the label — typically a path, value, or status.',
        roles: [
          {
            alias: '--kit-row-detail-size',
            label: 'Detail size',
            description: 'Font size of the secondary text.',
            kind: 'text-size',
            defaultToken: '--text-caption',
          },
          {
            alias: '--kit-row-detail-fg',
            label: 'Detail color',
            description: 'Color of the secondary text.',
            kind: 'color',
            defaultToken: '--text-tertiary',
          },
          {
            alias: '--kit-row-detail-font',
            label: 'Detail family',
            description: 'Font family of the secondary text. Defaults to mono so paths and identifiers line up.',
            kind: 'font',
            defaultToken: '--font-mono',
          },
        ],
      },
    ],
  },

  section: {
    groups: [
      {
        id: 'spacing',
        label: 'Spacing',
        description: 'Vertical rhythm — the section\'s outer bottom margin and the gaps between title / description / body.',
        roles: [
          {
            alias: '--kit-section-gap-bottom',
            label: 'Section bottom margin',
            description: 'Space below the section, separating it from whatever follows.',
            kind: 'spacing',
            defaultToken: '--space-lg',
          },
          {
            alias: '--kit-section-title-gap',
            label: 'Title bottom gap',
            description: 'Space below the title, before the description (or body).',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
          {
            alias: '--kit-section-desc-gap',
            label: 'Description bottom gap',
            description: 'Space below the description, before the body.',
            kind: 'spacing',
            defaultToken: '--space-md',
          },
        ],
      },
      {
        id: 'title',
        label: 'Title',
        description: 'Top-level heading of the section. Weight is hard-coded at 600.',
        roles: [
          {
            alias: '--kit-section-title-size',
            label: 'Title size',
            description: 'Font size of the section heading.',
            kind: 'text-size',
            defaultToken: '--text-heading',
          },
          {
            alias: '--kit-section-title-fg',
            label: 'Title color',
            description: 'Color of the section heading text.',
            kind: 'color',
            defaultToken: '--text-primary',
          },
        ],
      },
      {
        id: 'description',
        label: 'Description',
        description: 'Sub-heading paragraph below the title.',
        roles: [
          {
            alias: '--kit-section-desc-size',
            label: 'Description size',
            description: 'Font size of the description text.',
            kind: 'text-size',
            defaultToken: '--text-body',
          },
          {
            alias: '--kit-section-desc-fg',
            label: 'Description color',
            description: 'Color of the description text.',
            kind: 'color',
            defaultToken: '--text-tertiary',
          },
        ],
      },
    ],
  },

  'nav-item': {
    groups: [
      {
        id: 'shape',
        label: 'Shape',
        description: 'Padding and rounding of the nav item button.',
        roles: [
          {
            alias: '--kit-nav-item-pad-y',
            label: 'Vertical padding',
            description: 'Inset above and below the label.',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
          {
            alias: '--kit-nav-item-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on each side of the label.',
            kind: 'spacing',
            defaultToken: '--space-sm',
          },
          {
            alias: '--kit-nav-item-radius',
            label: 'Corner radius',
            description: 'Rounding on the hover/active highlight rectangle.',
            kind: 'radius',
            defaultToken: '--radius-sm',
            allowNone: true,
          },
        ],
      },
      {
        id: 'type',
        label: 'Type',
        description: 'Label sizing.',
        roles: [
          {
            alias: '--kit-nav-item-size',
            label: 'Label size',
            description: 'Font size of the nav item label.',
            kind: 'text-size',
            defaultToken: '--text-body',
          },
        ],
      },
      {
        id: 'rest',
        label: 'Resting',
        description: 'Color when the item is neither hovered nor active. Background is transparent.',
        roles: [
          {
            alias: '--kit-nav-item-fg',
            label: 'Text',
            description: 'Resting label color.',
            kind: 'color',
            defaultToken: '--text-secondary',
          },
        ],
      },
      {
        id: 'hover',
        label: 'Hover',
        description: 'Treatment when the cursor is over an inactive item.',
        roles: [
          {
            alias: '--kit-nav-item-bg-hover',
            label: 'Background',
            description: 'Fill on hover.',
            kind: 'color',
            defaultToken: '--bg-tertiary',
          },
          {
            alias: '--kit-nav-item-fg-hover',
            label: 'Text',
            description: 'Label color on hover.',
            kind: 'color',
            defaultToken: '--text-primary',
          },
        ],
      },
      {
        id: 'active',
        label: 'Active',
        description: 'Treatment for the currently-selected item (data-active="active").',
        roles: [
          {
            alias: '--kit-nav-item-bg-active',
            label: 'Background',
            description: 'Fill of the active item.',
            kind: 'color',
            defaultToken: '--accent-dim',
          },
          {
            alias: '--kit-nav-item-fg-active',
            label: 'Text',
            description: 'Label color of the active item.',
            kind: 'color',
            defaultToken: '--accent',
          },
        ],
      },
    ],
  },

  sidebar: {
    groups: [
      {
        id: 'surface',
        label: 'Surface',
        description: 'Background and right-edge separator that visually anchor the sidebar to the rest of the layout.',
        roles: [
          {
            alias: '--kit-sidebar-bg',
            label: 'Background',
            description: 'Fill color of the sidebar column.',
            kind: 'color',
            defaultToken: '--bg-secondary',
          },
          {
            alias: '--kit-sidebar-border',
            label: 'Right border',
            description: 'Vertical separator color between the sidebar and the work area.',
            kind: 'color',
            defaultToken: '--border-subtle',
            allowNone: true,
          },
        ],
      },
      {
        id: 'spacing',
        label: 'Spacing',
        description: 'Outer padding inside the sidebar column and the gap between stacked nav items.',
        roles: [
          {
            alias: '--kit-sidebar-pad-y',
            label: 'Vertical padding',
            description: 'Inset above the first item and below the last.',
            kind: 'spacing',
            defaultToken: '--space-md',
          },
          {
            alias: '--kit-sidebar-pad-x',
            label: 'Horizontal padding',
            description: 'Inset on each side of nav items.',
            kind: 'spacing',
            defaultToken: '--space-sm',
          },
          {
            alias: '--kit-sidebar-gap',
            label: 'Item gap',
            description: 'Vertical gap between consecutive nav items / titles.',
            kind: 'spacing',
            defaultToken: '--space-xs',
          },
        ],
      },
      {
        id: 'title',
        label: 'Section title',
        description: 'Mini all-caps headers that group nav items (e.g. "Tokens" vs "Atoms").',
        roles: [
          {
            alias: '--kit-sidebar-title-size',
            label: 'Title size',
            description: 'Font size of the section title text.',
            kind: 'text-size',
            defaultToken: '--text-caption',
          },
          {
            alias: '--kit-sidebar-title-color',
            label: 'Title color',
            description: 'Color of the section title text.',
            kind: 'color',
            defaultToken: '--text-muted',
          },
        ],
      },
    ],
  },

  // Icon currently has only one tweakable role — opacity. Color tinting
  // happens via the consumer's text color (see the brightness/saturate
  // filter in kit.css), so there's no foreground role to expose. Size
  // is per-call (passed as `size` opt) rather than a theme alias.
  // Listed for completeness; expand if/when the icon system grows
  // (e.g. proper fill-color via CSS mask).
  icon: {
    groups: [
      {
        id: 'appearance',
        label: 'Appearance',
        description: 'Visual treatment of the rendered icon. Color is inherited from surrounding text via the brightness filter; size is set per call site.',
        roles: [
          // Opacity isn't a token kind we currently support in the role
          // editor (no opacity-tokens pool), so this is intentionally
          // left as a placeholder doc note rather than an editable
          // role. When we add an "opacity" RoleKind, expose it here.
        ],
      },
    ],
  },
};
