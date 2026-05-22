//! iced `shader::Program` that samples the currently-imported WPE
//! frame as a fullscreen quad.
//!
//! Ownership flow per frame:
//! 1. WPE worker thread emits a `WpeFrame` (FD + metadata + token).
//! 2. App's subscription receives it, stashes in `slot.pending`,
//!    requests an iced redraw.
//! 3. Next render cycle: `Primitive::prepare` runs on iced's render
//!    thread. It takes the pending frame, imports as a wgpu texture
//!    (via `wgpu_import::import`), swaps in the bind group, and
//!    sends a `Cmd::Release` back to the WPE worker for the
//!    previously-displayed frame's token so WPE can recycle that
//!    buffer.
//! 4. `Primitive::render` issues the fullscreen-triangle draw call.

use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use crate::input;
use crate::wgpu_import::{self, DmabufMetadata, ImportedFrame};
use crate::wpe::{Cmd, InputEvent, ResourceToken, WpeFrame};

/// Shared between the App (which fills `pending`) and the shader
/// Pipeline (which drains it on next prepare). The `releaser`
/// channel goes back to the WPE worker thread so we can hand
/// recycled buffer-resource tokens back when a new frame replaces
/// an old one — and so the shader Program can request resizes when
/// the iced widget bounds change.
pub struct FrameSlot {
    pub pending: Mutex<Option<WpeFrame>>,
    pub releaser: Sender<Cmd>,
    /// Last size we asked WPE to render at (physical pixels). Used
    /// to debounce resize commands so we only fire on actual change.
    pub last_size: Mutex<(u32, u32)>,
    /// Latest CSS-cursor state from WPE, written by the worker
    /// thread on `wpe_view_set_cursor_from_name`. Read by
    /// `Program::mouse_interaction`. Value is a `CursorKind`
    /// discriminant.
    pub cursor: Arc<std::sync::atomic::AtomicU32>,
}

#[derive(Debug)]
pub struct WpeProgram {
    pub slot: Arc<FrameSlot>,
}

impl std::fmt::Debug for FrameSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameSlot").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct WpePrimitive {
    pub slot: Arc<FrameSlot>,
}

