// Swatch showcase — palette atoms, transparency demo, and size
// variants. The transparency row uses the kit's palette tokens at
// successively-lower alpha so the checker pattern beneath becomes
// progressively visible.

import { type Handle } from "@remix-run/ui";
import { Stack } from "@sola/stack";
import { Swatch } from "@sola/swatch";

const labelStyle =
  "font-size: 11px; opacity: 0.6; text-transform: uppercase;";

const PALETTE_COLORS: { name: string; value: string }[] = [
  { name: "bg-primary", value: "var(--bg-primary)" },
  { name: "bg-secondary", value: "var(--bg-secondary)" },
  { name: "bg-tertiary", value: "var(--bg-tertiary)" },
  { name: "border", value: "var(--border)" },
  { name: "accent", value: "var(--accent)" },
  { name: "accent-dim", value: "var(--accent-dim)" },
  { name: "danger", value: "var(--danger)" },
  { name: "success", value: "var(--success)" },
];

const SIZES = ["12px", "20px", "32px", "48px"];

// Alpha steps reference the accent atom via color-mix so the demo
// follows whatever color the theme has bound `--accent` to —
// changing the accent token recolors this row automatically.
// `color-mix(in srgb, var(--accent), transparent X%)` yields the
// accent at alpha (1 - X/100).
const ALPHA_STEPS = [
  { label: "1.0", value: "var(--accent)" },
  { label: "0.8", value: "color-mix(in srgb, var(--accent), transparent 20%)" },
  { label: "0.5", value: "color-mix(in srgb, var(--accent), transparent 50%)" },
  { label: "0.25", value: "color-mix(in srgb, var(--accent), transparent 75%)" },
  { label: "0.0", value: "transparent" },
];

export function SwatchShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-xxl)">
      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>palette atoms</span>
        <Stack direction="row" gap="var(--space-md)" align="center" wrap>
          {PALETTE_COLORS.map((c) => (
            <Stack gap="var(--space-xs)" align="center">
              <Swatch color={c.value} />
              <span style="font-size: 11px; opacity: 0.7;">{c.name}</span>
            </Stack>
          ))}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>sizes</span>
        <Stack direction="row" gap="var(--space-md)" align="end">
          {SIZES.map((s) => (
            <Stack gap="var(--space-xs)" align="center">
              <Swatch color="var(--accent)" size={s} />
              <span style="font-size: 11px; opacity: 0.7;">{s}</span>
            </Stack>
          ))}
        </Stack>
      </Stack>

      <Stack gap="var(--space-sm)">
        <span style={labelStyle}>transparency (accent at varying alpha)</span>
        <Stack direction="row" gap="var(--space-md)" align="center">
          {ALPHA_STEPS.map((a) => (
            <Stack gap="var(--space-xs)" align="center">
              <Swatch color={a.value} size="32px" />
              <span style="font-size: 11px; opacity: 0.7;">α {a.label}</span>
            </Stack>
          ))}
        </Stack>
      </Stack>
    </Stack>
  );
}
