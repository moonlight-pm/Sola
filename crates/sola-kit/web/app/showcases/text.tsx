// Text showcase — one row per kind (display / heading / body-lg /
// body / caption / label) plus a column of tone overlays (default /
// muted / subtle). The `as` prop is implicit per row; the showcase
// doesn't exercise it directly — it's just an escape hatch most
// callers won't need.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const KINDS = ["display", "heading", "body-lg", "body", "caption", "label"] as const;
const TONES = ["default", "muted", "subtle"] as const;

const SAMPLE = "The quick brown fox jumps over the lazy dog";

export function TextShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-xxl)">
      <Card
        label="Live preview"
        description="One row per kind, then a column of tone overlays applied to body kind."
      >
        <Stack gap="var(--space-xl)">
          <Stack gap="var(--space-md)">
            <Text kind="label" tone="muted">Kinds</Text>
            <Stack gap="var(--space-md)">
              {KINDS.map((k) => (
                <Stack gap="var(--space-xs)">
                  <Text kind="label" tone="subtle">{k}</Text>
                  <Text kind={k}>{SAMPLE}</Text>
                </Stack>
              ))}
            </Stack>
          </Stack>

          <Stack gap="var(--space-md)">
            <Text kind="label" tone="muted">Tones (body kind)</Text>
            <Stack gap="var(--space-md)">
              {TONES.map((t) => (
                <Stack gap="var(--space-xs)">
                  <Text kind="label" tone="subtle">{t}</Text>
                  <Text tone={t}>{SAMPLE}</Text>
                </Stack>
              ))}
            </Stack>
          </Stack>
        </Stack>
      </Card>

      <BindingsEditor componentName="text" />
    </Stack>
  );
}
