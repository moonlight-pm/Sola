// Storybook entry. Layout: <Root> as the viewport-filling flex
// column, then a <Split> pairing the kit-shipped <Sidebar> on the
// left with a <Container>-wrapped page on the right. Selection is
// parent-controlled via a closure-captured local; nav and content
// are both derived from the registry in `./showcases/index.ts` so
// adding a new showcase never touches this file.
//
// The page has a persistent header bar at the top: section heading
// on the left, global theme actions (Reset for now; Save / Load
// once theme files land) on the right.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { Container } from "@sola/container";
import { invoke } from "@sola/ipc";
import { Root } from "@sola/root";
import { Sidebar, SidebarSection, SidebarItem } from "@sola/sidebar";
import { Split } from "@sola/split";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

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

function resetTheme() {
  invoke("theme_reset").catch((err) => {
    console.error("theme_reset failed", err);
  });
}

export function Main(handle: Handle) {
  let selectedId = showcases[0]?.id ?? "";

  const select = (id: string) => {
    if (id === selectedId) return;
    selectedId = id;
    handle.update();
  };

  return () => {
    const entry = findShowcase(selectedId);
    const Showcase = entry?.component;
    const heading = entry?.label ?? selectedId;

    return (
      <Root>
        <Split direction="row" position="240px">
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
          <Container maxWidth="article">
            <Stack gap="xl">
              {/* Persistent content header. Section heading on the
                  left, global theme actions on the right. Future
                  additions: theme name + save/load buttons. */}
              <Stack
                direction="row"
                align="center"
                justify="between"
                gap="md"
              >
                <Text kind="display">{heading}</Text>
                <Stack direction="row" gap="sm" align="center">
                  <Button variant="ghost" onPress={resetTheme}>
                    Reset theme
                  </Button>
                </Stack>
              </Stack>
              {Showcase
                ? <Showcase />
                : (
                  <Text tone="muted">
                    No showcase registered for "{selectedId}".
                  </Text>
                )}
            </Stack>
          </Container>
        </Split>
      </Root>
    );
  };
}
