// Topic → category map used to color-stripe rows in the messages
// table. Categories are visual-only; unknown topics get the
// "unknown" stripe.

export const TOPIC_CATEGORIES: Record<string, string> = {
  Apps: "lifecycle",
  LaunchApp: "lifecycle",
  LaunchResult: "lifecycle",
  UserAppExited: "lifecycle",
  Shutdown: "lifecycle",
  Composition: "composition",
  Frame: "composition",
  Focus: "composition",
  SetWindowPolicy: "window",
  OutputGeometry: "window",
  MouseEntered: "input",
  ShellKeyBindings: "input",
  SetAppMenu: "menu",
  MenuAction: "menu",
  OpenUrl: "browser",
};

export function categoryOf(topic: string): string {
  return TOPIC_CATEGORIES[topic] || "unknown";
}
