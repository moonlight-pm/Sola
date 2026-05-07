// Stack showcase — demonstrates each prop axis (direction, gap,
// align, justify, wrap). Tinted accent boxes act as cells so the
// resulting layout is visible without visual rules.

import { type Handle } from "@remix-run/ui";
import { Stack } from "@sola/stack";

export function StackShowcase(_handle: Handle) {
  const labelStyle =
    "font-size: 11px; opacity: 0.6; text-transform: uppercase;";
  const cell = (n: number) => (
    <div style="background: var(--accent-dim); padding: 8px 12px; border-radius: 4px;">
      {n}
    </div>
  );

  return () => (
    <Stack gap="var(--space-xxl)">
      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>direction="column" (default), gap=md</span>
        <Stack gap="var(--space-md)">
          {cell(1)}
          {cell(2)}
          {cell(3)}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>direction="row", gap=md, align="center"</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          {cell(1)}
          {cell(2)}
          {cell(3)}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>direction="row", justify="between"</span>
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
        <span style={labelStyle}>direction="row", wrap=true</span>
        <Stack direction="row" gap="var(--space-sm)" wrap>
          {Array.from({ length: 12 }, (_, i) => cell(i + 1))}
        </Stack>
      </Stack>
    </Stack>
  );
}
