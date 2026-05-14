// BindingsEditor — per-component slot editor for the bindings
// editor on each showcase page.
//
// The editor shows, for each component the kit knows about, the
// slots grouped into categories ("Surface", "Item", etc. — defined
// in `crates/sola-kit/src/categories.rs`). For each slot, the user
// sees:
//
//   • A label.
//   • A *token picker* — a dropdown of palette atoms eligible for
//     this slot's selection group. Changing the picker re-points
//     the slot at the new token via `theme_set_binding`.
//   • An inline value editor for the *currently-bound* token's
//     global value (ColorInput / FontInput / NumberInput depending
//     on token kind). Edits here flow through `theme_set`, the
//     same path the Tokens page uses, and propagate everywhere the
//     token is referenced.
//
// In other words, the same row both picks "which text-size token
// drives this slot" and lets you tweak that token's actual size
// without leaving the page. The two operations are deliberately
// separate so the picker reflects intent and the value reflects
// rendered output.

import { type Handle } from "@remix-run/ui";
import { Card } from "@sola/card";
import { on } from "@sola/kit";
import { invoke } from "@sola/ipc";
import {
  type Theme,
  type Token,
  getTheme,
  onThemeChange,
} from "@sola/kit";
import { Popover } from "@sola/popover";
import { Text } from "@sola/text";
import { TokenValueEditor } from "@sola/token-value-editor";

interface SlotEntry {
  key: string;
  label: string;
}

interface Category {
  id: string;
  label: string;
  description?: string;
  slots: SlotEntry[];
}

export interface BindingsEditorProps {
  /** Component name as it appears in `Theme.components` keys
      (e.g. `"sidebar"`, `"button"`). */
  componentName: string;
}

