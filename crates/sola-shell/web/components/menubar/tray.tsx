// <Tray> — right-side tray: clock + toast notification.
//
// Clock: formatted as `HH:MM Weekday YYYY-MM-DD`, updated every 10 seconds.
// Toast: receives `{ event: "toast", message: string }` from Rust, visible
//        for 5 seconds then auto-hides.

import { type Handle } from "@remix-run/ui";
import { on as ipcOn } from "@sola/ipc";

const WEEKDAYS = [
  "Sunday", "Monday", "Tuesday", "Wednesday",
  "Thursday", "Friday", "Saturday",
];

function formatClock(now: Date): string {
  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  const weekday = WEEKDAYS[now.getDay()];
  const y = now.getFullYear();
  const mo = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${hh}:${mm} ${weekday} ${y}-${mo}-${d}`;
}

export function Tray(handle: Handle<{}>) {
  // Clock state.
  let clockText = formatClock(new Date());
  // No binding needed — the interval runs for the process lifetime.
  // A Remix v3 cleanup hook would be needed to cancel it on unmount,
  // but the menubar is a permanent window so process lifetime == component lifetime.
  setInterval(() => {
    clockText = formatClock(new Date());
    handle.update();
  }, 10_000);

  // Toast state.
  let toastMessage = "";
  let toastVisible = false;
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  // Subscribe to toast events pushed from Rust.
  ipcOn("toast", (msg: any) => {
    toastMessage = msg.message ?? "";
    toastVisible = true;
    handle.update();

    if (toastTimer !== null) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastVisible = false;
      toastTimer = null;
      handle.update();
    }, 5_000);
  });

  return () => {
    const toastCls = toastVisible
      ? "sola-menubar-toast sola-menubar-toast--visible"
      : "sola-menubar-toast";
    return (
      <div class="sola-menubar-tray">
        <div class="sola-menubar-clock">{clockText}</div>
        <div class={toastCls}>{toastMessage}</div>
      </div>
    );
  };
}
