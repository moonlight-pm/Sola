import { component, html } from '@arrow-js/core';
import { sidebar, navItem } from '@sola/kit';

export interface CatalogEntry {
  name: string;
  group: 'atom' | 'component';
  tokens: string[];
}

export interface SidebarState {
  selected: string;        // id like "tokens.colors", "atoms.button", "components.row"
}

export const TOKEN_ITEMS = [
  { id: 'tokens.colors',     label: 'Colors' },
  { id: 'tokens.typography', label: 'Typography' },
  { id: 'tokens.spacing',    label: 'Spacing & radius' },
];

interface SidebarProps {
  state: SidebarState;
  catalog: CatalogEntry[];
  onSelect: (id: string) => void;
}

export const appSidebar = component((props: SidebarProps) => {
  const navWith = (id: string, label: string) => navItem({
    label,
    active: () => props.state.selected === id,
    onClick: () => props.onSelect(id),
  });

  // Wrap the sidebar Cmp in an html template — the component factory
  // must return an ArrowTemplate (callable), not a Cmp descriptor.
  return html`${() => sidebar({
    body: html`
      <div class="kit-sidebar-title">Tokens</div>
      ${() => TOKEN_ITEMS.map(t => navWith(t.id, t.label))}
      <div class="kit-sidebar-title">Atoms</div>
      ${() => props.catalog.filter(e => e.group === 'atom').map(a => navWith(`atoms.${a.name}`, capitalise(a.name)))}
      <div class="kit-sidebar-title">Components</div>
      ${() => props.catalog.filter(e => e.group === 'component').map(c => navWith(`components.${c.name}`, capitalise(c.name)))}
    `,
  })}`;
});

function capitalise(s: string) {
  return s.split('-').map(p => p.charAt(0).toUpperCase() + p.slice(1)).join('');
}
