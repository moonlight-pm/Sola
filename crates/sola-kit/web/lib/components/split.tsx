// Split — two-child resizable layout primitive. The kit's "split
// pane" container.
//
//   <Split direction="row" position="280px">
//     <Sidebar />
//     <Container>{...}</Container>
//   </Split>
//
// Each child sits inside its own scroll area (overflow:auto) — the
// universal split-pane convention. A 1px themed divider with a wider
// invisible hit area lives between the two; dragging it resizes the
// FIRST pane along the main axis. Split has no other layout
// responsibilities — for everything else, compose with Stack /
// Container / etc.
//
// Position: required, no default. Accepted units are px or %.
//   "280px" — pixel-based; drag emits px.
//   "30%"   — percent-based; drag emits %.
//
// Stored state: `position` is the initial seed read on first render.
// Drag mutates Split's internal state; the prop isn't re-read on
// subsequent renders. To re-init from stored state, the app supplies
// the value as `position` once it's loaded (typically by gating
// Split's render until the load completes — Remix v3 closure-state
// idiom). For persistence, supply `onResize` and write the emitted
// string back to wherever you keep it.
//
// Children: exactly two are expected. The first is the start pane
// (top in column, left in row); the second absorbs remaining space.

import { type Handle, type RemixNode, ref } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface SplitProps {
  /** Main-axis orientation. Required, no default. */
  direction: "row" | "column";
  /** Initial size of the first pane along the main axis. CSS length
      string in px or %. Drag updates Split's internal state from
      here; this prop is read once at mount. */
  position: string;
  /** Fires after every drag step with the new position string in
      the same unit as `position` ("280px" or "30%"). Use to persist
      to whatever store the app keeps. */
  onResize?: (position: string) => void;
  /** Exactly two children expected. */
  children?: RemixNode;
}

const SIZE_RE = /^(-?\d*\.?\d+)([a-z%]*)$/i;

function parsePx(value: string, total: number): { px: number; unit: string } {
  const m = value.match(SIZE_RE);
  if (!m) return { px: 0, unit: "px" };
  const num = parseFloat(m[1]);
  const unit = m[2] || "px";
  const px = unit === "%" ? (num / 100) * total : num;
  return { px, unit };
}

function formatPx(px: number, total: number, unit: string): string {
  return unit === "%"
    ? `${((px / total) * 100).toFixed(2)}%`
    : `${Math.round(px)}px`;
}

export function Split(handle: Handle<SplitProps>) {
  // Closure-captured live state. Seeded from props on first read.
  let livePosition = handle.props.position;
  let containerEl: HTMLElement | null = null;
  let dividerEl: HTMLElement | null = null;
  let dragStart: {
    coord: number;
    px: number;
    total: number;
    unit: string;
  } | null = null;

  const isRow = () => handle.props.direction === "row";

  const onPointerDown = (e: PointerEvent) => {
    if (!containerEl || !dividerEl) return;
    const rect = containerEl.getBoundingClientRect();
    const total = isRow() ? rect.width : rect.height;
    const { px, unit } = parsePx(livePosition, total);
    dragStart = {
      coord: isRow() ? e.clientX : e.clientY,
      px,
      total,
      unit,
    };
    dividerEl.classList.add("is-dragging");
    dividerEl.setPointerCapture(e.pointerId);
    e.preventDefault();
  };

  const onPointerMove = (e: PointerEvent) => {
    if (!dragStart) return;
    const cur = isRow() ? e.clientX : e.clientY;
    const delta = cur - dragStart.coord;
    const newPx = Math.max(0, Math.min(dragStart.total, dragStart.px + delta));
    const newValue = formatPx(newPx, dragStart.total, dragStart.unit);
    if (newValue === livePosition) return;
    livePosition = newValue;
    handle.update();
    handle.props.onResize?.(newValue);
  };

  const onPointerUp = () => {
    dragStart = null;
    if (dividerEl) dividerEl.classList.remove("is-dragging");
  };

  return () => {
    const { direction, children } = handle.props;
    const items = Array.isArray(children) ? children : [children];
    const orientation = direction === "row" ? "vertical" : "horizontal";
    return (
      <div
        class={`sola-split sola-split-${direction}`}
        mix={[ref((el: HTMLElement | null) => { containerEl = el; })]}
      >
        <div
          class="sola-split-pane sola-split-pane-first"
          style={`flex-basis: ${livePosition}`}
        >
          {items[0]}
        </div>
        <div
          class="sola-split-divider"
          role="separator"
          aria-orientation={orientation}
          mix={[
            ref((el: HTMLElement | null) => { dividerEl = el; }),
            on("pointerdown", onPointerDown),
            on("pointermove", onPointerMove),
            on("pointerup", onPointerUp),
            on("pointercancel", onPointerUp),
          ]}
        />
        <div class="sola-split-pane sola-split-pane-second">
          {items[1]}
        </div>
      </div>
    );
  };
}
