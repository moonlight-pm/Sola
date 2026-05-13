// Button showcase — one row per variant, idle + disabled, with a
// press counter that demonstrates onPress wiring.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
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
      <Text tone="muted">
        Pressed {count} time{count === 1 ? "" : "s"}.
      </Text>

      <Stack gap="var(--space-sm)">
        <Text kind="label">default</Text>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button onPress={onPress}>Default</Button>
          <Button onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">primary</Text>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="primary" onPress={onPress}>Primary</Button>
          <Button variant="primary" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">ghost</Text>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="ghost" onPress={onPress}>Ghost</Button>
          <Button variant="ghost" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">danger</Text>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="danger" onPress={onPress}>Danger</Button>
          <Button variant="danger" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>
    </Stack>
  );
}
