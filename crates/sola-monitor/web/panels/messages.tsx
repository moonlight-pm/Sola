// Messages panel — toolbar (filter, topic dropdown, pause, clear,
// counter) + scrollable table of bus messages. Selection toggles
// the selected row's preview cell into a one-line highlighted JSON
// dump (per-design inline expansion).

import { type Handle, ref } from "@remix-run/ui";
import { Button } from "@sola/button";
import { PopoverSelect } from "@sola/popover-select";
import { Text } from "@sola/text";
import { TextInput } from "@sola/text-input";
import { on } from "@sola/kit";

import { categoryOf } from "../lib/categories";
import { highlightedJson, highlightedPreview } from "../lib/json-tokens";

export interface BusMessage {
  msgId: string;
  timestamp: number;
  topic: string;
  sticky: boolean;
  source: string;
  payload: unknown;
  rawHex: string | null;
}

export interface MessagesProps {
  // Caller-owned shared state. The panel mutates the visible-filter +
  // selection slices on this object and calls handle.update().
  state: MessagesState;
}

export interface MessagesState {
  messages: BusMessage[];
  filteredMessages: BusMessage[];
  selectedId: string | null;
  paused: boolean;
  pauseBufferLen: number;
  filter: string;
  topicFilter: string;
  count: number;
  autoScroll: boolean;
  knownTopics: string[];
}

function formatTime(ms: number): string {
  const d = new Date(ms);
  const h = String(d.getHours()).padStart(2, "0");
  const m = String(d.getMinutes()).padStart(2, "0");
  const s = String(d.getSeconds()).padStart(2, "0");
  const ms_ = String(d.getMilliseconds()).padStart(3, "0");
  return `${h}:${m}:${s}.${ms_}`;
}

export function MessagesPanel(handle: Handle<MessagesProps>) {
  let listEl: HTMLElement | null = null;

  const onScroll = () => {
    if (!listEl) return;
    const atBottom =
      listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - 4;
    if (handle.props.state.autoScroll !== atBottom) {
      handle.props.state.autoScroll = atBottom;
      handle.update();
    }
  };

  // Capture the list element after first render so we can wire scroll.
  const captureList = (el: HTMLElement | null) => {
    listEl = el;
  };

  return () => {
    const s = handle.props.state;

    return (
      <div class="monitor-messages">
        <div class="monitor-table-header">
          <span>Time</span>
          <span>Topic</span>
          <span>Source</span>
          <span>S</span>
          <span>Preview</span>
        </div>

        <div
          class="monitor-message-list"
          mix={[on("scroll", onScroll), ref(captureList)]}
        >
          {s.filteredMessages.map((msg) => {
            const selected = s.selectedId === msg.msgId;
            const previewCls = selected
              ? "monitor-cell preview expanded"
              : "monitor-cell preview";
            return (
              <div
                key={msg.msgId}
                class={`monitor-message-row${selected ? " selected" : ""}`}
                data-category={categoryOf(msg.topic)}
                mix={[on("click", () => {
                  s.selectedId = selected ? null : msg.msgId;
                  handle.update();
                })]}
              >
                <span class="monitor-cell time">{formatTime(msg.timestamp)}</span>
                <span class="monitor-cell topic">{msg.topic}</span>
                <span class="monitor-cell source">{msg.source || "—"}</span>
                <span class="monitor-cell sticky-dot">
                  {msg.sticky ? <span class="dot"/> : ""}
                </span>
                <span class={previewCls}>
                  {selected && msg.payload != null
                    ? highlightedJson(msg.payload)
                    : highlightedPreview(msg.payload)}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    );
  };
}

// Helper exposed for main.tsx — renders the toolbar above the panel
// so main.tsx can place it in its top-level Stack.
export interface ToolbarProps {
  state: MessagesState;
  onFilter: (v: string) => void;
  onTopic: (v: string) => void;
  onTogglePause: () => void;
  onClear: () => void;
}

export function MessagesToolbar(handle: Handle<ToolbarProps>) {
  return () => {
    const { state, onFilter, onTopic, onTogglePause, onClear } = handle.props;
    const topicOptions = [{ label: "All topics", value: "" }].concat(
      state.knownTopics.map((t) => ({ label: t, value: t })),
    );
    return (
      <div class="monitor-toolbar">
        <div style="display: flex; gap: var(--space-md); align-items: center">
          <div style="flex: 1; max-width: 320px">
            <TextInput
              value={state.filter}
              placeholder="Filter messages…"
              onInput={onFilter}
            />
          </div>
          <PopoverSelect
            value={state.topicFilter}
            options={topicOptions}
            onChange={onTopic}
          />
          <Button
            variant={state.paused ? "primary" : "ghost"}
            onPress={onTogglePause}
          >
            {state.paused ? `Resume (${state.pauseBufferLen})` : "Pause"}
          </Button>
          <Button variant="ghost" onPress={onClear}>Clear</Button>
          <div style="flex: 1"/>
          <Text tone="muted" kind="caption">{state.count} msgs</Text>
        </div>
      </div>
    );
  };
}
