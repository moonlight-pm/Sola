// <MenuLabels> — renders menu_labels[1..] as clickable/hoverable items.
//
// menu_labels[0] is the app name and is rendered by <AppTitle>.
// This component iterates the remaining labels (indices 1..n).

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface MenuLabelsProps {
  /** Full menu_labels array; index 0 is the app name, rendered elsewhere. */
  labels: string[];
  /** Returns true if the label at app-index `i` is currently open. */
  isOpen: (key: string) => boolean;
  /** anchorX comes from getBoundingClientRect().left of the label element. */
  onClick: (index: number, anchorX: number) => void;
  onHover: (index: number, anchorX: number) => void;
}

// Per-label item: captures its own DOM ref for getBoundingClientRect.
interface LabelItemProps {
  label: string;
  index: number;
  open: boolean;
  onClick: (index: number, anchorX: number) => void;
  onHover: (index: number, anchorX: number) => void;
}

import { ref } from "@remix-run/ui";

function LabelItem(handle: Handle<LabelItemProps>) {
  let el: HTMLDivElement | null = null;

  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    if (!el) return;
    handle.props.onClick(handle.props.index, el.getBoundingClientRect().left);
  };

  const handleMouseEnter = () => {
    if (!el) return;
    handle.props.onHover(handle.props.index, el.getBoundingClientRect().left);
  };

  const onRef = (elem: HTMLDivElement, signal: AbortSignal) => {
    el = elem;
    signal.addEventListener("abort", () => { el = null; });
  };

  return () => {
    const { label, open } = handle.props;
    const cls = open
      ? "sola-menubar-label sola-menubar-label--open"
      : "sola-menubar-label";
    return (
      <div
        class={cls}
        mix={[
          ref<HTMLDivElement>(onRef),
          on("click", handleClick),
          on("mouseenter", handleMouseEnter),
        ]}
      >
        {label}
      </div>
    );
  };
}

export function MenuLabels(handle: Handle<MenuLabelsProps>) {
  return () => {
    const { labels, isOpen, onClick, onHover } = handle.props;
    // Slice off index 0 (app name, rendered by AppTitle).
    const items = labels.slice(1);
    return (
      <div class="sola-menubar-labels">
        {items.map((label, i) => {
          // i is 0-based within items; the real menu index is i+1.
          const index = i + 1;
          return (
            <LabelItem
              key={String(index)}
              label={label}
              index={index}
              open={isOpen(`app:${index}`)}
              onClick={onClick}
              onHover={onHover}
            />
          );
        })}
      </div>
    );
  };
}
