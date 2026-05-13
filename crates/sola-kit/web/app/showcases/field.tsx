// Field showcase — demonstrates label-only, label+help, label+error,
// and labelless variants. Uses TextInput as the contained control to
// also exercise the Field+TextInput pairing that's the standard form
// row in the kit.

import { type Handle } from "@remix-run/ui";
import { Field } from "@sola/field";
import { Stack } from "@sola/stack";
import { TextInput } from "@sola/text-input";

export function FieldShowcase(_handle: Handle) {
  return () => (
    <Stack gap="var(--space-xxl)">
      <div style="max-width: 360px;">
        <Stack gap="var(--space-lg)">
          <Field label="Name">
            <TextInput placeholder="Joshua" />
          </Field>

          <Field label="Email" help="We'll never share it.">
            <TextInput type="email" placeholder="you@example.com" />
          </Field>

          <Field
            label="API token"
            error="Token must be at least 16 characters."
          >
            <TextInput value="abc" invalid />
          </Field>

          <Field help="Plain helper text without a label.">
            <TextInput placeholder="No label here" />
          </Field>
        </Stack>
      </div>
    </Stack>
  );
}
