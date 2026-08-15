//! iced `shader::Program` that samples the most recent CEF frame
//! as a fullscreen quad. Shared pipeline lives in `crate::shader`.

use std::sync::Arc;
use std::time::Instant;

use iced::widget::shader;
use iced::{Rectangle, keyboard, mouse};

use crate::engine::{Cmd, FrameSlot};
use crate::shader::{ImportedTexture, SamplePipeline};

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
    /// Last *press*: button, x, y, time, count.
    last_press: Option<(u32, i32, i32, Instant, u32)>,
    last_click_count: u32,
    last_pointer: Option<(i32, i32)>,
    pointer_in: bool,
    /// True while an IME preedit is live — suppress CHAR so we do not
    /// double-insert next to `ImeSetComposition`.
    composing: bool,
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
        // Chrome subscription keeps this current even when the URL bar
        // owns iced focus (so ⌘-click still sees Super).
        let mods_now = crate::input::stored_modifiers();

        match event {
            iced::Event::Mouse(m) => {
                let over = cursor.position_in(bounds);
                if over.is_none() {
                    if matches!(m, mouse::Event::CursorMoved { .. }) && state.pointer_in {
                        state.pointer_in = false;
                        if let Some((x, y)) = state.last_pointer {
                            let kbd_mods = input::modifiers_to_cef_mouse(mods_now);
                            let _ = self.slot.cmd_tx.send(Cmd::Input(input::pointer_leave(
                                x,
                                y,
                                state.held_button_mods,
                                kbd_mods,
                            )));
                            return Some(iced::widget::shader::Action::capture());
                        }
                    }
                    // Wheel / buttons outside the page belong to chrome.
                    return None;
                }
                let cur = over?;
                let (x, y) = crate::input::project_cursor_i32(
                    iced::Point::new(bounds.x + cur.x, bounds.y + cur.y),
                    bounds,
                    scale,
                );
                state.last_pointer = Some((x, y));
                state.pointer_in = true;
                note_ime_fallback(&self.slot, x, y);
                let kbd_mods = input::modifiers_to_cef_mouse(mods_now);
                let ev = match m {
                    mouse::Event::CursorMoved { .. } => {
                        Some(input::pointer_move(x, y, state.held_button_mods, kbd_mods))
                    }
                    mouse::Event::ButtonPressed(b) => input::button_number(*b).map(|button| {
                        state.held_button_mods |= input::button_to_modifier(button);
                        let prev = state.last_press.map(|(pb, px, py, at, count)| {
                            (pb, px, py, at.elapsed().as_millis(), count)
                        });
                        let count = input::next_click_count(prev, button, x, y);
                        state.last_press = Some((button, x, y, Instant::now(), count));
                        state.last_click_count = count;
                        input::pointer_button(
                            true,
                            button,
                            x,
                            y,
                            state.held_button_mods,
                            kbd_mods,
                            count,
                        )
                    }),
                    mouse::Event::ButtonReleased(b) => input::button_number(*b).map(|button| {
                        state.held_button_mods &= !input::button_to_modifier(button);
                        input::pointer_button(
                            false,
                            button,
                            x,
                            y,
                            state.held_button_mods,
                            kbd_mods,
                            state.last_click_count.max(1),
                        )
                    }),
                    mouse::Event::WheelScrolled { delta } => {
                        let (dx, dy, precise) = input::scroll_delta_to_cef(*delta);
                        let (dx, dy) = input::apply_shift_scroll(dx, dy, mods_now.shift());
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
                let is_left_press = matches!(m, mouse::Event::ButtonPressed(mouse::Button::Left));
                if let Some(e) = ev {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    if is_left_press {
                        return Some(
                            iced::widget::shader::Action::publish(crate::app::Msg::WebViewFocused)
                                .and_capture(),
                        );
                    }
                    if should_pump(&self.slot) {
                        return Some(iced::widget::shader::Action::request_redraw().and_capture());
                    }
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::InputMethod(ime) => {
                use iced::advanced::input_method::Event as ImeEv;
                let ev = match ime {
                    ImeEv::Opened => None,
                    ImeEv::Preedit(text, sel) => {
                        state.composing = !text.is_empty();
                        if text.is_empty() {
                            Some(input::ime_set_composition(String::new(), None))
                        } else {
                            Some(input::ime_set_composition(text.clone(), sel.clone()))
                        }
                    }
                    ImeEv::Commit(text) => {
                        state.composing = false;
                        Some(input::ime_commit(text.clone()))
                    }
                    ImeEv::Closed => {
                        let cancel = state.composing;
                        state.composing = false;
                        cancel.then(input::ime_cancel)
                    }
                };
                if let Some(e) = ev {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    return Some(iced::widget::shader::Action::capture());
                }
            }
            iced::Event::Keyboard(k) => {
                if let keyboard::Event::ModifiersChanged(m) = k {
                    state.modifiers = *m;
                    crate::input::store_modifiers(*m);
                }
                // Chrome Edit menu owns ⌘C/X/V/A/Z — do not also send them
                // to CEF or a page field pastes twice (JS + native).
                if let keyboard::Event::KeyPressed { key, modifiers, .. }
                | keyboard::Event::KeyReleased { key, modifiers, .. } = k
                {
                    crate::input::store_modifiers(*modifiers);
                    if crate::input::is_super_key(key) {
                        crate::input::note_super_key(matches!(
                            k,
                            keyboard::Event::KeyPressed { .. }
                        ));
                    }
                    if crate::input::is_chrome_edit_shortcut(key, crate::input::stored_modifiers())
                    {
                        return Some(iced::widget::shader::Action::capture());
                    }
                }
                // While composing, printable CHAR would double-insert next
                // to ImeSetComposition. Still send Escape (cancel) / arrows.
                if state.composing {
                    if let keyboard::Event::KeyPressed { key, .. } = k {
                        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                            state.composing = false;
                            let _ = self.slot.cmd_tx.send(Cmd::Input(input::ime_cancel()));
                            return Some(iced::widget::shader::Action::capture());
                        }
                        if matches!(key, keyboard::Key::Character(_)) {
                            return Some(iced::widget::shader::Action::capture());
                        }
                    }
                }
                let translated = match k {
                    keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text,
                        ..
                    } => input::translate_key(
                        true,
                        key,
                        text.as_ref().and_then(|t| t.chars().next()),
                        *modifiers,
                    ),
                    keyboard::Event::KeyReleased { key, modifiers, .. } => {
                        input::translate_key(false, key, None, *modifiers)
                    }
                    keyboard::Event::ModifiersChanged(_) => None,
                };
                if let Some(e) = translated {
                    let _ = self.slot.cmd_tx.send(Cmd::Input(e));
                    if should_pump(&self.slot) {
                        return Some(iced::widget::shader::Action::request_redraw().and_capture());
                    }
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
                    WE::RedrawRequested(_) => {
                        if should_pump(&self.slot) {
                            return Some(iced::widget::shader::Action::request_redraw());
                        }
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
        let raw = self.slot.cursor.load(std::sync::atomic::Ordering::Relaxed);
        crate::CursorKind::from_u32(raw).to_iced()
    }
}

struct CefImporter {
    /// Reused texture across frames of the same size.
    texture: Option<UploadedFrame>,
    bind_group: Option<wgpu::BindGroup>,
    /// 256-byte-aligned row staging — reused so we do not alloc per frame.
    staging: Vec<u8>,
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
                let size = cur.texture.size();
                size.width != frame.width || size.height != frame.height
            }
            None => true,
        };
        if need_new {
            let texture = cpu_import::create_texture(device, frame.width, frame.height);
            self.texture = Some(UploadedFrame { texture });
            self.bind_group = None;
        }
        let uploaded = self.texture.as_ref()?;
        cpu_import::upload(
            queue,
            &uploaded.texture,
            &frame,
            &mut self.staging,
            need_new,
        );
        if self.bind_group.is_none() {
            let view = uploaded
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            }));
        }
        Some(ImportedTexture {
            bind_group: self.bind_group.clone()?,
            size: (frame.width, frame.height),
        })
    }
}

