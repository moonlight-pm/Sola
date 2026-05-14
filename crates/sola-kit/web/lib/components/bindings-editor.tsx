// BindingsEditor — per-component slot editor for the bindings
// editor on each showcase page.
//
// The editor shows, for each component the kit knows about, the
// slots grouped into categories ("Surface", "Item", etc. — defined
// in `crates/sola-kit/src/categories.rs`). For each slot, the user
// sees:
//
//   • A label.
//   • A *token picker* — a PopoverSelect of palette atoms eligible
//     for this slot's selection group. Changing the picker re-
//     points the slot at the new token via `theme_set_binding`.
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
//
// Editor-wide picker width — every slot's PopoverSelect gets the
// same width, computed from the widest candidate token label
// across *all* slots in the editor (via @chenglou/pretext). One
// computed value flows down to every picker, so the column reads
// as uniform even though each slot has its own candidate set.

import { type Handle, ref } from "@remix-run/ui";
import {
  measureNaturalWidth,
  prepareWithSegments,
} from "@chenglou/pretext";
import { Card } from "@sola/card";
import { invoke } from "@sola/ipc";
import {
  type Theme,
  type Token,
  getTheme,
  onThemeChange,
} from "@sola/kit";
import { PopoverSelect } from "@sola/popover-select";
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

  // Captured by the `ref` mixin on the editor root; cleared on
  // remove. Used as the host node for pretext-driven width
  // measurement so we read the right CSS custom properties from
  // the live theme.
  let editorEl: HTMLElement | null = null;

  // Computed editor-wide picker width in px, or null until the
  // first measurement lands. Every PopoverSelect renders with
  // this width when set, falling back to "auto" while unset.
  let pickerWidth: number | null = null;

  // Cache key for the last measurement (sorted union of all
  // candidate labels + font). recomputePickerWidth short-
  // circuits when the hash hasn't changed.
  let pickerWidthHash = "";

  function recomputePickerWidth() {
    if (!editorEl || !theme || categories === null || categories.length === 0) {
      return;
    }
    const comp = theme.components[handle.props.componentName];
    if (!comp) return;

    const cs = getComputedStyle(editorEl);
    // The trigger CSS reads `--font-mono` and `--text-body` from
    // theme custom properties; pull them through here so the
    // measurement matches what the user actually sees.
    const family = cs.getPropertyValue("--font-mono").trim();
    const size = cs.getPropertyValue("--text-body").trim();
    if (!family || !size) return; // theme hasn't applied yet
    const font = `normal 400 ${size} ${family}`;

    // Collect every candidate token name across every slot's
    // selection group. A Set dedupes; sort makes the hash stable.
    const labels = new Set<string>();
    for (const cat of categories) {
      for (const slot of cat.slots) {
        const binding = comp.slots[slot.key];
        if (!binding) continue;
        for (const cand of candidatesForGroup(theme, binding.group)) {
          labels.add(cand);
        }
      }
    }
    const sorted = [...labels].sort();
    const hash = sorted.join("\x01") + "\x02" + font;
    if (hash === pickerWidthHash) return;
    pickerWidthHash = hash;

    if (sorted.length === 0) {
      pickerWidth = null;
      handle.update();
      return;
    }

    let widest = 0;
    for (const label of sorted) {
      const prepared = prepareWithSegments(label, font);
      const w = measureNaturalWidth(prepared);
      if (w > widest) widest = w;
    }

    // PopoverSelect trigger chrome — match the rule in
    // popover-select.css: padding-inline space-sm × 2, gap
    // space-xs, chevron 12px. We could getComputedStyle on an
    // actual trigger but that requires a mounted node we don't
    // have here; pull the token values directly off the editor
    // root (which inherits the same custom properties).
    const padInline = parseFloat(cs.getPropertyValue("--space-sm")) || 8;
    const gap = parseFloat(cs.getPropertyValue("--space-xs")) || 4;
    const chevron = 12;
    pickerWidth = Math.ceil(widest + padInline * 2 + gap + chevron + 2);
    handle.update();
  }

  let setupComplete = false;
  // deno-lint-ignore no-unused-vars
  const _dispose = onThemeChange((t) => {
    theme = t;
    if (!setupComplete) return;
    // Invalidate the hash so a same-set remeasure runs against
    // the new font.
    pickerWidthHash = "";
    recomputePickerWidth();
    handle.update();
  });
  setupComplete = true;

  const onEditorRef = (node: Element) => {
    editorEl = node as HTMLElement;
    queueMicrotask(recomputePickerWidth);
  };

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
    // New categories → new candidate set → invalidate cache.
    pickerWidthHash = "";
    recomputePickerWidth();
    handle.update();
  }

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
      // Component changed — re-fetch and re-measure.
      pickerWidthHash = "";
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

    // Cheap to call every render — recomputePickerWidth hashes
    // labels+font and short-circuits when nothing changed.
    queueMicrotask(recomputePickerWidth);

    // Width passed to every PopoverSelect. Falls back to "auto"
    // (each picker self-measures) until the editor-wide measure
    // lands; after that, every picker pins to the same width.
    const pickerWidthValue: "auto" | string = pickerWidth != null
      ? `${pickerWidth}px`
      : "auto";

    // The editor is one outer grid; each category is a Card that
    // spans all columns and uses `grid-template-columns: subgrid`
    // to inherit the outer grid's column tracks. That gets us
    // both the visual grouping (cards with header + chrome) and
    // perfect column alignment top-to-bottom across cards.
    return (
      <div class="sola-bindings-editor" mix={[ref(onEditorRef)]}>
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
              const options = candidates.map((c) => ({ value: c }));
              const onValueChange = (v: string) => rewriteToken(tokenName, v);
              return [
                <span class="sola-bindings-editor-label" key={`${slotKey}:lbl`}>
                  {slot.label}
                </span>,
                <PopoverSelect
                  key={`${slotKey}:pick`}
                  options={options}
                  value={tokenName}
                  onChange={(next) => rebindSlot(slot.key, next)}
                  width={pickerWidthValue}
                />,
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
