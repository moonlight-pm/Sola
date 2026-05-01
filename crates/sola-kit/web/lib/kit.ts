//! Sola Kit — design tokens, atoms, components.
//
// Importing this module installs a bus listener that applies any
// Topic::Theme broadcasts the framework forwards. Apps don't need to
// opt in beyond the import.

import { on } from '@sola/ipc';

/** Apply a map of CSS custom properties to :root. */
export function applyTheme(vars: Record<string, string>): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key.startsWith('--') ? key : `--${key}`, value);
  }
}

// Self-install: framework forwards Topic::Theme as { event: 'theme', vars }.
on('theme', (payload: { vars: Record<string, string> }) => {
  if (payload && payload.vars) applyTheme(payload.vars);
});

export { button, buttonTokens } from './components/button.js';
export type { ButtonOpts, ButtonVariant } from './components/button.js';
export { field, fieldTokens } from './components/field.js';
export type { FieldOpts } from './components/field.js';
export { badge, badgeTokens } from './components/badge.js';
export type { BadgeOpts, BadgeVariant } from './components/badge.js';
export { icon, iconTokens } from './components/icon.js';
export type { IconOpts } from './components/icon.js';
export { sidebar, sidebarTokens } from './components/sidebar.js';
export type { SidebarOpts } from './components/sidebar.js';
export { navItem, navItemTokens } from './components/nav-item.js';
export type { NavItemOpts } from './components/nav-item.js';
export { section, sectionTokens } from './components/section.js';
export type { SectionOpts } from './components/section.js';
export { row, rowTokens } from './components/row.js';
export type { RowOpts } from './components/row.js';
export { list, listTokens } from './components/list.js';
export type { ListOpts } from './components/list.js';
export { form, fieldRow, formTokens } from './components/form.js';
export type { FormOpts, FieldRowOpts } from './components/form.js';
export { tabs, tab, tabsTokens } from './components/tabs.js';
export type { TabsOpts, TabOpts, TabVariant } from './components/tabs.js';
export { toast, toastTokens } from './components/toast.js';
export type { ToastOpts } from './components/toast.js';
export { empty, emptyTokens } from './components/empty.js';
export type { EmptyOpts } from './components/empty.js';
