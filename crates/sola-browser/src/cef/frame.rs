//! iced `shader::Program` that samples the most recent CEF frame
//! as a fullscreen quad. Shared pipeline lives in `crate::shader`.

use std::sync::Arc;
use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use crate::shader::{ImportedTexture, SamplePipeline};
use crate::engine::{Cmd, FrameSlot};

use crate::cef::cpu_import::{self, UploadedFrame};
use crate::cef::engine::{CefEngine, CefFrame};
use crate::cef::input;

pub struct CefProgram {
    pub slot: Arc<FrameSlot<CefEngine>>,
}

impl std::fmt::Debug for CefProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CefProgram").finish_non_exhaustive()
    }
}

pub struct CefPrimitive {
    pub slot: Arc<FrameSlot<CefEngine>>,
}

impl std::fmt::Debug for CefPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CefPrimitive").finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct ProgramState {
    modifiers: keyboard::Modifiers,
    last_scale: f32,
    held_button_mods: u32,
    _started: Option<Instant>,
}

impl shader::Program<crate::app::Msg> for CefProgram {
    type State = ProgramState;
    type Primitive = CefPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        CefPrimitive {
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
        let (req_w, _req_h) = *self.slot.last_size.lock().unwrap();
        let scale = crate::input::scale_from_last_size(bounds, req_w, state.last_scale);
        state.last_scale = scale;
        let mods_now = state.modifiers;

        match event {
            iced::Event::Mouse(m) => {
                let cur = cursor.position_in(bounds)?;
                let (x, y) = crate::input::project_cursor_i32(
                    iced::Point::new(bounds.x + cur.x, bounds.y + cur.y),
                    bounds,
                    scale,
                );
                let kbd_mods = input::modifiers_to_cef_mouse(mods_now);
                let ev = match m {
                    mouse::Event::CursorMoved { .. } => Some(input::pointer_move(
                        x,
                        y,
                        state.held_button_mods,
                        kbd_mods,
                    )),
                    mouse::Event::ButtonPressed(b) => {
                        input::button_number(*b).map(|button| {
                            state.held_button_mods |= input::button_to_modifier(button);
                            input::pointer_button(
                                true,
                                button,
                                x,
                                y,
                                state.held_button_mods,
                                kbd_mods,
                            )
                        })
                    }
                    mouse::Event::ButtonReleased(b) => {
                        input::button_number(*b).map(|button| {
                            state.held_button_mods &= !input::button_to_modifier(button);
                            input::pointer_button(
                                false,
                                button,
                                x,
                                y,
                                state.held_button_mods,
                                kbd_mods,
                            )
                        })
                    }
                    mouse::Event::WheelScrolled { delta } => {
                        let (dx, dy, precise) = input::scroll_delta_to_cef(*delta);
                        Some(input::scroll(
                            x,
                            y,
                            dx,
                            dy,
                            precise,
                            state.held_button_mods,
                            kbd_mods,
                        ))
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
                                crate::app::Msg::WebViewFocused,
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
                let translated = match k {
                    keyboard::Event::KeyPressed {
                        key, modifiers, text, ..
                    } => input::translate_key(
                        true,
                        key,
                        text.as_ref().and_then(|t| t.chars().next()),
                        *modifiers,
                    ),
                    keyboard::Event::KeyReleased {
                        key, modifiers, ..
                    } => input::translate_key(false, key, None, *modifiers),
                    keyboard::Event::ModifiersChanged(_) => None,
                };
                if let Some(e) = translated {
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

struct CefImporter {
    /// Reused texture across frames of the same size.
    texture: Option<UploadedFrame>,
}

impl CefImporter {
    fn import_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        frame: CefFrame,
    ) -> Option<ImportedTexture> {
        let need_new = match &self.texture {
            Some(cur) => {
                // size tracked via texture size
                let size = cur.texture.size();
                size.width != frame.width || size.height != frame.height
            }
            None => true,
        };
        if need_new {
            let texture = cpu_import::create_texture(device, frame.width, frame.height);
            self.texture = Some(UploadedFrame { texture });
        }
        let uploaded = self.texture.as_ref()?;
        cpu_import::upload(queue, &uploaded.texture, &frame);
        let view = uploaded
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cef-shader bg"),
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
        Some(ImportedTexture {
            bind_group,
            size: (frame.width, frame.height),
        })
    }
}

impl shader::Primitive for CefPrimitive {
    type Pipeline = CefPipeline;

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
                scale: scale as f64,
            });
        }

        let paint_tab = self.slot.paint_tab.load(std::sync::atomic::Ordering::Relaxed);
        let mut guard = self.slot.pending.lock().unwrap();
        let Some(pending) = guard.take() else {
            return;
        };
        // Stale frame for a tab we already left — drop; engine parks per-tab.
        if pending.tab_id.0 != paint_tab {
            return;
        }
        drop(guard);

        if let Some(imported) = pipeline.importer.import_into(
            device,
            queue,
            &pipeline.sample.bind_group_layout,
            &pipeline.sample.sampler,
            pending.frame,
        ) {
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
            .render(encoder, target, clip_bounds, requested, "cef sample pass");
    }
}

#[derive(Debug)]
pub struct CefPipeline {
    sample: SamplePipeline,
    importer: CefImporter,
}

impl std::fmt::Debug for CefImporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CefImporter").finish_non_exhaustive()
    }
}

impl shader::Pipeline for CefPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            sample: SamplePipeline::new(device, queue, format, "cef"),
            importer: CefImporter { texture: None },
        }
    }
}


