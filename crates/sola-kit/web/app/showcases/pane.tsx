// Pane showcase — describes that the page itself uses a Pane and
// shows a constrained one with overflowing content to demonstrate
// the themed padding and the scroll behaviour.

import { type Handle } from "@remix-run/ui";
import { Pane } from "@sola/pane";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const PARAGRAPHS = Array.from({ length: 12 }, (_, i) => `
  Paragraph ${i + 1}. Pane is the scrollable padded content area
  used inside every kit app page. The block/inline padding comes
  from the theme (\`--sola-pane-padding-{block,inline}\`); the
  background bleeds through from Root so a theme bg swap recolours
  every Pane automatically.
`);

const demoContainerStyle =
  "height: 240px; display: flex; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden;";

export function PaneShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-xxl)">
      <Text tone="muted">
        The page you are reading is a Pane — the content area
        between the sidebar and the right edge of the window.
      </Text>

      <Stack gap="var(--space-sm)">
        <Text kind="label">scrolling demo (240px tall)</Text>
        <div style={demoContainerStyle}>
          <Pane as="div">
            <Stack gap="var(--space-md)">
              {PARAGRAPHS.map((p) => <Text>{p}</Text>)}
            </Stack>
          </Pane>
        </div>
      </Stack>
    </Stack>
  );
}
