// Sticky-state panel. Shows the latest message per (topic, source)
// pair, click-to-expand inline JSON. Owns the drag divider too —
// drag commits to `monitor_set_sidebar_width` on pointer-up so the
// width persists via Topic::MonitorConfig.

import { type Handle } from "@remix-run/ui";
import { invoke } from "@sola/ipc";
import { on } from "@sola/kit";

import { categoryOf } from "../lib/categories";
import { highlightedJson } from "../lib/json-tokens";
import type { BusMessage } from "./messages";

export interface StickyProps {
  state: StickyState;
}

export interface StickyState {
  stickyMessages: BusMessage[];
  expandedStickyKey: string | null;
  sidebarWidth: number;
}

const MIN_WIDTH = 120;
const MAX_WIDTH = 600;

export function StickyDivider(handle: Handle<StickyProps>) {
  let dragging = false;

  const onDown = (e: PointerEvent) => {
    dragging = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    (e.target as HTMLElement).classList.add("is-dragging");
    e.preventDefault();
  };

  const onMove = (e: PointerEvent) => {
    if (!dragging) return;
    const next = Math.max(
      MIN_WIDTH,
      Math.min(window.innerWidth - e.clientX, MAX_WIDTH),
    );
    if (next !== handle.props.state.sidebarWidth) {
      handle.props.state.sidebarWidth = next;
      handle.update();
    }
  };

  const onUp = (e: PointerEvent) => {
    if (!dragging) return;
    dragging = false;
    (e.target as HTMLElement).classList.remove("is-dragging");
    invoke("monitor_set_sidebar_width", { width: handle.props.state.sidebarWidth });
  };

  return () => (
    <div
      class="monitor-divider"
      mix={[
        on("pointerdown", onDown),
        on("pointermove", onMove),
        on("pointerup", onUp),
        on("pointercancel", onUp),
      ]}
    />
  );
}

export function StickyPanel(handle: Handle<StickyProps>) {
  return () => {
    const s = handle.props.state;
    return (
      <div class="monitor-sticky">
        <div class="monitor-sticky-header">Sticky State</div>
        <div class="monitor-sticky-list">
          {s.stickyMessages.map((msg) => {
            const key = `${msg.topic}:${msg.source}`;
            const expanded = s.expandedStickyKey === key;
            return (
              <div
                key={key}
                class="monitor-sticky-entry"
                data-category={categoryOf(msg.topic)}
              >
                <div
                  class={`monitor-sticky-item${expanded ? " expanded" : ""}`}
                  mix={[on("click", () => {
                    s.expandedStickyKey = expanded ? null : key;
                    handle.update();
                  })]}
                >
                  <span class="monitor-sticky-item-topic">{msg.topic}</span>
                  <span class="monitor-sticky-item-source">{msg.source || ""}</span>
                </div>
                {expanded && msg.payload != null
                  ? <div class="monitor-sticky-detail">{highlightedJson(msg.payload)}</div>
                  : ""}
              </div>
            );
          })}
        </div>
      </div>
    );
  };
}
