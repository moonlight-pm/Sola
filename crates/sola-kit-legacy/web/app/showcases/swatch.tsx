// Swatch showcase — palette atoms, semantic-token sizes,
// transparency demo, and the editable mode that absorbed the old
// ColorInput. Every size in this page is from the kit's space
// scale (xs / sm / md / lg / xl / xxl) — pixel values are not
// expressible at the type level, which is the design-system
// guarantee for color affordances.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { Stack } from "@sola/stack";
import { type SwatchSize, Swatch } from "@sola/swatch";
import { Text } from "@sola/text";

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

const SIZES: SwatchSize[] = ["xs", "sm", "md", "lg", "xl", "xxl"];

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

export function SwatchShowcase(handle: Handle) {
  // Editable demo — round-trips a value through the picker so the
  // page can show "you picked X" with a live update.
  let live = "#00d4ff";
  const onLiveChange = (v: string) => {
    live = v;
    handle.update();
  };

  return () => (
    <Stack gap="xxl">
      <Card
        label="Live preview"
        description="Palette atoms, the space-scale sizes, an alpha ramp using color-mix, and the editable mode that opens a ColorPicker on click."
      >
        <Stack gap="xl">
          <Stack gap="sm">
            <Text kind="label">Palette atoms</Text>
            <Stack direction="row" gap="md" align="center" wrap>
              {PALETTE_COLORS.map((c) => (
                <Stack gap="xs" align="center">
                  <Swatch color={c.value} />
                  <Text kind="caption" tone="muted">{c.name}</Text>
                </Stack>
              ))}
            </Stack>
          </Stack>

          <Stack gap="sm">
            <Text kind="label">Sizes (space scale)</Text>
            <Stack direction="row" gap="md" align="end">
              {SIZES.map((s) => (
                <Stack gap="xs" align="center">
                  <Swatch color="var(--accent)" size={s} />
                  <Text kind="caption" tone="muted">{s}</Text>
                </Stack>
              ))}
            </Stack>
          </Stack>

          <Stack gap="sm">
            <Text kind="label">Transparency (accent at varying alpha)</Text>
            <Stack direction="row" gap="md" align="center">
              {ALPHA_STEPS.map((a) => (
                <Stack gap="xs" align="center">
                  <Swatch color={a.value} size="xl" />
                  <Text kind="caption" tone="muted">α {a.label}</Text>
                </Stack>
              ))}
            </Stack>
          </Stack>

          <Stack gap="sm">
            <Text kind="label">Editable (onChange opens the picker)</Text>
            <div style="max-width: 360px;">
              <Stack gap="lg">
                <Field
                  label="Live"
                  help={`Current value: ${live || "(empty)"}`}
                >
                  <Swatch
                    color={live}
                    size="xxl"
                    onChange={onLiveChange}
                  />
                </Field>

                <Field label="Hex">
                  <Swatch color="#161b22" size="xxl" onChange={() => {}} />
                </Field>

                <Field label="Hex + alpha">
                  <Swatch color="#3fb95080" size="xxl" onChange={() => {}} />
                </Field>

                <Field
                  label="var() reference"
                  help="Picker opens at its last HSLA state; the swatch still renders the var."
                >
                  <Swatch
                    color="var(--accent)"
                    size="xxl"
                    onChange={() => {}}
                  />
                </Field>
              </Stack>
            </div>
          </Stack>
        </Stack>
      </Card>

      <BindingsEditor componentName="swatch" />
    </Stack>
  );
}
