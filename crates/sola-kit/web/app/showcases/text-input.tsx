// TextInput showcase — demonstrates controlled-value updates via
// onInput, and the visual states (idle, focus, disabled, invalid).
// The "Live" row echoes the typed value below the input so the
// onInput → handle.update() round-trip is visible.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { Field } from "@sola/field";
import { Stack } from "@sola/stack";
import { TextInput } from "@sola/text-input";

export function TextInputShowcase(handle: Handle) {
  let live = "";
  const onLiveInput = (v: string) => {
    live = v;
    handle.update();
  };

  return () => (
    <Stack gap="var(--space-xxl)">
      <Card
        label="Live preview"
        description="Each row demonstrates a different state — controlled input, type variants, disabled, and invalid border swap."
      >
        <div style="max-width: 360px;">
          <Stack gap="var(--space-lg)">
            <Field
              label="Live"
              help={`onInput → "${live || "(empty)"}"`}
            >
              <TextInput
                value={live}
                placeholder="Type and watch the help text"
                onInput={onLiveInput}
              />
            </Field>

            <Field label="Email" help="type=email — soft validation hint">
              <TextInput type="email" placeholder="you@example.com" />
            </Field>

            <Field label="Password" help="type=password — value is masked">
              <TextInput type="password" placeholder="hunter2" />
            </Field>

            <Field label="Disabled">
              <TextInput value="Cannot edit" disabled />
            </Field>

            <Field
              label="Invalid"
              error="Border swaps to the error color when invalid is set."
            >
              <TextInput value="bad" invalid />
            </Field>
          </Stack>
        </div>
      </Card>

      <BindingsEditor componentName="text-input" />
    </Stack>
  );
}
