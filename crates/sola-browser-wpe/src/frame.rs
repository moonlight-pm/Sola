//! iced `shader::Program` that samples the currently-imported WPE
//! frame as a fullscreen quad.
//!
//! Shared pipeline / WGSL live in `sola_browser_core::shader`. This
//! module owns input translation and dma-buf import via `FrameImport`.

use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use sola_browser_core::shader::{FrameImport, ImportedTexture, SamplePipeline};
use sola_browser_core::{Cmd, FrameSlot};

use crate::engine::{HeldToken, InputEvent, WpeEngine, WpeFrame};
use crate::input;
use crate::wgpu_import::{self, DmabufMetadata, ImportedFrame};

pub struct WpeProgram {
    pub slot: std::sync::Arc<FrameSlot<WpeEngine>>,
}

impl std::fmt::Debug for WpeProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WpeProgram").finish_non_exhaustive()
    }
}

pub struct WpePrimitive {
    pub slot: std::sync::Arc<FrameSlot<WpeEngine>>,
}

impl std::fmt::Debug for WpePrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WpePrimitive").finish_non_exhaustive()
    }
}

/// Per-program state: modifiers, held buttons, pointer deltas, timestamps.
#[derive(Debug)]
pub struct ProgramState {
    modifiers: keyboard::Modifiers,
    started: Option<Instant>,
    last_bounds: Rectangle,
    last_scale: f32,
    held_button_mods: u32,
    last_pointer: Option<(f64, f64)>,
}

