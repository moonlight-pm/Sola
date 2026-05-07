// ColorInput showcase — live editing where the swatch follows the
// typed value via onInput → handle.update(). Demonstrates a few
// CSS color forms (hex, rgba, var, color-mix) and the
// disabled/invalid states.

import { type Handle } from "@remix-run/ui";
import { ColorInput } from "@sola/color-input";
import { Field } from "@sola/field";
import { Stack } from "@sola/stack";

export function ColorInputShowcase(handle: Handle) {
  let live = "var(--accent)";
  const onLiveInput = (v: string) => {
    live = v;
    handle.update();
  };

  return () => (
    <Stack gap="var(--space-xxl)">
      <div style="max-width: 480px;">
        <Stack gap="var(--space-lg)">
          <Field
            label="Live"
            help={`Swatch reflects: ${live || "(empty → transparent)"}`}
          >
            <ColorInput value={live} onInput={onLiveInput} />
          </Field>

          <Field label="Hex">
            <ColorInput value="#161b22" />
          </Field>

          <Field label="rgba">
            <ColorInput value="rgba(0, 212, 255, 0.5)" />
          </Field>

          <Field label="var() reference">
            <ColorInput value="var(--accent)" />
          </Field>

          <Field label="color-mix()">
            <ColorInput value="color-mix(in srgb, var(--accent), transparent 60%)" />
          </Field>

          <Field label="Empty (swatch shows checker)">
            <ColorInput value="" />
          </Field>

          <Field label="Disabled">
            <ColorInput value="#3fb950" disabled />
          </Field>

          <Field label="Invalid" error="Not a recognised CSS color.">
            <ColorInput value="not-a-color" invalid />
          </Field>
        </Stack>
      </div>
    </Stack>
  );
}
