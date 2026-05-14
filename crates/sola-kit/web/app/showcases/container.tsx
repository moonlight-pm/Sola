// Container showcase — demos every semantic max-width tag plus a
// custom CSS length, with the BindingsEditor for the padding slots.

import { type Handle } from "@remix-run/ui";
import { BindingsEditor } from "@sola/bindings-editor";
import { Card } from "@sola/card";
import { Container, type MaxWidthTag } from "@sola/container";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";

const TAGS: MaxWidthTag[] = ["narrow", "reading", "wide", "full"];

const FRAME =
  "background: var(--bg-secondary); border: 1px dashed var(--border-subtle); border-radius: var(--radius-md);";

const SAMPLE = (
  <Text>
    Container is the centered max-width column with themed padding.
    Resize the window to see the column stop growing past its
    configured max.
  </Text>
);

export function ContainerShowcase(_handle: Handle) {
  return () => (
    <Stack gap="xxl">
      <Card
        label="Semantic tags"
        description="The four built-in tags — hardcoded widths chosen for typography, not theming."
      >
        <Stack gap="lg">
          {TAGS.map((tag) => (
            <Stack gap="xs">
              <Text kind="label">maxWidth="{tag}"</Text>
              <div style={FRAME}>
                <Container maxWidth={tag}>{SAMPLE}</Container>
              </div>
            </Stack>
          ))}
        </Stack>
      </Card>

      <Card
        label="Custom CSS length"
        description='Pass any string (e.g. "640px", "70ch") for a per-call width that escapes the tag scale.'
      >
        <Stack gap="xs">
          <Text kind="label">maxWidth="640px"</Text>
          <div style={FRAME}>
            <Container maxWidth="640px">{SAMPLE}</Container>
          </div>
        </Stack>
      </Card>

      <BindingsEditor componentName="container" />
    </Stack>
  );
}
