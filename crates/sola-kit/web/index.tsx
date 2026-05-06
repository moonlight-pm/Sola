import { invoke, on } from "@sola/ipc";

// The Rust side renders the merged theme as a `:root { ... }` CSS block
// and pushes it whenever Topic::Theme changes (incl. the sticky replay
// on first connect). We adopt a constructable stylesheet once and
// replaceSync the rules on every delivery — no DOM mutation.
const themeSheet = new CSSStyleSheet();
document.adoptedStyleSheets = [...document.adoptedStyleSheets, themeSheet];
on("theme", (msg: { css?: string }) => {
  if (msg.css) themeSheet.replaceSync(msg.css);
});

// Smoke test: prove (a) HTML is rendering as HTML (not text), and (b) the
// IPC pipeline works in both directions — invoke() → cefQuery → Rust →
// __solaRecv → resolved Promise. Replaced when the Remix UI spike lands.
const main = document.createElement("main");
main.style.font = "14px/1.5 system-ui";
main.style.padding = "2rem";
const h1 = document.createElement("h1");
h1.textContent = "sola-kit";
const p1 = document.createElement("p");
p1.textContent = "HTML rendered as HTML ✓";
const ipcEl = document.createElement("p");
ipcEl.textContent = "ipc: pinging…";
main.append(h1, p1, ipcEl);
document.body.append(main);

invoke("ping", { from: "index.tsx" })
  .then((res) => {
    ipcEl.textContent = `ipc: ${JSON.stringify(res)} ✓`;
  })
  .catch((err) => {
    ipcEl.textContent = `ipc error: ${err}`;
  });
