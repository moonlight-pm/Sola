// Badge showcase — one row per kind with idle samples + a context
// example showing a Badge inline with surrounding Text. Bindings
// editor below for live theming.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Badge, type BadgeKind } from "@sola/badge";
import { Card } from "@sola/card";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const KINDS: BadgeKind[] = [
  "neutral",
  "info",
  "success",
  "warning",
  "danger",
];

export function BadgeShowcase(handle: Handle) {
  return () => (
    <Stack gap="xxl">
      <Card
        label="Live preview"
        description="Each row is one kind. The context example shows a Badge inline with surrounding text."
      >
        <Stack gap="lg">
          {KINDS.map((k) => (
            <Stack gap="xs">
              <Text kind="label">{k}</Text>
              <Stack direction="row" gap="md" align="center">
                <Badge kind={k}>{k}</Badge>
                <Badge kind={k}>longer label text</Badge>
              </Stack>
            </Stack>
          ))}
          <Stack gap="xs">
            <Text kind="label">in context</Text>
            <Stack direction="row" gap="sm" align="center">
              <Text>Firefox</Text>
              <Badge kind="warning">not found</Badge>
            </Stack>
          </Stack>
        </Stack>
      </Card>
      <BindingsEditor component="badge" />
    </Stack>
  );
}
