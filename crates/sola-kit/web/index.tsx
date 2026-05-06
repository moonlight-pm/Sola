import { on } from "@sola/ipc";

// The Rust side renders the merged theme as a `:root { ... }` CSS block
// and pushes it whenever Topic::Theme changes (incl. the sticky replay
// on first connect). We adopt a constructable stylesheet once and
// replaceSync the rules on every delivery — no DOM mutation.
const themeSheet = new CSSStyleSheet();
document.adoptedStyleSheets = [...document.adoptedStyleSheets, themeSheet];
on("theme", (msg: { css?: string }) => {
  if (msg.css) themeSheet.replaceSync(msg.css);
});
