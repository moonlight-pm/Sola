import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { Sidebar, SidebarSection, SidebarItem } from "@sola/sidebar";

// Storybook layout: kit-shipped <Sidebar> on the left, content pane on
// the right. Selection is parent-controlled — we track the current id
// in a closure-captured local and call `handle.update()` when it
// changes, then pass `active={...}` and `onSelect={...}` per item.
//
// Items are label-only here (the storybook's preference); other apps
// can use the `leading`/`trailing` named slots for icons or counters.

interface NavEntry {
  id: string;
  label: string;
}

interface NavSection {
  label: string;
  items: NavEntry[];
}

const sections: NavSection[] = [
  {
    label: "Components",
    items: [
      { id: "button", label: "Button" },
      { id: "sidebar", label: "Sidebar" },
    ],
  },
  {
    label: "Theme",
    items: [
      { id: "palette", label: "Palette" },
      { id: "bindings", label: "Bindings" },
    ],
  },
];

export function Main(handle: Handle) {
  let selectedId = "button";

  const select = (id: string) => {
    if (id === selectedId) return;
    selectedId = id;
    handle.update();
  };

  const layoutStyle =
    "display: flex; height: 100vh; width: 100vw; min-height: 0; " +
    "background: var(--sola-page-bg); color: var(--sola-page-text); " +
    "font-family: var(--sola-page-font); font-size: var(--sola-page-text-size);";
  const contentStyle =
    "flex: 1 1 auto; padding: 24px 32px; overflow: auto;";

  return () => (
    <main style={layoutStyle}>
      <Sidebar>
        {sections.map((section) => (
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
        <h1 style="margin-top: 0;">{labelFor(selectedId)}</h1>
        {selectedId === "button"
          ? <ButtonShowcase />
          : (
            <p style="opacity: 0.75;">
              Storybook content for "{selectedId}" goes here.
            </p>
          )}
      </section>
    </main>
  );
}

// ── Button storybook page ───────────────────────────────────────────
//
// Renders one row per variant with idle and disabled instances. The
// `pressedFlash` lets clicks visibly do something — a tiny "pressed
// N times" line below each row, demonstrating onPress wiring.

function ButtonShowcase(handle: Handle) {
  let count = 0;
  const onPress = () => {
    count++;
    handle.update();
  };

  const rowStyle = "display: flex; gap: 12px; align-items: center;";
  const groupStyle = "display: flex; flex-direction: column; gap: 8px;";
  const labelStyle = "font-size: 11px; opacity: 0.6; text-transform: uppercase;";

  return () => (
    <div style="display: flex; flex-direction: column; gap: 24px; max-width: 640px;">
      <p style="opacity: 0.75; margin: 0;">
        Pressed {count} time{count === 1 ? "" : "s"}.
      </p>
      <div style={groupStyle}>
        <span style={labelStyle}>default</span>
        <div style={rowStyle}>
          <Button onPress={onPress}>Default</Button>
          <Button onPress={onPress} disabled>Disabled</Button>
        </div>
      </div>
      <div style={groupStyle}>
        <span style={labelStyle}>primary</span>
        <div style={rowStyle}>
          <Button variant="primary" onPress={onPress}>Primary</Button>
          <Button variant="primary" onPress={onPress} disabled>Disabled</Button>
        </div>
      </div>
      <div style={groupStyle}>
        <span style={labelStyle}>ghost</span>
        <div style={rowStyle}>
          <Button variant="ghost" onPress={onPress}>Ghost</Button>
          <Button variant="ghost" onPress={onPress} disabled>Disabled</Button>
        </div>
      </div>
      <div style={groupStyle}>
        <span style={labelStyle}>danger</span>
        <div style={rowStyle}>
          <Button variant="danger" onPress={onPress}>Danger</Button>
          <Button variant="danger" onPress={onPress} disabled>Disabled</Button>
        </div>
      </div>
    </div>
  );
}

function labelFor(id: string): string {
  for (const s of sections) {
    for (const i of s.items) {
      if (i.id === id) return i.label;
    }
  }
  return id;
}
