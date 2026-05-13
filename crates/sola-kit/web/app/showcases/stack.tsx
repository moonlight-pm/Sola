// Stack showcase — demonstrates each prop axis (direction, gap,
// align, justify, wrap). Tinted accent boxes act as cells so the
// resulting layout is visible without visual rules.

import { type Handle } from "@remix-run/ui";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function StackShowcase(_handle: Handle) {
  const cell = (n: number) => (
    <div style="background: var(--accent-dim); padding: 8px 12px; border-radius: 4px;">
      {n}
    </div>
  );

  return () => (
    <Stack gap="var(--space-xxl)">
      <Stack gap="var(--space-sm)">
        <Text kind="label">direction="column" (default), gap=md</Text>
        <Stack gap="var(--space-md)">
          {cell(1)}
          {cell(2)}
          {cell(3)}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">direction="row", gap=md, align="center"</Text>
        <Stack direction="row" gap="var(--space-md)" align="center">
          {cell(1)}
          {cell(2)}
          {cell(3)}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">direction="row", justify="between"</Text>
        <Stack
          direction="row"
          justify="between"
          align="center"
          gap="var(--space-md)"
        >
          {cell(1)}
          {cell(2)}
          {cell(3)}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <Text kind="label">direction="row", wrap=true</Text>
        <Stack direction="row" gap="var(--space-sm)" wrap>
          {Array.from({ length: 12 }, (_, i) => cell(i + 1))}
        </Stack>
      </Stack>
    </Stack>
  );
}
