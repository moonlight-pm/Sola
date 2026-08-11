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
//! `WPE_IS_BUFFER(buffer)` criticals. We keep a short **retire ring**
//! (1 frame) so tokens outlive the submit that still samples them.
//! Inactive tabs hold at most one parked snapshot (restored on switch).

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use crate::shader::SamplePipeline;
use crate::{Cmd, FrameSlot};

use super::engine::{HeldToken, InputEvent, WpeEngine};
use super::input;
use super::wgpu_import::{self, DmabufMetadata, ImportedFrame};

/// Retire **owned** textures only (WPE buffers are released right after
/// blit+Wait). Depth 1 so the previous owned frame survives one iced frame.
const RETIRE_DEPTH: usize = 1;

/// Device scale for content — see [`super::paint_budget`].
///
/// Default is **honest compositor scale** (no forced 2×). Forced supersample
/// caused residual full-width black bands under scroll (tile paint outrun).
/// Opt-in: `SOLA_BROWSER_SUPER_SAMPLE=1` or `SOLA_BROWSER_DPR=2`.
fn choose_content_dpr(compositor_scale: f64, css_w: u32, css_h: u32) -> f64 {
    super::paint_budget::choose_dpr(compositor_scale, css_w, css_h)
}

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

/// Immediately free all GPU holds for `tab_id` (closed tab).
fn purge_tab(pipeline: &mut WpePipeline, tab_id: u64) {
    pipeline.parked.remove(&tab_id);
    pipeline.retire.retain(|s| s.tab_id != tab_id);
    if pipeline.active.as_ref().is_some_and(|a| a.tab_id == tab_id) {
        pipeline.active = None;
        pipeline.sample.clear();
        crate::wpe::paint_stats::global().note_sample_clear("purge_tab");
    }
}

