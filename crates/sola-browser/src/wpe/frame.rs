//! iced `shader::Program` that samples the currently-imported WPE
//! frame as a fullscreen quad.
//!
//! Shared pipeline / WGSL live in `crate::shader`. This module owns
//! input translation and dma-buf import.
//!
//! # Frame lifetime (critical)
//!
//! WPE dma-bufs are released via `wpe_view_buffer_released` when
//! [`HeldToken`] drops. Releasing the buffer the GPU is still sampling
//! causes rapid flicker (animated sites) and
//! `WPE_IS_BUFFER(buffer)` criticals. We keep a short **retire ring** of
//! recently replaced surfaces so tokens outlive 1–2 submitted frames.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use crate::shader::SamplePipeline;
use crate::{Cmd, FrameSlot};

use super::engine::{HeldToken, InputEvent, WpeEngine};
use super::input;
use super::wgpu_import::{self, DmabufMetadata, ImportedFrame};

/// How many replaced frames to keep alive after uninstall (GPU lag).
/// Keep short — deep retire held WPE dma-bufs past pool reuse and caused
/// "buffer ptr already live" + invalid `buffer_released` under media.
const RETIRE_DEPTH: usize = 1;

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

#[derive(Debug)]
pub struct ProgramState {
    last_bounds: Rectangle,
    last_scale: f32,
    last_pointer: Option<(f64, f64)>,
    held_button_mods: u32,
    modifiers: keyboard::Modifiers,
    started: Option<Instant>,
}

impl Default for ProgramState {
    fn default() -> Self {
        Self {
            last_bounds: Rectangle::default(),
            last_scale: 1.0,
            last_pointer: None,
            held_button_mods: 0,
            modifiers: keyboard::Modifiers::default(),
            started: None,
        }
    }
}

impl ProgramState {
    fn now_ms(&mut self) -> u32 {
        let started = *self.started.get_or_insert_with(Instant::now);
        started.elapsed().as_millis() as u32
    }
}

/// Last GPU frame for one tab. Parked when the tab is inactive so
/// switching back restores pixels without a black flash. Holds the
/// WPE recycle token until the surface is dropped.
struct TabSurface {
    tab_id: u64,
    imported: ImportedFrame,
    _token: HeldToken,
    size: (u32, u32),
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wpe-shader bg"),
        layout,
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
    })
}

fn show_surface(pipeline: &mut WpePipeline, device: &wgpu::Device, surface: &TabSurface) {
    let bg = make_bind_group(
        device,
        &pipeline.sample.bind_group_layout,
        &pipeline.sample.sampler,
        &surface.imported.texture,
    );
    pipeline.sample.install_bind_group(bg, surface.size);
}

/// Drop `surf` only after `RETIRE_DEPTH` newer surfaces have replaced it,
/// so in-flight GPU work can finish sampling the dma-buf.
fn retire(pipeline: &mut WpePipeline, surf: TabSurface) {
    pipeline.retire.push_back(surf);
    while pipeline.retire.len() > RETIRE_DEPTH {
        let _ = pipeline.retire.pop_front();
    }
}

/// Immediately free all GPU holds for `tab_id` (closed tab).
fn purge_tab(pipeline: &mut WpePipeline, tab_id: u64) {
    pipeline.parked.remove(&tab_id);
    pipeline.retire.retain(|s| s.tab_id != tab_id);
    if pipeline.active.as_ref().is_some_and(|a| a.tab_id == tab_id) {
        pipeline.active = None;
        pipeline.sample.clear();
    }
}

