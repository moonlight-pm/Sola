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

/**
 * Controls handed to a render-function `content`. The popover's
 * imperative close is exposed so a selection inside the panel can
 * dismiss the popover without round-tripping a controlled-mode
 * prop through the parent.
 */
export interface PopoverContentControls {
  close: () => void;
}

export type PopoverContent =
  | RemixNode
  | ((controls: PopoverContentControls) => RemixNode);

export interface PopoverProps {
  /**
   * The floating panel content rendered when open. Either a
   * RemixNode (static) or a render function receiving controls
   * for imperative close.
   */
  content: PopoverContent;
  /** Where to anchor the panel relative to the trigger. */
  placement?: PopoverPlacement;
  /** Trigger element(s). Click toggles; outside click closes. */
  children?: RemixNode;
  /**
   * Optional controlled-mode open state. When provided, Popover
   * reads its open state from this prop instead of internal state,
   * and emits `onOpenChange` whenever it would otherwise self-
   * toggle (trigger click, outside-click, peer popover opening).
   * Both props must be present together; passing only one degrades
   * to uncontrolled behaviour.
   */
  open?: boolean;
  /** See `open`. Receives the next open state. */
  onOpenChange?: (open: boolean) => void;
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

  // Internal state for the uncontrolled case. When the consumer
  // passes `open` + `onOpenChange` we read the prop directly and
  // mutate via the callback; this field is ignored. Mixed usage
  // (only one of the two) falls back to uncontrolled.
  let uncontrolledOpen = false;

  const isControlled = () =>
    handle.props.open !== undefined && handle.props.onOpenChange !== undefined;

  const readOpen = () =>
    isControlled() ? !!handle.props.open : uncontrolledOpen;

  const setOpen = (next: boolean) => {
    if (isControlled()) {
      handle.props.onOpenChange?.(next);
      handle.update();
    } else {
      if (uncontrolledOpen === next) return;
      uncontrolledOpen = next;
      // onOpenChange fires as a notification in uncontrolled mode
      // too — consumers like FontInput use it to kick off lazy work
      // on first open (e.g. fetching the font list).
      handle.props.onOpenChange?.(next);
      handle.update();
    }
  };

  const close = () => {
    if (!readOpen()) return;
    if (currentOpen && currentOpen.close === close) currentOpen = null;
    setOpen(false);
  };

  const open = () => {
    if (readOpen()) return;
    if (currentOpen && currentOpen.close !== close) currentOpen.close();
    currentOpen = { close };
    setOpen(true);
  };

  const onTriggerClick = (e: Event) => {
    // Critical: ignore clicks whose target is inside the popover
    // *content*, even though they bubble up here. A consumer that
    // closes the popover from inside its content (FontInput's
    // option-click → onChange + close) schedules a re-render via
    // handle.update; CEF/Chromium drain Remix's microtasks between
    // bubble-phase listeners, so by the time this handler fires the
    // popover-content has already been removed from the DOM and the
    // `onContentClick` stop-propagation it carried never ran. Re-
    // toggling here would silently re-open the just-closed popover.
    // The `closest()` check guards against that without depending on
    // microtask ordering.
    const target = e.target as HTMLElement | null;
    if (target?.closest?.(".sola-popover-content")) return;
    e.stopPropagation();
    if (readOpen()) close();
    else open();
  };

  // Clicks landing on the popover content must not bubble to the
  // root's onClick (which would toggle the popover closed). This is
  // belt-and-braces alongside the `closest()` check in
  // `onTriggerClick`: it covers the common case where the content
  // stays mounted across the bubble.
  const onContentClick = (e: Event) => {
    e.stopPropagation();
  };

  return () => {
    const placement = handle.props.placement ?? "bottom-start";
    const isOpen = readOpen();
    // Keep the module-singleton tracker honest under controlled
    // mode: if the parent flips us open/closed without going through
    // a trigger click (e.g. close-on-select), update the bookkeeping
    // so the document outside-click listener and peer popovers see
    // the right state.
    if (isOpen) {
      if (currentOpen && currentOpen.close !== close) currentOpen.close();
      currentOpen = { close };
    } else if (currentOpen && currentOpen.close === close) {
      currentOpen = null;
    }
    const rawContent = handle.props.content;
    const resolvedContent: RemixNode = typeof rawContent === "function"
      ? (rawContent as (c: PopoverContentControls) => RemixNode)({ close })
      : rawContent;

    return (
      <span class="sola-popover-root" mix={[on("click", onTriggerClick)]}>
        {handle.props.children}
        {isOpen
          ? (
            <span
              class={`sola-popover-content sola-popover-${placement}`}
              mix={[on("click", onContentClick)]}
            >
              {resolvedContent}
            </span>
          )
          : ""}
      </span>
    );
  };
}
