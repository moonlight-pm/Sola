// Settings frontend root. Task 3 scaffold: just a Root with a
// placeholder. The full Split/Sidebar/Container/panels structure
// lands in Tasks 4-7.

import { type Handle } from "@remix-run/ui";
import { Root } from "@sola/root";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function Main(_handle: Handle) {
  return () => (
    <Root>
      <Stack gap="md" align="center" justify="center" fill>
        <Text kind="display">Settings</Text>
        <Text tone="muted">scaffold</Text>
      </Stack>
    </Root>
  );
}
