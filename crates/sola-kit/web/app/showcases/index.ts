// Showcase registry — single source of truth for what shows up in
// the storybook's navigation. Each entry pairs a stable id (used for
// selection state) with a Remix v3 component factory. Sections are
// derived from the `section` field; section ordering matches first
// appearance in this list.
//
// Adding a new component story is a two-step:
//   1. Drop `web/app/showcases/<name>.tsx` exporting a Remix v3
//      factory (handle: Handle) => RenderFn.
//   2. Add an entry to the array below.
//
// `main.tsx` never changes.

import { type Handle, type RemixNode } from "@remix-run/ui";

import { ButtonShowcase } from "./button.tsx";
import { FieldShowcase } from "./field.tsx";
import { SidebarShowcase } from "./sidebar.tsx";
import { StackShowcase } from "./stack.tsx";
import { SwatchShowcase } from "./swatch.tsx";
import { TextInputShowcase } from "./text-input.tsx";

export interface ShowcaseEntry {
  /** Stable id; used as the selectedId in the storybook's nav. */
  id: string;
  /** Display label for the sidebar entry and the page heading. */
  label: string;
  /** Section header in the sidebar. First appearance defines order. */
  section: string;
  /** Remix v3 component factory rendering the showcase page. */
  component: (handle: Handle) => () => RemixNode;
}

export const showcases: ShowcaseEntry[] = [
  { id: "button", label: "Button", section: "Components", component: ButtonShowcase },
  { id: "field", label: "Field", section: "Components", component: FieldShowcase },
  { id: "sidebar", label: "Sidebar", section: "Components", component: SidebarShowcase },
  { id: "swatch", label: "Swatch", section: "Components", component: SwatchShowcase },
  { id: "text-input", label: "TextInput", section: "Components", component: TextInputShowcase },
  { id: "stack", label: "Stack", section: "Layout", component: StackShowcase },
];

/** Lookup by id; returns `undefined` if no such showcase exists. */
export function findShowcase(id: string): ShowcaseEntry | undefined {
  return showcases.find((s) => s.id === id);
}
