// <AppTitle> — the focused application name displayed in the menubar.
//
// Clicking it opens/toggles menu index 0 ("app" source). Only clickable when
// there are menu labels available (same guard as the legacy menubar.ts).

import { type Handle, ref } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface AppTitleProps {
  name: string;
  open: boolean;
  /** Whether there are menu labels to show (menu_labels.length > 0). */
  clickable: boolean;
  onClick: (anchorX: number) => void;
  onHover: (anchorX: number) => void;
}

export function AppTitle(handle: Handle<AppTitleProps>) {
  let el: HTMLDivElement | null = null;

  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    if (!el || !handle.props.clickable) return;
    handle.props.onClick(el.getBoundingClientRect().left);
  };

  const handleMouseEnter = () => {
    if (!el || !handle.props.clickable) return;
    handle.props.onHover(el.getBoundingClientRect().left);
  };

  const onRef = (elem: HTMLDivElement, signal: AbortSignal) => {
    el = elem;
    signal.addEventListener("abort", () => { el = null; });
  };

  return () => {
    const { name, open } = handle.props;
    const cls = open
      ? "sola-menubar-app-title sola-menubar-app-title--open"
      : "sola-menubar-app-title";
    return (
      <div
        class={cls}
        mix={[
          ref<HTMLDivElement>(onRef),
          on("click", handleClick),
          on("mouseenter", handleMouseEnter),
        ]}
      >
        {name}
      </div>
    );
  };
}
