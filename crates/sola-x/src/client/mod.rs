// Client-side Wayland connection to sola.
//
// sola-x connects to sola as a regular Wayland client, creating
// proxy surfaces for each X11 window. This module handles the
// connection lifecycle and reconnection on compositor restart.
//
// Phase 2: client connection, registry, surface management, input.
