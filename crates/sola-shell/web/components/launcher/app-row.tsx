// <AppRow> — one result row in the launcher panel.
//
// Renders an icon placeholder (Unicode ⬡) and the application label.
// The selected modifier swaps the background and text colors.
//
// NOTE: Legacy used <img src="sola-assets://icons/…"> which does not
// resolve in CEF (no sola-assets:// scheme registered). The Unicode
// placeholder ⬡ is used for all entries until asset-bundle or scheme
// plumbing is added in a follow-up pass.

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface AppEntry {
  app_id: string;
  label: string;
  icon: string;
}

interface Props {
  app: AppEntry;
  selected: boolean;
  onHover: () => void;
  onClick: () => void;
}

export function AppRow(handle: Handle<Props>) {
  return () => {
    const cls =
      "sola-launcher-row" +
      (handle.props.selected ? " sola-launcher-row--selected" : "");
    return (
      <div
        class={cls}
        mix={[on("mouseenter", handle.props.onHover), on("click", handle.props.onClick)]}
      >
        <div class="sola-launcher-icon">
          <span>{"⬡"}</span>
        </div>
        <span class="sola-launcher-label">{handle.props.app.label}</span>
      </div>
    );
  };
}
