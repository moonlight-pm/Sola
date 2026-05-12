// Sidebar showcase — the kit-shipped Sidebar component is rendered
// on the left of every storybook page (it's the storybook's nav).
// This page describes that and is where future variant stories
// (icons, trailing badges, disabled items, custom widths) will live.

import { type Handle } from "@remix-run/ui";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function SidebarShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-md)">
      <Text>
        The kit-shipped Sidebar is rendered on the left of this page —
        the storybook navigation itself uses it.
      </Text>
      <Text tone="subtle">
        Variant stories (icons, trailing counts, disabled items,
        custom widths) will go here.
      </Text>
    </Stack>
  );
}
