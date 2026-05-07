// Storybook entry. Layout: <Root> at top, then a flex row of the
// kit-shipped <Sidebar> on the left and the selected showcase on
// the right. Selection is parent-controlled via a closure-captured
// local; nav and content are both derived from the registry in
// `./showcases/index.ts` so adding a new showcase never touches
// this file.

import { type Handle } from "@remix-run/ui";
import { Root } from "@sola/root";
import { Sidebar, SidebarSection, SidebarItem } from "@sola/sidebar";

import { findShowcase, showcases } from "./showcases/index.ts";

interface NavSection {
  label: string;
  items: { id: string; label: string }[];
}

/** Group the registry's showcases by `section`, preserving first-
    appearance order for both sections and items. */
function buildNavSections(): NavSection[] {
  const map = new Map<string, { id: string; label: string }[]>();
  for (const s of showcases) {
    if (!map.has(s.section)) map.set(s.section, []);
    map.get(s.section)!.push({ id: s.id, label: s.label });
  }
  return Array.from(map.entries()).map(([label, items]) => ({ label, items }));
}

const navSections = buildNavSections();

export function Main(handle: Handle) {
  let selectedId = showcases[0]?.id ?? "";

  const select = (id: string) => {
    if (id === selectedId) return;
    selectedId = id;
    handle.update();
  };

  // <Root> handles bg/text/font; the inner div is just the
  // app-specific layout (sidebar row + content column).
  const layoutStyle =
    "display: flex; height: 100%; width: 100%; min-height: 0;";
  const contentStyle =
    "flex: 1 1 auto; padding: 24px 32px; overflow: auto;";

  return () => {
    const entry = findShowcase(selectedId);
    const Showcase = entry?.component;
    const heading = entry?.label ?? selectedId;

    return (
      <Root>
        <div style={layoutStyle}>
          <Sidebar>
            {navSections.map((section) => (
              <SidebarSection label={section.label}>
                {section.items.map((item) => (
                  <SidebarItem
                    active={item.id === selectedId}
                    onSelect={() => select(item.id)}
                  >
                    {item.label}
                  </SidebarItem>
                ))}
              </SidebarSection>
            ))}
          </Sidebar>
          <section style={contentStyle}>
            <h1 style="margin-top: 0;">{heading}</h1>
            {Showcase
              ? <Showcase />
              : (
                <p style="opacity: 0.75;">
                  No showcase registered for "{selectedId}".
                </p>
              )}
          </section>
        </div>
      </Root>
    );
  };
}