fn note_ime_fallback(slot: &FrameSlot<CefEngine>, x: i32, y: i32) {
    if let Ok(mut caret) = slot.ime.lock() {
        if caret.w <= 0 || caret.h <= 0 {
            caret.x = x;
            caret.y = y;
        }
    }
}

fn should_pump(slot: &FrameSlot<CefEngine>) -> bool {
    use std::sync::atomic::Ordering;
    let pending = slot.pending.lock().unwrap().is_some();
    let last = slot.last_frame_ms.load(Ordering::Relaxed);
    let recent = last != 0
        && crate::engine::monotonic_ms().saturating_sub(last)
            < crate::engine::FRAME_PUMP_HANGOVER_MS;
    if pending || recent {
        slot.pumping.store(true, Ordering::Relaxed);
        return true;
    }
    slot.pumping.store(false, Ordering::Relaxed);
    if slot.pending.lock().unwrap().is_some() {
        slot.pumping.store(true, Ordering::Relaxed);
        true
    } else {
        false
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
        } else {
            drop(last);
        }
        let want = (req_w, req_h);
        let paint_tab = self
            .slot
            .paint_tab
            .load(std::sync::atomic::Ordering::Relaxed);
        let pending = {
            let mut guard = self.slot.pending.lock().unwrap();
            guard.take()
        };
        if let Some(pending) = pending {
            if pending.tab_id.0 == paint_tab
                && crate::shader::size_matches((pending.frame.width, pending.frame.height), want)
            {
                if let Some(imported) = pipeline.importer.import_into(
                    device,
                    queue,
                    &pipeline.sample.bind_group_layout,
                    &pipeline.sample.sampler,
                    pending.frame,
                ) {
                    pipeline.sample.install(imported);
                    pipeline.sample.note_frame();
                    pipeline.painted_tab = paint_tab;
                    self.slot
                        .blank_content
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    return;
                }
            }
            // Wrong tab, or a parked snapshot at the wrong size — drop.
        }
        // Different tab than what's on the GPU, or chrome asked to blank:
        // never keep the previous page up while we wait.
        let blank = self
            .slot
            .blank_content
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        if blank || pipeline.painted_tab != paint_tab {
            pipeline.sample.clear();
            pipeline.painted_tab = u64::MAX;
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
    /// Tab id currently sampled (`u64::MAX` = fallback / none).
    painted_tab: u64,
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
            importer: CefImporter {
                texture: None,
                bind_group: None,
                staging: Vec::new(),
            },
            painted_tab: u64::MAX,
        }
    }
}
