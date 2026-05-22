// <MenuItem> — a single entry in the dropdown menu.
//
// Data shape mirrors open_menu's Rust serialization:
//   Divider:  { type: "divider" }
//   Action:   { type: "action", id, app_id, label, shortcut?, disabled? }

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";
import { invoke } from "@sola/ipc";

export type MenuItemData =
  | { type: "divider" }
  | {
      type: "action";
      label: string;
      id: string;
      app_id: string;
      shortcut?: string | null;
      disabled?: boolean;
    };

interface Props {
  item: MenuItemData;
}

export function MenuItem(handle: Handle<Props>) {
  return () => {
    const item = handle.props.item;

    if (item.type === "divider") {
      return <div class="sola-menu-divider" />;
    }

    const cls =
      "sola-menu-item" + (item.disabled ? " sola-menu-item--disabled" : "");

    if (item.disabled) {
      return (
        <div class={cls}>
          <span class="sola-menu-item-label">{item.label}</span>
          {item.shortcut ? (
            <span class="sola-menu-item-shortcut">{item.shortcut}</span>
          ) : null}
        </div>
      );
    }

    const onClick = () =>
      invoke("action", { app_id: item.app_id, action_id: item.id });

    return (
      <div class={cls} mix={[on("click", onClick)]}>
        <span class="sola-menu-item-label">{item.label}</span>
        {item.shortcut ? (
          <span class="sola-menu-item-shortcut">{item.shortcut}</span>
        ) : null}
      </div>
    );
  };
}
