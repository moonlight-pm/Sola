// Built-in kit entry point. Served at `app:///index.tsx` for any app
// that doesn't ship its own. Imports the app's root component through
// the bare specifier `@sola/app-root` — the kit's importmap injection
// (in `crates/sola-kit/src/lib.rs::build_importmap`) maps that to
// whatever URL the app's `SolaApp::ROOT_COMPONENT` constant declared
// (defaults to `/main.tsx`).
//
// The app side just exports a Remix v3 `Main` component factory from
// that file; everything else (theme stylesheet, future kit-wide
// listeners) is handled by `setupKit()`.

import { createRoot } from "@remix-run/ui";
import { setupKit } from "@sola/kit";
import { Main } from "@sola/app-root";

declare global {
  interface Window {
    __solaInitial: unknown;
  }
}

setupKit();
const initial = window.__solaInitial ?? null;
createRoot(document.body).render(<Main initial={initial} />);
