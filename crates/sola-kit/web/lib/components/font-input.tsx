// FontInput — a Popover trigger that opens a searchable list of
// installed font families.
//
// Parallel to ColorInput → ColorPicker: trigger button shows the
// current font name *rendered in that font* so the picker reads as
// a real type sample, and clicking opens a popover with a filter
// input + scrollable list. Each list entry renders in its own font
// for instant recognition, mirroring native font menus on macOS /
// Word / design tools.
//
// The font list is fetched from the kit's Rust side via
// `invoke("list_fonts")` — the kit shells out to `fc-list` and
// returns the de-duplicated list of canonical family names with
// CSS generics filtered out. A small refresh button in the popover
// header re-fetches the list on demand (user installs a new font →
// click ↻); the list is otherwise cached for the component's
// lifetime so a re-open is instant.
//
// The Popover is used in controlled mode so picking a family
// closes the popover automatically — the right UX for a select-
// style menu and a deliberate divergence from ColorPicker's stay-
// open behaviour (which is right for that interaction because the
// user keeps tweaking sliders).

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";
import { invoke } from "@sola/ipc";
import { Popover } from "@sola/popover";
import { TextInput } from "@sola/text-input";

export interface FontInputProps {
  /** Current font family. Rendered in itself as the trigger label. */
  value?: string;
  /** Fires when the user picks a different family from the list. */
  onChange?: (value: string) => void;
}

interface FontListResult {
  families?: string[];
  error?: string;
}

export function FontInput(handle: Handle<FontInputProps>) {
  let fonts: string[] | null = null;
  let loading = false;
  let loadError: string | null = null;
  let query = "";

  async function loadFonts() {
    loading = true;
    loadError = null;
    handle.update();
    try {
      const result = (await invoke("list_fonts")) as FontListResult;
      if (result?.error) {
        loadError = String(result.error);
        fonts = [];
      } else {
        fonts = Array.isArray(result?.families) ? result.families : [];
      }
    } catch (e) {
      loadError = String(e);
      fonts = [];
    } finally {
      loading = false;
      handle.update();
    }
  }

  // Lazy first load — kicked off the first time the picker opens
  // (Popover's onOpenChange fires when its uncontrolled state
  // flips to true). Subsequent opens reuse the cached list; the
  // refresh button forces a re-fetch.
  const onOpenChange = (next: boolean) => {
    if (next && fonts === null && !loading) loadFonts();
  };

  const onQueryInput = (v: string) => {
    query = v;
    handle.update();
  };

  return () => {
    const { value, onChange } = handle.props;
    const hasValue = !!value && value.trim() !== "";
    // If the token currently holds a CSS stack (default themes ship
    // `'JetBrains Mono', 'Fira Code', monospace` etc.), show only
    // the first family in the trigger label — the rest are
    // fallback chrome the user shouldn't need to read. The trigger
    // style still uses the full value so the browser keeps using
    // whatever font in the stack is actually installed.
    const triggerLabel = hasValue ? firstFamily(value!) : "Choose font…";
    const triggerStyle = hasValue ? `font-family: ${cssQuote(value!)}` : "";

    const q = query.trim().toLowerCase();
    const filtered = (fonts ?? []).filter((f) =>
      q === "" || f.toLowerCase().includes(q)
    );

    // `close` is supplied by Popover at render time — picking a
    // family commits the change AND closes the picker without
    // having to coordinate state through controlled props.
    const renderContent = ({ close }: { close: () => void }) => {
      const onSelect = (family: string) => {
        onChange?.(family);
        close();
      };
      const onRefresh = (e: Event) => {
        e.stopPropagation();
        loadFonts();
      };
      return (
        <div class="sola-font-input-panel">
          <div class="sola-font-input-header">
            <TextInput
              value={query}
              onInput={onQueryInput}
              placeholder="Filter…"
            />
            <button
              type="button"
              class="sola-font-input-refresh"
              aria-label="Refresh font list"
              title="Refresh font list"
              disabled={loading ? true : false}
              mix={[on("click", onRefresh)]}
            >
              {refreshIcon()}
            </button>
          </div>
          <div class="sola-font-input-list">
            {loading
              ? <div class="sola-font-input-status">Loading…</div>
              : loadError
              ? (
                <div class="sola-font-input-status sola-font-input-status-error">
                  {loadError}
                </div>
              )
              : filtered.length === 0
              ? <div class="sola-font-input-status">No matches</div>
              : filtered.map((family) => {
                // Compare against the primary family of the current
                // value so a stack like `'JetBrains Mono', monospace`
                // still highlights "JetBrains Mono" in the list.
                const selected = hasValue && firstFamily(value!) === family;
                const classes = "sola-font-input-option" +
                  (selected ? " is-selected" : "");
                return (
                  <button
                    type="button"
                    class={classes}
                    style={`font-family: ${cssQuote(family)}`}
                    mix={[on("click", () => onSelect(family))]}
                  >
                    {family}
                  </button>
                );
              })}
          </div>
        </div>
      );
    };

    // Trigger is a <span>, not a <button>, so the wrapping Field's
    // native `<label>` doesn't treat it as the implicit form
    // control to activate when the row is clicked elsewhere — same
    // workaround ColorInput uses. We keep keyboard accessibility
    // light for v1; if real users hit this, swap in role="button"
    // tabindex="0" + a keydown handler.
    return (
      <Popover content={renderContent} onOpenChange={onOpenChange}>
        <span
          class="sola-font-input-trigger"
          style={triggerStyle}
        >
          <span class="sola-font-input-trigger-label">{triggerLabel}</span>
          {chevronIcon()}
        </span>
      </Popover>
    );
  };
}

/** Quote a CSS font-family value defensively. Wraps in double-
 *  quotes and escapes backslashes + interior double-quotes per the
 *  CSS-value syntax — covers names with spaces (most), apostrophes,
 *  or other punctuation that an unquoted bareword form would mis-
 *  parse. Plain identifiers don't strictly need quoting but quoting
 *  uniformly is cheaper than detecting which names do.
 *
 *  Skips quoting for values that already look like a stack (contain
 *  an unquoted comma) so the browser's font-family fallback chain
 *  keeps working: `'JetBrains Mono', monospace` must reach the
 *  parser as multiple comma-separated families, not one quoted
 *  string with a comma in it. */
function cssQuote(s: string): string {
  if (s.includes(",")) return s;
  return `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

/** Strip the trailing fallback families and any wrapping quotes
 *  from a CSS font-family value to recover the user's primary
 *  choice as a plain string. */
function firstFamily(stack: string): string {
  const first = stack.split(",")[0].trim();
  return first.replace(/^['"]|['"]$/g, "");
}

function chevronIcon() {
  return (
    <svg
      class="sola-font-input-chevron"
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

function refreshIcon() {
  return (
    <svg
      class="sola-font-input-refresh-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
      <path d="M8 16H3v5" />
    </svg>
  );
}
