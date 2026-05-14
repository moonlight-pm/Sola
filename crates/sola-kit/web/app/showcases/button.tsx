// Button showcase — one row per variant (idle + disabled) sitting
// above the per-component bindings editor. The press counter
// demonstrates onPress wiring; every binding the editor surfaces
// drives the live buttons above through the same theme protocol.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function ButtonShowcase(handle: Handle) {
  let count = 0;
  const onPress = () => {
    count++;
    handle.update();
  };

  return () => (
    <Stack gap="var(--space-xxl)">
      <Card
        label="Live preview"
        description="Each row is one variant in idle and disabled states. The press counter at the top reflects onPress wiring."
      >
        <Stack gap="var(--space-lg)">
          <Text tone="muted">
            Pressed {count} time{count === 1 ? "" : "s"}.
          </Text>

          <Stack gap="var(--space-xs)">
            <Text kind="label">default</Text>
            <Stack direction="row" gap="var(--space-md)" align="center">
              <Button onPress={onPress}>Default</Button>
              <Button onPress={onPress} disabled>Disabled</Button>
            </Stack>
          </Stack>

          <Stack gap="var(--space-xs)">
            <Text kind="label">primary</Text>
            <Stack direction="row" gap="var(--space-md)" align="center">
              <Button variant="primary" onPress={onPress}>Primary</Button>
              <Button variant="primary" onPress={onPress} disabled>
                Disabled
              </Button>
            </Stack>
          </Stack>

          <Stack gap="var(--space-xs)">
            <Text kind="label">ghost</Text>
            <Stack direction="row" gap="var(--space-md)" align="center">
              <Button variant="ghost" onPress={onPress}>Ghost</Button>
              <Button variant="ghost" onPress={onPress} disabled>
                Disabled
              </Button>
            </Stack>
          </Stack>

          <Stack gap="var(--space-xs)">
            <Text kind="label">danger</Text>
            <Stack direction="row" gap="var(--space-md)" align="center">
              <Button variant="danger" onPress={onPress}>Danger</Button>
              <Button variant="danger" onPress={onPress} disabled>
                Disabled
              </Button>
            </Stack>
          </Stack>
        </Stack>
      </Card>

      <BindingsEditor componentName="button" />
    </Stack>
  );
}