impl Default for ProgramState {
    fn default() -> Self {
        Self {
            modifiers: keyboard::Modifiers::default(),
            started: None,
            last_bounds: Rectangle {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            last_scale: 1.0,
            held_button_mods: 0,
            last_pointer: None,
        }
    }
}

impl ProgramState {
    fn now_ms(&mut self) -> u32 {
        let started = *self.started.get_or_insert_with(Instant::now);
        started.elapsed().as_millis() as u32
    }
}

impl shader::Program<sola_browser_core::app::Msg> for WpeProgram {
    type State = ProgramState;
    type Primitive = WpePrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        WpePrimitive {
            slot: self.slot.clone(),
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::shader::Action<sola_browser_core::app::Msg>> {
        state.last_bounds = bounds;
        let (req_w, _req_h) = *self.slot.last_size.lock().unwrap();
        let scale = sola_browser_core::input::scale_from_last_size(bounds, req_w, state.last_scale);
        state.last_scale = scale;
        let time_ms = state.now_ms();
        let mods_now = state.modifiers;

        match event {
            iced::Event::Mouse(m) => {
                let cur = cursor.position_in(bounds)?;
                let (x, y) = sola_browser_core::input::project_cursor_f64(
                    iced::Point::new(bounds.x + cur.x, bounds.y + cur.y),
                    bounds,
                    scale,
                );
                let kbd_mods = input::modifiers_to_wpe(mods_now);
                let ev = match m {
                    mouse::Event::CursorMoved { .. } => {
                        let (dx, dy) = match state.last_pointer {
                            Some((px, py)) => (x - px, y - py),
                            None => (0.0, 0.0),
                        };
                        state.last_pointer = Some((x, y));
                        Some(InputEvent::PointerMove {
                            x,
                            y,
                            delta_x: dx,
                            delta_y: dy,
                            modifiers: kbd_mods | state.held_button_mods,
                            time_ms,
                        })
                    }
                    mouse::Event::ButtonPressed(b) => {
                        input::button_to_wpe(*b).map(|button| {
                            state.held_button_mods |= input::button_to_modifier(button);
                            InputEvent::PointerButton {
                                down: true,
                                x,
                                y,
                                button,
                                modifiers: kbd_mods | state.held_button_mods,
                                time_ms,
                            }
                        })
                    }
                    mouse::Event::ButtonReleased(b) => {
                        input::button_to_wpe(*b).map(|button| {
                            state.held_button_mods &= !input::button_to_modifier(button);
                            InputEvent::PointerButton {
                                down: false,
                                x,
                                y,
                                button,
                                modifiers: kbd_mods | state.held_button_mods,
                                time_ms,
                            }
                        })
                    }
                    mouse::Event::WheelScrolled { delta } => {
                        let (delta_x, delta_y, precise) = input::scroll_delta_to_wpe(*delta);
                        Some(InputEvent::Scroll {
                            x,
                            y,
                            delta_x,
                            delta_y,
                            precise,
                            modifiers: kbd_mods | state.held_button_mods,
                            time_ms,
                        })
                    }
                    mouse::Event::CursorLeft => {
                        state.last_pointer = None;
                        None
                    }
                    _ => None,
                };
                let is_left_press =
                    matches!(m, mouse::Event::ButtonPressed(mouse::Button::Left));
                if let Some(e) = ev {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    if is_left_press {
                        return Some(
                            iced::widget::shader::Action::publish(
                                sola_browser_core::app::Msg::WebViewFocused,
                            )
                            .and_capture(),
                        );
                    }
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::Keyboard(k) => {
                if let keyboard::Event::ModifiersChanged(m) = k {
                    state.modifiers = *m;
                }
                if let Some(e) = input::translate_keyboard(k, time_ms) {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::Window(w) => {
                use iced::window::Event as WE;
                match w {
                    WE::Focused => {
                        let _ = self.slot.cmd_tx.send(Cmd::Focus(true));
                    }
                    WE::Unfocused => {
                        let _ = self.slot.cmd_tx.send(Cmd::Focus(false));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let raw = self
            .slot
            .cursor
            .load(std::sync::atomic::Ordering::Relaxed);
        sola_browser_core::CursorKind::from_u32(raw).to_iced()
    }
}

/// Holds GPU-imported dma-buf + recycle token until the next frame or drop.
struct CurrentHold {
    _imported: ImportedFrame,
    _token: HeldToken,
}

struct WpeImporter;

impl FrameImport for WpeImporter {
    type Frame = WpeFrame;
    type Hold = CurrentHold;

    fn import(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        mut frame: WpeFrame,
    ) -> Option<(ImportedTexture, Self::Hold)> {
        let release_tx = frame.release_tx.clone();
        let token = frame.take_token()?;
        let Some(fd) = frame.take_fd() else {
            let _ = HeldToken::new(token, release_tx);
            return None;
        };
        let meta = DmabufMetadata {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            stride: frame.stride,
            offset: frame.offset,
        };
        let size = (frame.width, frame.height);
        let imported = match unsafe { wgpu_import::import(device, fd, &meta) } {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("wgpu_import::import failed: {e}");
                // Import failure must not pin the pool forever.
                let _ = HeldToken::new(token, release_tx);
                return None;
            }
        };
        // Frame has no token/fd left; Drop is a no-op.
        drop(frame);

        let view = imported
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wpe-shader bg"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Some((
            ImportedTexture { bind_group, size },
            CurrentHold {
                _imported: imported,
                _token: HeldToken::new(token, release_tx),
            },
        ))
    }
}

// Need release_tx accessible — it's private on WpeFrame. Expose via method.
// Patch: add release_tx() on WpeFrame or make field pub(crate).

impl shader::Primitive for WpePrimitive {
    type Pipeline = WpePipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        let scale = viewport.scale_factor() as f32;
        let req_w = (bounds.width * scale).round().max(1.0) as u32;
        let req_h = (bounds.height * scale).round().max(1.0) as u32;
        let mut last = self.slot.last_size.lock().unwrap();
        if *last != (req_w, req_h) {
            *last = (req_w, req_h);
            drop(last);
            let _ = self.slot.cmd_tx.send(Cmd::Resize {
                width: req_w,
                height: req_h,
            });
        }

        let mut guard = self.slot.pending.lock().unwrap();
        let Some(frame) = guard.take() else {
            return;
        };
        drop(guard);

        tracing::trace!(
            w = frame.width,
            h = frame.height,
            stride = frame.stride,
            "shader::prepare: importing new WPE frame",
        );

        if let Some((imported, hold)) = WpeImporter::import(
            device,
            queue,
            &pipeline.sample.bind_group_layout,
            &pipeline.sample.sampler,
            frame,
        ) {
            // Previous hold Drop releases its WPE token (pool depth ≥2
            // masks GPU still sampling previous dma-buf pages).
            pipeline.hold = Some(hold);
            pipeline.sample.install(imported);
            pipeline.sample.note_frame();
        }
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let requested = *self.slot.last_size.lock().unwrap();
        pipeline
            .sample
            .render(encoder, target, clip_bounds, requested, "wpe sample pass");
    }
}

#[derive(Debug)]
pub struct WpePipeline {
    sample: SamplePipeline,
    hold: Option<CurrentHold>,
}

impl shader::Pipeline for WpePipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            sample: SamplePipeline::new(device, format, "wpe"),
            hold: None,
        }
    }
}

// Debug for CurrentHold
impl std::fmt::Debug for CurrentHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurrentHold").finish_non_exhaustive()
    }
}

