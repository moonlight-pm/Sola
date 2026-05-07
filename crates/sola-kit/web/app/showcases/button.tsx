// Button showcase — one row per variant, idle + disabled, with a
// press counter that demonstrates onPress wiring.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { Stack } from "@sola/stack";

export function ButtonShowcase(handle: Handle) {
  let count = 0;
  const onPress = () => {
    count++;
    handle.update();
  };

  const labelStyle =
    "font-size: 11px; opacity: 0.6; text-transform: uppercase;";

  return () => (
    <Stack gap="var(--space-xxl)">
      <p style="opacity: 0.75; margin: 0;">
        Pressed {count} time{count === 1 ? "" : "s"}.
      </p>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>default</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button onPress={onPress}>Default</Button>
          <Button onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>primary</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="primary" onPress={onPress}>Primary</Button>
          <Button variant="primary" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>ghost</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="ghost" onPress={onPress}>Ghost</Button>
          <Button variant="ghost" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>danger</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          <Button variant="danger" onPress={onPress}>Danger</Button>
          <Button variant="danger" onPress={onPress} disabled>Disabled</Button>
        </Stack>
      </Stack>
    </Stack>
  );
}