impl shader::Program<crate::app::Msg> for WpeProgram {
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
    ) -> Option<iced::widget::shader::Action<crate::app::Msg>> {
        state.last_bounds = bounds;
        // Input is in CSS/layout pixels (WPE size is logical + device scale).
        state.last_scale = 1.0;
        let time_ms = state.now_ms();
        let mods_now = state.modifiers;

        match event {
            iced::Event::Mouse(m) => {
                let cur = cursor.position_in(bounds)?;
                let (x, y) = crate::input::project_cursor_f64(
                    iced::Point::new(bounds.x + cur.x, bounds.y + cur.y),
                    bounds,
                    1.0,
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
                    mouse::Event::ButtonPressed(b) => input::button_to_wpe(*b).map(|button| {
                        state.held_button_mods |= input::button_to_modifier(button);
                        InputEvent::PointerButton {
                            down: true,
                            x,
                            y,
                            button,
                            modifiers: kbd_mods | state.held_button_mods,
                            time_ms,
                        }
                    }),
                    mouse::Event::ButtonReleased(b) => input::button_to_wpe(*b).map(|button| {
                        state.held_button_mods &= !input::button_to_modifier(button);
                        InputEvent::PointerButton {
                            down: false,
                            x,
                            y,
                            button,
                            modifiers: kbd_mods | state.held_button_mods,
                            time_ms,
                        }
                    }),
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
                let is_left_press = matches!(m, mouse::Event::ButtonPressed(mouse::Button::Left));
                if let Some(e) = ev {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    if is_left_press {
                        return Some(
                            iced::widget::shader::Action::publish(crate::app::Msg::WebViewFocused)
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
        crate::CursorKind::from_u32(raw).to_iced()
    }
}

impl shader::Primitive for WpePrimitive {
    type Pipeline = WpePipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        // HiDPI model (matches WebKitGTK / Chromium OSR):
        //   - WPE resize = **CSS/layout** size (logical widget bounds)
        //   - scale_changed = device pixel ratio
        //   - dma-buf = CSS × scale (physical)
        // Passing physical size *and* scale was double-scaling and soft text.
        //
        // When iced reports scale≈1 (common on this stack) we supersample at
        // 2× so WebKit rasterizes twice the density and we downscale in the
        // shader — sharper than 1× glyphs stretched by the compositor.
        let compositor_scale = (viewport.scale_factor() as f64).max(1.0);
        let dpr = if compositor_scale < 1.25 {
            2.0
        } else {
            compositor_scale
        };
        let logical_w = bounds.width.round().max(1.0) as u32;
        let logical_h = bounds.height.round().max(1.0) as u32;
        let phys_w = ((logical_w as f64) * dpr).round().max(1.0) as u32;
        let phys_h = ((logical_h as f64) * dpr).round().max(1.0) as u32;
        // last_size tracks **physical** buffer size for mismatch checks.
        let requested = (phys_w, phys_h);
        {
            let mut last = self.slot.last_size.lock().unwrap();
            if *last != requested {
                *last = requested;
                let _ = self.slot.cmd_tx.send(Cmd::Resize {
                    width: logical_w,
                    height: logical_h,
                    scale: dpr,
                });
            }
        }

        // Drop GPU caches for closed tabs (frees dma-buf tokens).
        {
            let mut drop_list = self.slot.drop_paint_tabs.lock().unwrap();
            for id in drop_list.drain(..) {
                purge_tab(pipeline, id);
            }
        }

        let paint_tab = self
            .slot
            .paint_tab
            .load(std::sync::atomic::Ordering::Relaxed);

        // Keep GPU active surface in sync with chrome `paint_tab`.
        //
        // After closing the active tab, `active` is None and the previous
        // tab is usually parked — restore it. Also park-on-switch so the
        // leave tab keeps a last-good frame.
        let active_id = pipeline.active.as_ref().map(|a| a.tab_id);
        if active_id != Some(paint_tab) {
            if let Some(prev) = pipeline.active.take() {
                // Replace any older park for this tab (retire the previous
                // park so we do not hold unbounded buffers).
                if let Some(old_park) = pipeline.parked.insert(prev.tab_id, prev) {
                    retire(pipeline, old_park);
                }
            }
            if let Some(surf) = pipeline.parked.remove(&paint_tab) {
                show_surface(pipeline, device, &surf);
                pipeline.active = Some(surf);
            }
            // No park: dark fallback until the worker emits a frame
            // (SetActiveTab uses a 1px resize nudge for static pages).
        }

        // NOTE: do **not** re-send Resize every frame when buffer size ≠
        // scissor. That 1px-nudge heal loop blanked the view (~1 Hz steady,
        // burst on focus). Worker may still heal once with a long cooldown.

        let Some(pending) = self.slot.pending.lock().unwrap().take() else {
            return;
        };

        let tab_id = pending.tab_id.0;
        let mut frame = pending.frame;

        let release_tx = frame.release_tx.clone();
        let Some(token) = frame.take_token() else {
            return;
        };
        let Some(fd) = frame.take_fd() else {
            // Still own the token — recycle even without an fd.
            let _ = HeldToken::new(token, release_tx);
            return;
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
        drop(frame);

        let imported = match unsafe { wgpu_import::import(device, fd, &meta) } {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(tab = tab_id, "wgpu_import::import failed: {e}");
                let _ = HeldToken::new(token, release_tx);
                return;
            }
        };

        let surface = TabSurface {
            tab_id,
            imported,
            _token: HeldToken::new(token, release_tx),
            size,
        };

        if tab_id == paint_tab {
            if let Some(old) = pipeline.active.take() {
                if old.tab_id == tab_id {
                    // Same tab, new frame: delay release (GPU may still sample).
                    retire(pipeline, old);
                } else {
                    if let Some(old_park) = pipeline.parked.insert(old.tab_id, old) {
                        retire(pipeline, old_park);
                    }
                }
            }
            show_surface(pipeline, device, &surface);
            pipeline.active = Some(surface);
            pipeline.sample.note_frame();
        } else {
            // Background prime / late frame: keep last-good park only.
            if let Some(old_park) = pipeline.parked.insert(tab_id, surface) {
                retire(pipeline, old_park);
            }
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

pub struct WpePipeline {
    sample: SamplePipeline,
    active: Option<TabSurface>,
    /// Last frame per inactive tab — restored on switch (no black flash).
    parked: HashMap<u64, TabSurface>,
    /// Recently replaced surfaces kept alive so dma-buf release lags the GPU.
    retire: VecDeque<TabSurface>,
}

impl std::fmt::Debug for WpePipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WpePipeline")
            .field("active_tab", &self.active.as_ref().map(|a| a.tab_id))
            .field("parked", &self.parked.len())
            .field("retire", &self.retire.len())
            .finish_non_exhaustive()
    }
}

impl shader::Pipeline for WpePipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            sample: SamplePipeline::new(device, queue, format, "wpe"),
            active: None,
            parked: HashMap::new(),
            retire: VecDeque::with_capacity(RETIRE_DEPTH + 1),
        }
    }
}
