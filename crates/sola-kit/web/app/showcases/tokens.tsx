// Tokens editor — the first functional storybook page. Reads the
// current theme via `getTheme()` / `onThemeChange()` (populated by
// the bus pump's structured `definition` push), groups palette
// atoms by kind, and renders one Field per token: ColorInput for
// `Color`, TextInput for everything else.
//
// Edits commit on blur/Enter via `onChange` (not `onInput` — token
// editing isn't a hot loop, and committing per-keystroke would
// generate a Rust round-trip per character). Each commit:
//
//   1. Mutates a local Theme clone with the new token value.
//   2. Calls `handle.update()` so the input reflects the local
//      copy immediately.
//   3. invoke("theme_set", { theme }) — Rust persists, emits
//      Topic::Theme, and the bus pump pushes a fresh CSS +
//      definition back via `__solaRecv`.
//   4. The kit's onThemeChange listener (subscribed in this
//      factory) overwrites the local theme with the round-tripped
//      copy and re-renders. The two should be identical; this
//      step exists so external mutations (a future theme editor
//      in another window, or a `theme_reset` press) propagate.
//
// Reset is a Button that invokes `theme_reset`; the round-trip
// then refreshes the visible token rows.

import { type Handle } from "@remix-run/ui";
import { Button } from "@sola/button";
import { ColorInput } from "@sola/color-input";
import { Field } from "@sola/field";
import {
  type Theme,
  type Token,
  type TokenKind,
  getTheme,
  onThemeChange,
} from "@sola/kit";
import { invoke } from "@sola/ipc";
import { Stack } from "@sola/stack";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";

// Display order for the kind groups. The serialized payload uses
// alphabetical (Color, FontFamily, Radius, Space, TextSize); we
// override that for a more semantic top-to-bottom flow: visual
// (color, font), then sizing (text → space → radius).
const KIND_ORDER: TokenKind[] = [
  "Color",
  "FontFamily",
  "TextSize",
  "Space",
  "Radius",
];

const KIND_LABELS: Record<TokenKind, string> = {
  Color: "Colors",
  FontFamily: "Fonts",
  TextSize: "Text sizes",
  Space: "Spacing",
  Radius: "Radii",
};

