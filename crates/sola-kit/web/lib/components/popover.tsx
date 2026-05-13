// Popover — a floating panel anchored below (or above) a trigger.
//
// Usage:
//
//   <Popover content={<MyPanel/>}>
//     <button>click me</button>
//   </Popover>
//
// Children are the trigger; `content` is what appears when open.
// Click on the trigger toggles; click outside closes; only one
// popover is open globally at a time (opening a second one closes
// the first).
//
// Positioning is via `position: absolute` against the wrapping
// `<span class="sola-popover-root">` — no portal, no Floating UI
// dependency. Placement defaults to `bottom-start` (top: 100%; left:
// 0) and supports the four corner variants.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

export type PopoverPlacement =
  | "bottom-start"
  | "bottom-end"
  | "top-start"
  | "top-end";

export interface PopoverProps {
  /** The floating panel content rendered when open. */
  content: RemixNode;
  /** Where to anchor the panel relative to the trigger. */
  placement?: PopoverPlacement;
  /** Trigger element(s). Click toggles; outside click closes. */
  children?: RemixNode;
}

// Module-singleton state — only one popover instance may be open at
// a time across the page. The currently-open instance registers its
// own `close` function; opening another instance calls the existing
// one's close first.
let currentOpen: { close: () => void } | null = null;
let documentListenerInstalled = false;

function installDocumentListener(): void {
  if (documentListenerInstalled) return;
  documentListenerInstalled = true;
  // `click` fires after `mousedown`+`mouseup` on the SAME element,
  // so a press-and-drag inside the popover doesn't close it. Using
  // mousedown here would close on any press outside before the
  // resulting click could reach a button inside the popover.
  document.addEventListener("click", (e) => {
    if (currentOpen === null) return;
    const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
    const inside = path.some((n) => {
      const el = n as HTMLElement;
      return el.classList && el.classList.contains("sola-popover-root");
    });
    if (!inside) currentOpen.close();
  });
}

export function Popover(handle: Handle<PopoverProps>) {
  installDocumentListener();

  let isOpen = false;

  const close = () => {
    if (!isOpen) return;
    isOpen = false;
    if (currentOpen && currentOpen.close === close) currentOpen = null;
    handle.update();
  };

  const open = () => {
    if (isOpen) return;
    if (currentOpen && currentOpen.close !== close) currentOpen.close();
    isOpen = true;
    currentOpen = { close };
    handle.update();
  };

  const onTriggerClick = (e: Event) => {
    // Stop the click from bubbling to the document listener (which
    // would see the click as "outside" because the listener fires
    // *after* this one in the bubble path… wait, it doesn't — we
    // installed it on `document` so it's the last to fire. The real
    // reason: prevent app-level click handlers above this Popover
    // from also reacting to a trigger press).
    e.stopPropagation();
    if (isOpen) close();
    else open();
  };

  // Clicks landing on the popover content must not bubble to the
  // root's onClick (which would toggle the popover closed).
  const onContentClick = (e: Event) => {
    e.stopPropagation();
  };

  return () => {
    const placement = handle.props.placement ?? "bottom-start";
    return (
      <span class="sola-popover-root" mix={[on("click", onTriggerClick)]}>
        {handle.props.children}
        {isOpen
          ? (
            <span
              class={`sola-popover-content sola-popover-${placement}`}
              mix={[on("click", onContentClick)]}
            >
              {handle.props.content}
            </span>
          )
          : ""}
      </span>
    );
  };
}
