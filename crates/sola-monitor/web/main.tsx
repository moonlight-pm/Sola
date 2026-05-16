// Monitor root. Owns the shared state, the bus_message ingest,
// the filtered/sticky derivations, and the toolbar action wiring.
// Composes the toolbar + messages panel + divider + sticky panel.

import { type Handle } from "@remix-run/ui";
import { Root } from "@sola/root";
import { Button } from "@sola/button";
import { on as ipcOn } from "@sola/ipc";

import {
  MessagesPanel,
  MessagesToolbar,
  type BusMessage,
  type MessagesState,
} from "./panels/messages";
import { StickyDivider, StickyPanel, type StickyState } from "./panels/sticky";

const MAX_MESSAGES = 5000;

type MonitorState = MessagesState & StickyState;

interface MainProps {}

export function Main(handle: Handle<MainProps>) {
  const state: MonitorState = {
    // MessagesState
    messages: [],
    filteredMessages: [],
    selectedId: null,
    paused: false,
    pauseBufferLen: 0,
    filter: "",
    topicFilter: "",
    count: 0,
    autoScroll: true,
    knownTopics: [],
    // StickyState
    stickyMessages: [],
    expandedStickyKey: null,
    sidebarWidth: 240,
  };

  let pauseBuffer: BusMessage[] = [];
  const seenTopics = new Set<string>();
  const stickyMap = new Map<string, BusMessage>();

  const applyFilter = () => {
    const filterLower = state.filter.toLowerCase();
    const topicFilter = state.topicFilter;
    state.filteredMessages = state.messages.filter((msg) => {
      if (topicFilter && msg.topic !== topicFilter) return false;
      if (filterLower) {
        const topicMatch = msg.topic.toLowerCase().includes(filterLower);
        const sourceMatch = msg.source.toLowerCase().includes(filterLower);
        const payloadMatch = msg.payload
          ? JSON.stringify(msg.payload).toLowerCase().includes(filterLower)
          : false;
        if (!topicMatch && !sourceMatch && !payloadMatch) return false;
      }
      return true;
    });
  };

  const refreshKnownTopics = () => {
    state.knownTopics = Array.from(seenTopics).sort();
  };

  const addMessage = (msg: BusMessage) => {
    if (state.paused) {
      pauseBuffer.push(msg);
      state.pauseBufferLen = pauseBuffer.length;
      handle.update();
      return;
    }
    state.messages.push(msg);
    if (state.messages.length > MAX_MESSAGES) {
      state.messages.splice(0, state.messages.length - MAX_MESSAGES);
    }
    state.count = state.messages.length;

    if (msg.sticky) {
      const key = `${msg.topic}:${msg.source}`;
      stickyMap.set(key, msg);
      state.stickyMessages = Array.from(stickyMap.values());
    }

    if (!seenTopics.has(msg.topic)) {
      seenTopics.add(msg.topic);
      refreshKnownTopics();
    }

    applyFilter();
    handle.update();
    if (state.autoScroll) {
      requestAnimationFrame(() => {
        const list = document.querySelector(".monitor-message-list");
        if (list) list.scrollTop = list.scrollHeight;
      });
    }
  };

  // --- IPC wiring ---

  ipcOn("bus_message", (msg: BusMessage) => addMessage(msg));

  ipcOn("state", (msg: { sidebar_width: number }) => {
    state.sidebarWidth = msg.sidebar_width;
    handle.update();
  });

  // --- Toolbar handlers ---

  const onFilter = (v: string) => {
    state.filter = v;
    applyFilter();
    handle.update();
  };

  const onTopic = (v: string) => {
    state.topicFilter = v;
    applyFilter();
    handle.update();
  };

  const togglePause = () => {
    state.paused = !state.paused;
    if (!state.paused) {
      for (const msg of pauseBuffer) addMessage(msg);
      pauseBuffer = [];
      state.pauseBufferLen = 0;
    }
    handle.update();
  };

  const clearMessages = () => {
    state.messages = [];
    state.filteredMessages = [];
    state.selectedId = null;
    state.count = 0;
    pauseBuffer = [];
    state.pauseBufferLen = 0;
    handle.update();
  };

  const jumpToBottom = () => {
    state.autoScroll = true;
    handle.update();
    requestAnimationFrame(() => {
      const list = document.querySelector(".monitor-message-list");
      if (list) list.scrollTop = list.scrollHeight;
    });
  };

  return () => (
    <Root>
      <div style="display: flex; flex-direction: column; height: 100vh; position: relative">
        <MessagesToolbar
          state={state}
          onFilter={onFilter}
          onTopic={onTopic}
          onTogglePause={togglePause}
          onClear={clearMessages}
        />
        <div
          class="monitor-main"
          style={`--monitor-sidebar-width: ${state.sidebarWidth}px`}
        >
          <MessagesPanel state={state}/>
          <StickyDivider state={state}/>
          <StickyPanel state={state}/>
        </div>
        {!state.autoScroll
          ? (
            <div class="monitor-autoscroll-pill">
              <Button variant="ghost" onPress={jumpToBottom}>
                ↓ Auto-scroll paused — click to resume
              </Button>
            </div>
          )
          : ""}
      </div>
    </Root>
  );
}