/// Delay release of a replaced surface so the GPU can finish sampling.
fn retire(pipeline: &mut WpePipeline, surface: TabSurface) {
    pipeline.retire.push_back(surface);
    while pipeline.retire.len() > RETIRE_DEPTH {
        let _ = pipeline.retire.pop_front(); // Drop → HeldToken → buffer_released
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
        // Stock Wayland: pointer/scroll over the companion hit the WPE seat
        // natively; iced only injects when the event lands on this shader
        // (chrome chrome / dual-window click into the hole widget).
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
                        // Adaptive paint budget: drop supersample while flinging
                        // so WebKit tiles keep up (anti-checkerboard).
                        super::paint_budget::note_scroll();
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
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        // HiDPI: CSS = logical bounds; scale = compositor DPR so text is
        // crisp (1 CSS px → scale device px). Cap only at MAX_PHYS_EDGE.
        let compositor_scale = (viewport.scale_factor() as f64).max(1.0);
        let logical_w = bounds.width.round().max(1.0) as u32;
        let logical_h = bounds.height.round().max(1.0) as u32;
        let dpr = choose_content_dpr(compositor_scale, logical_w, logical_h);
        let phys_w = ((logical_w as f64) * dpr).round().max(1.0) as u32;
        let phys_h = ((logical_h as f64) * dpr).round().max(1.0) as u32;
        // Absorb 1px bound jitter so we never Resize-storm (cache destroy → flicker).
        let requested = super::paint_budget::stabilize_phys(phys_w, phys_h);

        // Content plane: position in **parent surface coords** (iced buffer
        // pixels = CSS × compositor_scale), not WebKit buffer pixels.
        // width/height are CSS layout size; buffer_scale = dpr handles 2× paint.
        if crate::content_plane::mode().is_plane() {
            if let Some(tx) = crate::content_plane::global_sender() {
                let parent_scale = compositor_scale;
                let x = (bounds.x as f64 * parent_scale).round() as i32;
                let y = (bounds.y as f64 * parent_scale).round() as i32;
                let surf_w = ((logical_w as f64) * parent_scale).round().max(1.0) as u32;
                let surf_h = ((logical_h as f64) * parent_scale).round().max(1.0) as u32;
                let _ = tx.send(crate::content_plane::ContentPlaneCmd::SetRect {
                    x,
                    y,
                    width: surf_w,
                    height: surf_h,
                    buffer_scale: dpr.round().clamp(1.0, 2.0) as i32,
                });
            }
        }
        // Stock Wayland: content is a sibling surface; publish global scissor
        // for river lockstep (layout CSS coords + chrome WindowGeometry).
        if crate::content_plane::mode().is_wayland() {
            crate::lockstep::note_content_local(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            );
        }
        {
            let mut last = self.slot.last_size.lock().unwrap();
            if *last != requested {
                *last = requested;
                tracing::info!(
                    logical_w,
                    logical_h,
                    dpr,
                    phys_w,
                    phys_h,
                    compositor_scale,
                    present = if crate::content_plane::mode().is_wayland() {
                        "wayland-stock"
                    } else if crate::content_plane::mode().is_plane() {
                        "plane"
                    } else {
                        "import"
                    },
                    "content resize/dpr"
                );
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

        // Tab switch: release previous hold. Only act when we actually leave a
        // different tab — `active == None` used to hit this every prepare and
        // call sample.clear() forever (permanent black until a new import).
        let active_id = pipeline.active.as_ref().map(|a| a.tab_id);
        if let Some(prev_id) = active_id {
            if prev_id != paint_tab {
                if let Some(prev) = pipeline.active.take() {
                    drop(prev);
                }
                if let Some(old) = pipeline.parked.remove(&paint_tab) {
                    show_surface(pipeline, device, &old);
                    pipeline.active = Some(old);
                } else {
                    // Keep last bind group until a new frame imports —
                    // sample.clear() flashes full black on tab switch /
                    // OpenUrl paint_tab churn (paint_tab_switch_no_park).
                }
                for (_, surf) in pipeline.parked.drain() {
                    drop(surf);
                }
            }
        } else if let Some(old) = pipeline.parked.remove(&paint_tab) {
            // First paint after empty: restore park if any.
            show_surface(pipeline, device, &old);
            pipeline.active = Some(old);
        }

        let Some(pending) = self.slot.pending.lock().unwrap().take() else {
            crate::wpe::paint_stats::PaintStats::inc(
                &crate::wpe::paint_stats::global().prepare_idle,
            );
            return;
        };

        // Allow the next NewFrame wakeup (in case Msg::NewFrame was coalesced
        // away or prepare ran from a different redraw path).
        self.slot
            .redraw_queued
            .store(false, std::sync::atomic::Ordering::Release);

        let tab_id = pending.tab_id.0;
        let mut frame = pending.frame;

        // Background tab frames: drop without import (release via WpeFrame Drop).
        // Worker now filters inactive tabs; this is a belt-and-suspenders race
        // guard for paint_tab vs worker active desync.
        if tab_id != paint_tab {
            crate::wpe::paint_stats::PaintStats::inc(
                &crate::wpe::paint_stats::global().drop_bg,
            );
            return;
        }

        crate::wpe::paint_stats::PaintStats::inc(
            &crate::wpe::paint_stats::global().prepare_new,
        );

        let release_tx = frame.release_tx.clone();
        let size = (frame.width, frame.height);
        let w = frame.width;
        let h = frame.height;

        // CPU-converted multi-plane YUV (token already released in worker).
        if let Some(rgba) = frame.take_rgba() {
            drop(frame);
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("wpe-yuv-bgra"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w.saturating_mul(4)),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            let surface = TabSurface {
                tab_id,
                imported: ImportedFrame::from_owned_texture(tex),
                _token: HeldToken::none(),
                size,
            };
            if let Some(old) = pipeline.active.take() {
                retire(pipeline, old);
            }
            show_surface(pipeline, device, &surface);
            pipeline.active = Some(surface);
            pipeline.sample.note_frame();
            return;
        }

        let Some(token) = frame.take_token() else {
            return;
        };
        let Some(fd) = frame.take_fd() else {
            let _ = HeldToken::new(token, release_tx);
            return;
        };
        // P0: wait for WebKit GPU write fence before importing (GTK/WPE
        // production path). Without this we blit incomplete frames → black
        // swaths / nav flicker on YouTube homepage scroll.
        match frame.take_render_fence() {
            Some(fence) => {
                if super::engine::wait_render_fence(fence, 50) {
                    crate::wpe::paint_stats::PaintStats::inc(
                        &crate::wpe::paint_stats::global().fence_ok,
                    );
                }
            }
            None => {
                crate::wpe::paint_stats::PaintStats::inc(
                    &crate::wpe::paint_stats::global().fence_none,
                );
            }
        }
        let mut planes = vec![wgpu_import::DmabufPlaneLayout {
            stride: frame.stride,
            offset: frame.offset,
        }];
        for (s, o) in frame.extra_planes.drain(..) {
            planes.push(wgpu_import::DmabufPlaneLayout {
                stride: s,
                offset: o,
            });
        }
        let meta = DmabufMetadata {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            planes,
        };
        drop(frame);

        let imported = match unsafe { wgpu_import::import(device, fd, &meta) } {
            Ok(f) => {
                crate::wpe::paint_stats::global().note_import_ok();
                f
            }
            Err(e) => {
                crate::wpe::paint_stats::PaintStats::inc(
                    &crate::wpe::paint_stats::global().import_err,
                );
                tracing::error!(tab = tab_id, "wgpu_import::import failed: {e}");
                let _ = HeldToken::new(token, release_tx);
                return;
            }
        };

        // Blit → owned, Wait for GPU, then release WPE immediately.
        // Holding active+retire dma-bufs exhausted WebKit's 2–3 buffer pool
        // (claim=0, ignore-only after scroll). Sampling import without Wait
        // after clear-to-black blit caused mid-scroll black flashes.
        let owned = pipeline
            .sample
            .blit_to_owned(device, queue, &imported.texture, size);
        // Blit finished (Wait): free import memory + return buffer to WPE now.
        drop(imported);
        drop(HeldToken::new(token, release_tx));

        let surface = TabSurface {
            tab_id,
            imported: ImportedFrame::from_owned_texture(owned),
            _token: HeldToken::none(),
            size,
        };

        if let Some(old) = pipeline.active.take() {
            retire(pipeline, old);
        }
        show_surface(pipeline, device, &surface);
        pipeline.active = Some(surface);
        pipeline.sample.note_frame();
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        // Transparent hole so content shows under chrome:
        // - **plane:** subsurface is a child of iced; Load leaves parent empty
        //   enough for the child (same surface tree).
        // - **wayland lockstep:** companion is a *sibling below* chrome.
        //   Load leaves opaque chrome bg in the hole → content only flashes
        //   when chrome moves first (user dogfood). Must Clear α=0 so the
        //   compositor composites the content window through the hole.
        if crate::content_plane::mode().is_plane()
            || crate::content_plane::mode().is_wayland()
        {
            let load = if crate::content_plane::mode().is_wayland() {
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                })
            } else {
                wgpu::LoadOp::Load
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wpe content hole"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_scissor_rect(
                clip_bounds.x,
                clip_bounds.y,
                clip_bounds.width.max(1),
                clip_bounds.height.max(1),
            );
            let _ = &mut pass;
            return;
        }
        let requested = *self.slot.last_size.lock().unwrap();
        pipeline
            .sample
            .render(encoder, target, clip_bounds, requested, "wpe sample pass");
    }
}

pub struct WpePipeline {
    sample: SamplePipeline,
    /// Painted tab's live frame (dma-buf import + WPE loan token).
    active: Option<TabSurface>,
    /// One last-good snapshot per inactive tab (restored on switch).
    parked: HashMap<u64, TabSurface>,
    /// Recently replaced surfaces; delayed drop so GPU finishes sampling
    /// before `buffer_released`.
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
