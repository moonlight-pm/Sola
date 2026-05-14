// Popover showcase — a row of triggers, one per placement variant,
// each opening a panel below/above and aligned left/right. Click
// outside to close; opening one closes any other.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Button } from "@sola/button";
import { Card } from "@sola/card";
import { Popover, type PopoverPlacement } from "@sola/popover";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const PLACEMENTS: PopoverPlacement[] = [
  "bottom-start",
  "bottom-end",
  "top-start",
  "top-end",
];

function panelFor(placement: PopoverPlacement) {
  return (
    <Stack gap="var(--space-xs)">
      <Text kind="label">{placement}</Text>
      <Text>
        Popover content. Click outside, or the trigger again, to close.
      </Text>
    </Stack>
  );
}

export function PopoverShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-xxl)">
      <Card
        label="Live preview"
        description="Single popover open globally — click a second trigger and the first closes automatically."
      >
        <Stack gap="var(--space-sm)">
          <Text kind="label">Placements</Text>
          <Stack direction="row" gap="var(--space-lg)" wrap>
            {PLACEMENTS.map((p) => (
              <Popover placement={p} content={panelFor(p)}>
                <Button variant="ghost">{p}</Button>
              </Popover>
            ))}
          </Stack>
        </Stack>
      </Card>

      <BindingsEditor componentName="popover" />
    </Stack>
  );
}