/// Per-program state iced manages for us. Holds the running
/// modifier set, session start time (for monotonic 32-bit ms
/// timestamps WPE wants), and a bitmask of pointer buttons
/// currently held down so we can OR the matching
/// `WPE_MODIFIER_POINTER_BUTTON*` bits into PointerMove events
/// — that's how WebKit knows a move is a drag rather than a
/// hover.
#[derive(Debug)]
pub struct ProgramState {
    modifiers: keyboard::Modifiers,
    started: Option<Instant>,
    last_bounds: Rectangle,
    last_scale: f32,
    /// OR of `WPE_MODIFIER_POINTER_BUTTON{1..5}` for every
    /// pointer button currently held. Updated on
    /// ButtonPressed / ButtonReleased.
    held_button_mods: u32,
    /// Last cursor position in WPE view pixels — used to
    /// compute delta_x/delta_y on PointerMove (WebKit reads
    /// these for relative-motion features like pointer lock,
    /// and they're cheap to maintain).
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

impl<Msg> shader::Program<Msg> for WpeProgram {
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
    ) -> Option<iced::widget::shader::Action<Msg>> {
        state.last_bounds = bounds;
        // `update()` doesn't get the viewport. Derive scale from the
        // size `prepare()` last asked WPE for vs the widget's logical
        // width — both are present and consistent because we send
        // `Cmd::Resize { width: bounds.width * scale, height: ... }`
        // from `prepare`. On a freshly-mounted shader (before the
        // first prepare ran) we fall back to last-known scale.
        let (req_w, _req_h) = *self.slot.last_size.lock().unwrap();
        let scale = if bounds.width > 0.0 {
            (req_w as f32 / bounds.width).max(0.5)
        } else {
            state.last_scale
        };
        state.last_scale = scale;
        let time_ms = state.now_ms();
        let mods_now = state.modifiers;

        match event {
            iced::Event::Mouse(m) => {
                let cur = cursor.position_in(bounds)?;
                let (x, y) = input::project_cursor(
                    iced::Point::new(bounds.x + cur.x, bounds.y + cur.y),
                    bounds,
                    scale,
                );
                let kbd_mods = input::modifiers_to_wpe(mods_now);
                let ev = match m {
                    mouse::Event::CursorMoved { .. } => {
                        // delta vs previous position so drag-detect /
                        // pointer-lock-style features see motion.
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
                            // OR in held-button bits — WebKit needs
                            // these on every move during a drag to
                            // know the button is still down.
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
                        // Drop pointer state so the next entry starts
                        // fresh (no spurious huge delta).
                        state.last_pointer = None;
                        None
                    }
                    _ => None,
                };
                if let Some(e) = ev {
                    let _ = self.slot.releaser.send(Cmd::Input(e));
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::Keyboard(k) => {
                // Track the modifier set ourselves so we can stamp
                // it onto mouse events that arrive without their
                // own modifier snapshot.
                if let keyboard::Event::ModifiersChanged(m) = k {
                    state.modifiers = *m;
                }
                if let Some(e) = input::translate_keyboard(k, time_ms) {
                    let _ = self.slot.releaser.send(Cmd::Input(e));
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::Window(w) => {
                use iced::window::Event as WE;
                match w {
                    WE::Focused => {
                        let _ = self.slot.releaser.send(Cmd::Focus(true));
                    }
                    WE::Unfocused => {
                        let _ = self.slot.releaser.send(Cmd::Focus(false));
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
        // CSS cursor pushed by WebKit via the
        // `wpe_view_set_cursor_from_name` vmethod we hijacked.
        // Worker thread writes the discriminant into the shared
        // atomic; we read it here every render. Falls back to
        // Default if WebKit hasn't told us anything yet.
        let raw = self
            .slot
            .cursor
            .load(std::sync::atomic::Ordering::Relaxed);
        input::CursorKind::from_u32(raw).to_iced()
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
        // Mirror the iced widget's physical size to WPE so the
        // WebProcess re-lays out at the actual viewport size
        // instead of the headless default (1024x768). Runs on
        // every prepare but only sends a Cmd when the size
        // actually changes.
        let scale = viewport.scale_factor() as f32;
        let req_w = (bounds.width * scale).round().max(1.0) as u32;
        let req_h = (bounds.height * scale).round().max(1.0) as u32;
        let mut last = self.slot.last_size.lock().unwrap();
        if *last != (req_w, req_h) {
            *last = (req_w, req_h);
            drop(last);
            let _ = self
                .slot
                .releaser
                .send(Cmd::Resize {
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

        let new_token = frame.token;
        let meta = DmabufMetadata {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            stride: frame.stride,
            offset: frame.offset,
        };
        let imported = match unsafe { wgpu_import::import(device, frame.fd, &meta) } {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("wgpu_import::import failed: {e}");
                // Don't send Release for `new_token` either — WPE
                // already considers the buffer in flight; releasing
                // a buffer we never "consumed" would confuse it.
                return;
            }
        };

        let view = imported
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        pipeline.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wpe-shader bg"),
            layout: &pipeline.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                },
            ],
        }));

        // Release the previous frame's buffer back to WPE so it can
        // recycle it. Order matters — `pipeline.current` Drop runs
        // *after* this swap, so the previous wgpu::Texture stays
        // alive until we've already told WPE the underlying buffer
        // is free. That's safe because wgpu's texture wraps the
        // *imported* memory, not the producer's memory; the producer
        // (WPE) reuses its own buffer pool independently.
        if let Some(prev) = pipeline.current.take() {
            let _ = self.slot.releaser.send(Cmd::Release { token: prev.token });
        }
        pipeline.current = Some(CurrentFrame {
            _imported: imported,
            token: new_token,
            size: (frame.width, frame.height),
        });

        // FPS counter — log every ~1s. Bench harness scrapes this
        // out of the log file.
        pipeline.fps_count += 1;
        let elapsed = pipeline.fps_window_start.elapsed();
        if elapsed >= std::time::Duration::from_secs(1) {
            let fps = pipeline.fps_count as f64 / elapsed.as_secs_f64();
            tracing::info!(fps = format!("{:.1}", fps), "shader fps");
            pipeline.fps_count = 0;
            pipeline.fps_window_start = std::time::Instant::now();
        }
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        // Decide load op + whether to draw based on what state the
        // pipeline is in. The earlier blanket Clear(BLACK) caused a
        // visible black rectangle on hover transitions when iced
        // submitted a render of a sub-region: the clear wiped the
        // target, and any pixels not subsequently re-drawn this
        // present went out black.
        //
        // Three states:
        //   - No frame imported yet → Clear(BLACK), no draw. The
        //     target shows black instead of iced's default white.
        //   - Frame imported, size mismatches our last Resize → Load,
        //     no draw. We preserve whatever was there (either the
        //     prior black from startup, or a previous good frame
        //     while WPE catches up to a resize).
        //   - Frame imported, size matches → Load + draw. Normal
        //     steady-state path.
        let (load_op, do_draw) = match pipeline.current.as_ref() {
            None => (wgpu::LoadOp::Clear(wgpu::Color::BLACK), false),
            Some(current) => {
                let requested = *self.slot.last_size.lock().unwrap();
                if current.size == requested {
                    (wgpu::LoadOp::Load, true)
                } else {
                    (wgpu::LoadOp::Load, false)
                }
            }
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("wpe sample pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: load_op,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let Some(bg) = (do_draw).then_some(pipeline.bind_group.as_ref()).flatten() else {
            return;
        };

        // The fullscreen triangle covers NDC -1..1. Without
        // `set_viewport`, that maps across the entire surface
        // texture and the WPE buffer's top-left lands at the
        // window's top-left — putting the page's first scanline
        // under the chrome bar. Scissor only clips writes, it
        // doesn't re-map UVs. set_viewport, by contrast, remaps
        // NDC -1..1 to a sub-rect of the target, which is what
        // we want: triangle covers the widget bounds exactly,
        // UVs interpolate across the widget bounds, page top
        // lands at the widget's top edge.
        pass.set_viewport(
            clip_bounds.x as f32,
            clip_bounds.y as f32,
            clip_bounds.width as f32,
            clip_bounds.height as f32,
            0.0,
            1.0,
        );
        // Scissor still belt-and-braces — if iced ever passes us
        // a clip_bounds smaller than the widget bounds (clipped
        // by an ancestor container), scissor enforces it.
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[derive(Debug)]
struct CurrentFrame {
    _imported: ImportedFrame,
    token: ResourceToken,
    /// Pixel size of the imported DMA-BUF. Compared in `render()`
    /// against the size we last requested from WPE — if WPE is
    /// still catching up (e.g. immediately after window open,
    /// frames arrive at the headless 1024x768 default before our
    /// resize propagates), we skip drawing so the user sees the
    /// clear color instead of a stretched intermediate frame.
    size: (u32, u32),
}

#[derive(Debug)]
pub struct WpePipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    current: Option<CurrentFrame>,
    /// FPS counter — tracks frames imported since `fps_window_start`
    /// and logs at info every ~1s. Picked up by the bench harness
    /// out of /opt/sola/log/sola.log.
    fps_count: u64,
    fps_window_start: std::time::Instant,
}

impl shader::Pipeline for WpePipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("wpe sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wpe bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wpe shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wpe pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wpe rp"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            bind_group: None,
            current: None,
            fps_count: 0,
            fps_window_start: std::time::Instant::now(),
        }
    }
}

const SHADER_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var out: VsOut;
    let x = f32((vid << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vid & 2u) * 2.0 - 1.0;
    out.pos = vec4<f32>(x, -y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;