export function TokensShowcase(handle: Handle) {
  // Local mutable copy of the theme. Initialised from getTheme()
  // (already populated by the time the user navigates here) and
  // refreshed on every `theme` event delivery.
  let theme: Theme | null = getTheme();

  // Subscribe once at mount. The dispose function is captured but
  // not currently called — Remix v3 has no unmount hook, and the
  // showcase lives effectively forever in the storybook context.
  // If a real leak shows up, mark the showcase with an
  // `unmounted` flag and short-circuit the listener body.
  //
  // `onThemeChange` fires the listener synchronously if a theme is
  // already in memory at subscribe time. Calling `handle.update()`
  // during that sync fire throws — we're still inside the
  // component setup function and the reconciler hasn't wired up
  // `scheduleUpdate` yet (component.ts: setScheduleUpdate is
  // called *after* render returns). The initial value is already
  // captured via `getTheme()` above, so the `setupComplete` gate
  // skips the redundant update during the sync replay and only
  // forwards real subsequent theme events.
  let setupComplete = false;
  // deno-lint-ignore no-unused-vars
  const _dispose = onThemeChange((t) => {
    theme = t;
    if (!setupComplete) return;
    handle.update();
  });
  setupComplete = true;

  /**
   * Build a fresh Theme with the given palette token's value
   * replaced. We allocate a new object for every level the editor
   * touches (theme → palette → tokens → token) so reactivity
   * downstream sees referentially distinct state, and so an
   * external observer holding the old theme isn't surprised by
   * mutation.
   */
  function withTokenValue(
    base: Theme,
    name: string,
    value: string,
  ): Theme {
    const oldToken = base.palette.tokens[name];
    if (!oldToken) return base;
    return {
      ...base,
      palette: {
        ...base.palette,
        tokens: {
          ...base.palette.tokens,
          [name]: { ...oldToken, value },
        },
      },
    };
  }

  function commitToken(name: string, value: string) {
    if (!theme) return;
    theme = withTokenValue(theme, name, value);
    handle.update();
    invoke("theme_set", { theme }).catch((err) => {
      console.error("theme_set failed", err);
    });
  }

  function reset() {
    invoke("theme_reset").catch((err) => {
      console.error("theme_reset failed", err);
    });
  }

  return () => {
    if (!theme) {
      return (
        <Stack gap="var(--space-md)">
          <Text tone="subtle">
            Waiting for the first theme delivery from the bus…
          </Text>
        </Stack>
      );
    }

    // Bucket tokens by kind. Object.entries returns the BTreeMap-
    // serialized order (alphabetical), which gives stable output
    // within each bucket.
    const byKind: Record<TokenKind, [string, Token][]> = {
      Color: [],
      FontFamily: [],
      TextSize: [],
      Space: [],
      Radius: [],
    };
    for (const [name, token] of Object.entries(theme.palette.tokens)) {
      byKind[token.kind].push([name, token]);
    }

    // Container max-width is deliberate — at the storybook's full
    // 1150 px width the tokens read as a sparse list of micro-controls.
    // 880 px keeps two columns of inline-label rows comfortably wide
    // (label band + control + breathing room) without going past where
    // a reader naturally tracks.
    const containerStyle =
      "max-width: 880px; margin: 0 auto; width: 100%;";

    const cardStyle = [
      "background: var(--bg-secondary)",
      "border: 1px solid var(--border-subtle)",
      "border-radius: var(--radius-lg)",
      "padding: var(--space-lg) var(--space-xl)",
    ].join("; ");

    const cardHeaderStyle = [
      "display: flex",
      "align-items: baseline",
      "justify-content: space-between",
      "gap: var(--space-md)",
      "padding-bottom: var(--space-md)",
      "border-bottom: 1px solid var(--border-subtle)",
      "margin-bottom: var(--space-md)",
    ].join("; ");

    // 2-column grid by default; the FontFamily group has long
    // freetext values so it gets a single column for readability.
    const gridStyle = (kind: TokenKind) => {
      const cols = kind === "FontFamily" ? 1 : 2;
      return [
        "display: grid",
        `grid-template-columns: repeat(${cols}, minmax(0, 1fr))`,
        "column-gap: var(--space-xl)",
        "row-gap: var(--space-sm)",
      ].join("; ");
    };

    return (
      <div style={containerStyle}>
        <Stack gap="var(--space-xxl)">
          <Stack
            direction="row"
            justify="between"
            align="center"
            gap="var(--space-md)"
          >
            <Text tone="muted">
              Edit palette atoms — every component slot bound to a
              token follows the change automatically.
            </Text>
            <Button variant="ghost" onPress={reset}>Reset</Button>
          </Stack>

          {KIND_ORDER.map((kind) => {
            const entries = byKind[kind];
            if (entries.length === 0) return "";
            return (
              <section style={cardStyle}>
                <div style={cardHeaderStyle}>
                  <Text kind="label">{KIND_LABELS[kind]}</Text>
                  <Text tone="subtle">{String(entries.length)}</Text>
                </div>
                <div style={gridStyle(kind)}>
                  {entries.map(([name, token]) => {
                    const helpText = token.groups.length
                      ? `groups: ${token.groups.join(", ")}`
                      : "";
                    const onChange = (v: string) => commitToken(name, v);
                    return (
                      <Field
                        direction="row"
                        label={name}
                        title={helpText}
                      >
                        {kind === "Color"
                          ? <ColorInput value={token.value} onChange={onChange} />
                          : <TextInput value={token.value} onChange={onChange} />}
                      </Field>
                    );
                  })}
                </div>
              </section>
            );
          })}
        </Stack>
      </div>
    );
  };
}
