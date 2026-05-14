// Split showcase — demos a horizontal and vertical split inside
// fixed-size containers, plus the BindingsEditor for the divider
// slots.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { Split } from "@sola/split";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const FRAME =
  "height: 200px; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden;";

const PANE_FILL = (color: string) =>
  `padding: var(--space-md); background: ${color}; height: 100%;`;

export function SplitShowcase(_handle: Handle) {
  return () => (
    <Stack gap="xxl">
      <Card
        label="Live preview"
        description="Drag the divider to resize. Each pane scrolls independently — the universal split-pane convention."
      >
        <Stack gap="lg">
          <Stack gap="sm">
            <Text kind="label">direction="row", position="240px"</Text>
            <div style={FRAME}>
              <Split direction="row" position="240px">
                <div style={PANE_FILL("var(--bg-secondary)")}>
                  <Text>Left pane (240px initial)</Text>
                </div>
                <div style={PANE_FILL("var(--bg-tertiary)")}>
                  <Text>Right pane (fills)</Text>
                </div>
              </Split>
            </div>
          </Stack>

          <Stack gap="sm">
            <Text kind="label">direction="column", position="40%"</Text>
            <div style={FRAME}>
              <Split direction="column" position="40%">
                <div style={PANE_FILL("var(--bg-secondary)")}>
                  <Text>Top pane (40% initial)</Text>
                </div>
                <div style={PANE_FILL("var(--bg-tertiary)")}>
                  <Text>Bottom pane (fills)</Text>
                </div>
              </Split>
            </div>
          </Stack>
        </Stack>
      </Card>

      <BindingsEditor componentName="split" />
    </Stack>
  );
}
