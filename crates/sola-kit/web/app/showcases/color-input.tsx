// ColorInput showcase — the swatch trigger opens the picker
// popover; sliders + hex input edit the value. The "Live" row
// shows the picker round-trip wired up through onChange.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { ColorInput } from "@sola/color-input";
import { Field } from "@sola/field";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

export function ColorInputShowcase(handle: Handle) {
  let live = "#00d4ff";
  const onLiveChange = (v: string) => {
    live = v;
    handle.update();
  };

  return () => (
    <Stack gap="var(--space-xxl)">
      <Card
        label="Live preview"
        description="Click any swatch to open the HSL + alpha picker. Outside click closes; opening a second picker closes the first."
      >
        <div style="max-width: 480px;">
          <Stack gap="var(--space-lg)">
            <Field
              label="Live"
              help={`Current value: ${live || "(empty)"}`}
            >
              <ColorInput value={live} onChange={onLiveChange} />
            </Field>

            <Field label="Hex">
              <ColorInput value="#161b22" />
            </Field>

            <Field label="rgba">
              <ColorInput value="rgba(0, 212, 255, 0.5)" />
            </Field>

            <Field label="Hex + alpha">
              <ColorInput value="#3fb95080" />
            </Field>

            <Field
              label="var() reference"
              help="Picker opens at its last HSLA state; the swatch still renders the var."
            >
              <ColorInput value="var(--accent)" />
            </Field>

            <Field label="Empty (swatch shows checker)">
              <ColorInput value="" />
            </Field>
          </Stack>
        </div>
      </Card>

      <BindingsEditor componentName="color-input" />
    </Stack>
  );
}
