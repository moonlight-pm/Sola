// Sidebar showcase — the kit-shipped Sidebar component is rendered
// on the left of every storybook page (it's the storybook's nav).
// This page describes that and is where future variant stories
// (icons, trailing badges, disabled items, custom widths) will live.

import { type Handle } from "@remix-run/ui";
import { Stack } from "@sola/stack";

export function SidebarShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-md)">
      <p style="opacity: 0.85; margin: 0;">
        The kit-shipped Sidebar is rendered on the left of this page —
        the storybook navigation itself uses it.
      </p>
      <p style="opacity: 0.6; margin: 0;">
        Variant stories (icons, trailing counts, disabled items,
        custom widths) will go here.
      </p>
    </Stack>
  );
}