export function BindingsEditor(handle: Handle<BindingsEditorProps>) {
  let theme: Theme | null = getTheme();
  let categories: Category[] | null = null;
  let loadError: string | null = null;

  let setupComplete = false;
  // deno-lint-ignore no-unused-vars
  const _dispose = onThemeChange((t) => {
    theme = t;
    if (!setupComplete) return;
    handle.update();
  });
  setupComplete = true;

  async function loadCategories(name: string): Promise<void> {
    try {
      const result = (await invoke("list_categories", { component: name })) as
        | { categories?: Category[]; error?: string };
      if (result?.error) {
        loadError = String(result.error);
        categories = [];
      } else {
        categories = result?.categories ?? [];
      }
    } catch (e) {
      loadError = String(e);
      categories = [];
    }
    handle.update();
  }

  // Component name only changes if a parent re-renders us with a
  // different one — kick off (or re-kick) the fetch when that
  // happens. Lazy on first render avoids fetching for components
  // the user never opens.
  let lastFetchedFor: string | null = null;

  function rebindSlot(slot: string, token: string): void {
    invoke("theme_set_binding", {
      component: handle.props.componentName,
      slot,
      token,
    }).catch((err) => console.error("theme_set_binding failed", err));
  }

  function rewriteToken(name: string, value: string): void {
    if (!theme) return;
    // Same shape `commitToken` in tokens.tsx uses: clone the theme
    // with the named atom's value replaced, then push via
    // theme_set. The bus loopback updates this component along
    // with every other consumer.
    const oldToken = theme.palette.tokens[name];
    if (!oldToken) return;
    const next: Theme = {
      ...theme,
      palette: {
        ...theme.palette,
        tokens: {
          ...theme.palette.tokens,
          [name]: { ...oldToken, value },
        },
      },
    };
    theme = next;
    handle.update();
    invoke("theme_set", { theme: next }).catch((err) =>
      console.error("theme_set failed", err)
    );
  }

  return () => {
    const name = handle.props.componentName;
    if (lastFetchedFor !== name) {
      lastFetchedFor = name;
      void loadCategories(name);
    }

    if (loadError) {
      return (
        <Text tone="muted">
          Could not load bindings editor: {loadError}
        </Text>
      );
    }
    if (categories === null) {
      return <Text tone="muted">Loading…</Text>;
    }
    if (categories.length === 0) {
      return (
        <Text tone="muted">
          No editor metadata for "{name}" yet.
        </Text>
      );
    }
    if (!theme) {
      return <Text tone="muted">Waiting for theme delivery…</Text>;
    }
    const themeRef = theme;

    const comp = themeRef.components[name];
    if (!comp) {
      return (
        <Text tone="muted">
          "{name}" is not in the current theme.
        </Text>
      );
    }

    // The editor is one outer grid; each category is a Card that
    // spans all columns and uses `grid-template-columns: subgrid`
    // to inherit the outer grid's column tracks. That gets us
    // both the visual grouping (cards with header + chrome) and
    // perfect column alignment top-to-bottom across cards — the
    // Label / Picker / Value triplets in card N line up with the
    // ones in card N+1 because they all consume the same outer
    // grid columns. The subgrid override lives in this component's
    // CSS, scoped under `.sola-bindings-editor` so Card stays
    // display-block when used elsewhere.
    return (
      <div class="sola-bindings-editor">
        {categories.map((cat) => (
          <Card label={cat.label} description={cat.description}>
            {cat.slots.flatMap((slot) => {
              const binding = comp.slots[slot.key];
              const slotKey = `${cat.id}/${slot.key}`;
              if (!binding) {
                return [
                  <span class="sola-bindings-editor-label" key={`${slotKey}:lbl`}>
                    {slot.label}
                  </span>,
                  <Text tone="muted" key={`${slotKey}:pick`}>—</Text>,
                  <Text tone="muted" key={`${slotKey}:val`}>
                    slot not in theme
                  </Text>,
                ];
              }
              const tokenName = binding.token;
              const token: Token | undefined =
                themeRef.palette.tokens[tokenName];
              const candidates = candidatesForGroup(themeRef, binding.group);
              const onValueChange = (v: string) => rewriteToken(tokenName, v);
              // Token picker — Popover-based so we never spawn a
              // native OS popup window (CEF's <select> would,
              // which sola-river sees as a real top-level surface
              // and the shell rezones around).
              const renderPickerContent = (
                { close }: { close: () => void },
              ) => (
                <div class="sola-bindings-editor-picker-list">
                  {candidates.map((cand) => {
                    const selected = cand === tokenName;
                    const cls = "sola-bindings-editor-picker-option" +
                      (selected ? " is-selected" : "");
                    return (
                      <button
                        type="button"
                        class={cls}
                        mix={[
                          on("click", () => {
                            rebindSlot(slot.key, cand);
                            close();
                          }),
                        ]}
                      >
                        {cand}
                      </button>
                    );
                  })}
                </div>
              );
              return [
                <span class="sola-bindings-editor-label" key={`${slotKey}:lbl`}>
                  {slot.label}
                </span>,
                <Popover content={renderPickerContent} key={`${slotKey}:pick`}>
                  <span class="sola-bindings-editor-picker">
                    <span class="sola-bindings-editor-picker-label">
                      {tokenName}
                    </span>
                    {chevronIcon()}
                  </span>
                </Popover>,
                <span class="sola-bindings-editor-value" key={`${slotKey}:val`}>
                  {token
                    ? (
                      <TokenValueEditor
                        token={token}
                        onChange={onValueChange}
                      />
                    )
                    : <Text tone="muted">missing token</Text>}
                </span>,
              ];
            })}
          </Card>
        ))}
      </div>
    );
  };
}

function candidatesForGroup(theme: Theme, group: string): string[] {
  const out: string[] = [];
  for (const [name, token] of Object.entries(theme.palette.tokens)) {
    if (token.groups.includes(group)) out.push(name);
  }
  out.sort();
  return out;
}

function chevronIcon() {
  return (
    <svg
      class="sola-bindings-editor-picker-chevron"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}
