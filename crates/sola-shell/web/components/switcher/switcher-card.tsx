// <SwitcherCard> — one app tile in the switcher overlay.
//
// Props:
//   app      — { app_id, name, icon } entry from the switcher render envelope
//   selected — whether this card is the active selection
//   onHover  — callback fired on mouseenter to update selection via invoke

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface AppEntry {
  app_id: string;
  name: string;
  icon: string;
}

interface Props {
  app: AppEntry;
  selected: boolean;
  onHover: () => void;
}

export function SwitcherCard(handle: Handle<Props>) {
  return () => {
    const cls =
      "sola-switcher-card" +
      (handle.props.selected ? " sola-switcher-card--selected" : "");
    return (
      <div class={cls} mix={[on("mouseenter", handle.props.onHover)]}>
        <div class="sola-switcher-icon">
          {/* Unicode placeholder — sola-assets:// scheme doesn't resolve in CEF */}
          <span>{"⬡"}</span>
        </div>
        <div class="sola-switcher-name">{handle.props.app.name}</div>
      </div>
    );
  };
}
