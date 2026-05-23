// <SystemMenu> — the Sola logo button at the leftmost position of the menubar.
//
// Fetches the SVG on first render and appends it inline so `fill="currentColor"`
// inherits the themed `--sola-menubar-system-menu-fg` color from the CSS.
// The logo name is a compile-time constant — change SYSTEM_LOGO to switch.

import { type Handle, ref } from "@remix-run/ui";
import { on } from "@sola/kit";

const SYSTEM_LOGO = "pillars";

export interface SystemMenuProps {
  open: boolean;
  onClick: (anchorX: number) => void;
  onHover: (anchorX: number) => void;
}

export function SystemMenu(handle: Handle<SystemMenuProps>) {
  // Captured from the ref mixin — needed for getBoundingClientRect().
  let containerEl: HTMLDivElement | null = null;

  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    if (!containerEl) return;
    handle.props.onClick(containerEl.getBoundingClientRect().left);
  };

  const handleMouseEnter = () => {
    if (!containerEl) return;
    handle.props.onHover(containerEl.getBoundingClientRect().left);
  };

  // Fetch and inline the SVG once when the element is first mounted.
  // The ref callback runs once on mount (and again with null on unmount).
  // We use a guard flag so re-renders after open-state changes don't
  // trigger another fetch.
  let svgLoaded = false;

  const onRef = (el: HTMLDivElement, signal: AbortSignal) => {
    containerEl = el;
    if (!svgLoaded && !el.querySelector("svg")) {
      svgLoaded = true;
      fetch(`/assets/${SYSTEM_LOGO}.svg`)
        .then((r) => r.text())
        .then((svg) => {
          if (signal.aborted) return;
          const doc = new DOMParser().parseFromString(svg, "image/svg+xml");
          const root = doc.documentElement;
          if (root.tagName.toLowerCase() === "svg") {
            el.appendChild(document.adoptNode(root));
          }
        })
        .catch(() => {
          // If the SVG fails to load, the button is still usable — just empty.
        });
    }
    signal.addEventListener("abort", () => {
      containerEl = null;
    });
  };

  return () => {
    const { open } = handle.props;
    const cls = open
      ? "sola-menubar-system-menu sola-menubar-system-menu--open"
      : "sola-menubar-system-menu";
    return (
      <div
        class={cls}
        mix={[
          ref<HTMLDivElement>(onRef),
          on("click", handleClick),
          on("mouseenter", handleMouseEnter),
        ]}
      />
    );
  };
}
