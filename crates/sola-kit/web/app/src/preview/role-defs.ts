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
};
