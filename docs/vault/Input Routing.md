# Input Routing

The compositor ([[sola-compositor]]) owns all input via libinput and the Wayland seat. One rule governs dispatch:

## The Super Key Rule

- **Super held** → key event goes to the [[Sola Bus]] as `Topic::Key(KeyEvent)`. Never forwarded to any Wayland client.
- **No Super** → key event goes to the focused Wayland client via normal Wayland protocol.

All global shortcuts use Super as a modifier. The compositor doesn't know what any shortcut means — it just routes based on whether Super is held. Shell apps listen on the bus and decide what to do.

## Input Grab

Shell apps that need exclusive input use a grab/release pattern via [[Topics]]:

1. App emits `Topic::GrabInput("my-app-id")` on the bus
2. Compositor finds the surface by `app_id`, raises it, gives it keyboard focus
3. While grabbed, all input goes to that surface exclusively
4. App emits `Topic::ReleaseInput` when done
5. Compositor hides the surface, restores normal focus

## Key Chord: Shutdown

`Super+Shift+Backspace` (on release) triggers desktop shutdown. [[sola]] listens for this pattern in `Topic::Key` events on the bus and kills all processes.

## Who Handles What

| Key combo | Handler | Mechanism |
|---|---|---|
| Super+Tab | [[sola-switcher]] | Listens on bus, grabs input |
| Super+Space | Launcher (future) | Listens on bus, grabs input |
| Super+Shift+Backspace | [[sola]] | Listens on bus, shuts down |
| Everything else with Super | Shell apps | Listens on bus |
| Keys without Super | Focused Wayland client | Normal Wayland protocol |
