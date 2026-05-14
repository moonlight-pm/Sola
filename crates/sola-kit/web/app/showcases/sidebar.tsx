// Sidebar showcase — the kit-shipped Sidebar component is rendered
// on the left of every storybook page (it's the storybook's nav),
// so the example here is "the very sidebar you're looking at".
//
// The rest of the page is the per-component bindings editor: each
// of Sidebar's slots grouped by category, with a token picker
// (constrained to the slot's selection group) and an inline editor
// for the currently-bound token's global value.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function SidebarShowcase(_handle: Handle) {
  return () => (
    <Stack gap="xl">
      <Text tone="muted">
        Every page in this storybook is framed by the kit-shipped
        Sidebar on the left — that's your live example. The editor
        below lets you re-point each slot at a different token, or
        tweak the bound token's global value inline (same path as
        the Tokens page; changes propagate everywhere the atom is
        referenced).
      </Text>
      <BindingsEditor componentName="sidebar" />
    </Stack>
  );
}
