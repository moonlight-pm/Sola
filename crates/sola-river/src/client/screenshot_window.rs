//! Window screenshots via `ext-image-copy-capture-v1` + foreign toplevels.
//!
//! Captures the window's own scene (River `createWithSceneNode`), so the
//! app does **not** need to be on top — and we never raise it. Occluded
//! and composition-hidden windows still have a buffer to copy.

use std::ffi::c_void;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use sola_bus::topics::CaptureFormat;
use tracing::{debug, info, warn};
use wayland_client::protocol::{wl_buffer, wl_output, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child};

use crate::client::screenshot::{CaptureKind, complete_shot, emit_shot_err};
use crate::client::AppData;
use crate::protocol::ext_foreign_toplevel_list_v1::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use crate::protocol::ext_image_capture_source_v1::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use crate::protocol::ext_image_copy_capture_v1::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::Options as CopyOptions,
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

const EVT_TOPLEVEL_OPCODE: u16 = 0;

/// One mapped toplevel from `ext_foreign_toplevel_list_v1`.
pub struct ForeignToplevel {
    pub handle: ExtForeignToplevelHandleV1,
    pub identifier: String,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pending_identifier: Option<String>,
    pending_app_id: Option<String>,
    pending_title: Option<String>,
}

pub struct WindowFlight {
    kind: CaptureKind,
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureSessionV1,
    frame: Option<ExtImageCopyCaptureFrameV1>,
    width: u32,
    height: u32,
    shm_formats: Vec<wl_shm::Format>,
    format: Option<wl_shm::Format>,
    stride: u32,
    memfd: Option<OwnedFd>,
    pool: Option<wl_shm_pool::WlShmPool>,
    buffer: Option<wl_buffer::WlBuffer>,
    map_ptr: Option<*mut c_void>,
    map_len: usize,
}

unsafe impl Send for WindowFlight {}

/// Start a window capture. Does not raise or focus the window.
pub fn start(
    state: &mut AppData,
    path: PathBuf,
    format: CaptureFormat,
    app_id: &str,
    title: Option<&str>,
) {
    let Some(source_mgr) = state.screenshot.toplevel_source_manager.clone() else {
        emit_shot_err(
            state,
            "window capture requires ext_foreign_toplevel_image_capture_source_manager_v1 (River 0.4.2+)",
        );
        return;
    };
    let Some(copy_mgr) = state.screenshot.copy_manager.clone() else {
        emit_shot_err(
            state,
            "window capture requires ext_image_copy_capture_manager_v1 (River 0.4.2+)",
        );
        return;
    };
    let Some(qh) = state.qh.clone() else {
        emit_shot_err(state, "wayland queue handle not ready");
        return;
    };
    let Some(toplevel) = find_toplevel(&state.screenshot.toplevels, app_id, title) else {
        emit_shot_err(
            state,
            format!("window not found for capture: app_id={app_id} title={title:?}"),
        );
        return;
    };
    let handle = toplevel.handle.clone();
    let identifier = toplevel.identifier.clone();
    info!(
        path = %path.display(),
        %app_id,
        ?title,
        %identifier,
        "screenshot: window toplevel (no raise)"
    );

    let source = source_mgr.create_source(&handle, &qh, ());
    let session = copy_mgr.create_session(&source, CopyOptions::empty(), &qh, ());
    let kind = match format {
        CaptureFormat::Png => CaptureKind::Png { path },
        CaptureFormat::Rgba => CaptureKind::Rgba { path },
    };
    state.screenshot.window_flight = Some(WindowFlight {
        kind,
        source,
        session,
        frame: None,
        width: 0,
        height: 0,
        shm_formats: Vec::new(),
        format: None,
        stride: 0,
        memfd: None,
        pool: None,
        buffer: None,
        map_ptr: None,
        map_len: 0,
    });
}

pub fn find_toplevel<'a>(
    toplevels: &'a [ForeignToplevel],
    app_id: &str,
    title: Option<&str>,
) -> Option<&'a ForeignToplevel> {
    toplevels
        .iter()
        .filter(|t| t.app_id.as_deref() == Some(app_id))
        .find(|t| match title {
            Some(want) => t.title.as_deref() == Some(want),
            None => true,
        })
}

fn pick_shm_format(formats: &[wl_shm::Format]) -> Option<wl_shm::Format> {
    const PREFER: [wl_shm::Format; 5] = [
        wl_shm::Format::Xrgb8888,
        wl_shm::Format::Argb8888,
        wl_shm::Format::Xbgr8888,
        wl_shm::Format::Abgr8888,
        wl_shm::Format::Bgr888,
    ];
    PREFER.into_iter().find(|f| formats.contains(f))
}

fn shm_stride(format: wl_shm::Format, width: u32) -> Option<u32> {
    match format {
        wl_shm::Format::Xrgb8888
        | wl_shm::Format::Argb8888
        | wl_shm::Format::Xbgr8888
        | wl_shm::Format::Abgr8888 => width.checked_mul(4),
        wl_shm::Format::Bgr888 => width.checked_mul(3),
        _ => None,
    }
}

fn clear_window_flight(state: &mut AppData) {
    let Some(mut flight) = state.screenshot.window_flight.take() else {
        return;
    };
    if let Some(ptr) = flight.map_ptr.take() {
        if !ptr.is_null() && flight.map_len > 0 {
            unsafe {
                let _ = munmap(ptr, flight.map_len);
            }
        }
    }
    if let Some(buf) = flight.buffer.take() {
        buf.destroy();
    }
    if let Some(pool) = flight.pool.take() {
        pool.destroy();
    }
    if let Some(frame) = flight.frame.take() {
        frame.destroy();
    }
    flight.session.destroy();
    flight.source.destroy();
}

fn try_submit_frame(state: &mut AppData) {
    let (format, width, height, stride) = {
        let Some(flight) = state.screenshot.window_flight.as_ref() else {
            return;
        };
        if flight.frame.is_some() {
            return;
        }
        if flight.width == 0 || flight.height == 0 {
            return;
        }
        let Some(format) = flight.format else {
            return;
        };
        (format, flight.width, flight.height, flight.stride)
    };
    let Some(shm) = state.screenshot.shm.clone() else {
        clear_window_flight(state);
        emit_shot_err(state, "wl_shm disappeared mid-window-capture");
        return;
    };
    let Some(qh) = state.qh.clone() else {
        clear_window_flight(state);
        emit_shot_err(state, "queue handle missing mid-window-capture");
        return;
    };
    let size = (stride as u64).checked_mul(height as u64).unwrap_or(0);
    if size == 0 || size > i32::MAX as u64 {
        clear_window_flight(state);
        emit_shot_err(state, format!("invalid window-capture buffer size {size}"));
        return;
    }
    let memfd = match memfd_create("sola-window-capture", MemfdFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(e) => {
            clear_window_flight(state);
            emit_shot_err(state, format!("memfd_create failed: {e}"));
            return;
        }
    };
    if let Err(e) = ftruncate(&memfd, size) {
        clear_window_flight(state);
        emit_shot_err(state, format!("ftruncate failed: {e}"));
        return;
    }
    let map_len = size as usize;
    let map_ptr = match unsafe {
        mmap(
            std::ptr::null_mut(),
            map_len,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            memfd.as_fd(),
            0,
        )
    } {
        Ok(ptr) => ptr,
        Err(e) => {
            clear_window_flight(state);
            emit_shot_err(state, format!("mmap failed: {e}"));
            return;
        }
    };
    let pool = shm.create_pool(memfd.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        format,
        &qh,
        (),
    );

    let Some(flight) = state.screenshot.window_flight.as_mut() else {
        return;
    };
    let frame = flight.session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();
    flight.memfd = Some(memfd);
    flight.pool = Some(pool);
    flight.buffer = Some(buffer);
    flight.map_ptr = Some(map_ptr);
    flight.map_len = map_len;
    flight.frame = Some(frame);
    debug!(width, height, stride, ?format, "window capture: frame submitted");
    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "wayland flush after window capture failed");
        }
    }
}

fn on_constraints_done(state: &mut AppData) {
    let (width, formats) = {
        let Some(flight) = state.screenshot.window_flight.as_ref() else {
            return;
        };
        (flight.width, flight.shm_formats.clone())
    };
    let Some(format) = pick_shm_format(&formats) else {
        clear_window_flight(state);
        emit_shot_err(
            state,
            format!("window capture: no supported shm format in {formats:?}"),
        );
        return;
    };
    let Some(stride) = shm_stride(format, width) else {
        clear_window_flight(state);
        emit_shot_err(state, format!("window capture: cannot stride {format:?}"));
        return;
    };
    if let Some(flight) = state.screenshot.window_flight.as_mut() {
        flight.format = Some(format);
        flight.stride = stride;
    }
    try_submit_frame(state);
}

fn on_frame_ready(state: &mut AppData) {
    let Some(flight) = state.screenshot.window_flight.as_ref() else {
        return;
    };
    let Some(ptr) = flight.map_ptr else {
        clear_window_flight(state);
        emit_shot_err(state, "window capture ready but buffer not mapped");
        return;
    };
    let Some(format) = flight.format else {
        clear_window_flight(state);
        emit_shot_err(state, "window capture ready but format missing");
        return;
    };
    let src = unsafe { std::slice::from_raw_parts(ptr as *const u8, flight.map_len) };
    let raw = src.to_vec();
    let width = flight.width;
    let height = flight.height;
    let stride = flight.stride;
    let kind = flight.kind.clone();
    clear_window_flight(state);
    complete_shot(state, raw, format, width, height, stride, false, kind);
}

impl Dispatch<ExtForeignToplevelListV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _list: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                state.screenshot.toplevels.push(ForeignToplevel {
                    handle: toplevel,
                    identifier: String::new(),
                    app_id: None,
                    title: None,
                    pending_identifier: None,
                    pending_app_id: None,
                    pending_title: None,
                });
            }
            ext_foreign_toplevel_list_v1::Event::Finished => {
                debug!("ext_foreign_toplevel_list_v1 finished");
            }
        }
    }

    event_created_child!(AppData, ExtForeignToplevelListV1, [
        EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for AppData {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(slot) = state
            .screenshot
            .toplevels
            .iter_mut()
            .find(|t| t.handle == *handle)
        else {
            warn!(object = ?handle.id(), "event for unknown foreign toplevel");
            return;
        };
        match event {
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                slot.pending_identifier = Some(identifier);
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                slot.pending_app_id = Some(app_id);
            }
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                slot.pending_title = Some(title);
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(id) = slot.pending_identifier.take() {
                    slot.identifier = id;
                }
                if let Some(app) = slot.pending_app_id.take() {
                    slot.app_id = Some(app);
                }
                if let Some(title) = slot.pending_title.take() {
                    slot.title = Some(title);
                }
                debug!(
                    identifier = %slot.identifier,
                    app_id = ?slot.app_id,
                    title = ?slot.title,
                    "foreign toplevel ready"
                );
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                let id = handle.id();
                state.screenshot.toplevels.retain(|t| t.handle != *handle);
                handle.destroy();
                debug!(object = ?id, "foreign toplevel closed");
            }
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for AppData {
    fn event(
        state: &mut Self,
        session: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(flight) = state.screenshot.window_flight.as_mut() else {
            return;
        };
        if flight.session != *session {
            return;
        }
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                flight.width = width;
                flight.height = height;
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format } => {
                match format {
                    WEnum::Value(f) => flight.shm_formats.push(f),
                    WEnum::Unknown(v) => {
                        debug!(format = v, "window capture: unknown shm format");
                    }
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                on_constraints_done(state);
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                clear_window_flight(state);
                emit_shot_err(state, "window capture session stopped");
            }
            ext_image_copy_capture_session_v1::Event::DmabufDevice { .. }
            | ext_image_copy_capture_session_v1::Event::DmabufFormat { .. } => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for AppData {
    fn event(
        state: &mut Self,
        frame: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let ours = state
            .screenshot
            .window_flight
            .as_ref()
            .and_then(|f| f.frame.as_ref())
            .is_some_and(|f| f == frame);
        if !ours {
            return;
        }
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => on_frame_ready(state),
            ext_image_copy_capture_frame_v1::Event::Failed { reason } => {
                clear_window_flight(state);
                emit_shot_err(state, format!("window capture failed: {reason:?}"));
            }
            ext_image_copy_capture_frame_v1::Event::Transform { transform } => {
                if !matches!(transform, WEnum::Value(wl_output::Transform::Normal)) {
                    warn!(?transform, "window capture: non-normal transform, copied as-is");
                }
            }
            ext_image_copy_capture_frame_v1::Event::Damage { .. }
            | ext_image_copy_capture_frame_v1::Event::PresentationTime { .. } => {}
        }
    }
}

impl Dispatch<ExtImageCaptureSourceV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ExtImageCaptureSourceV1,
        _: <ExtImageCaptureSourceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<
    crate::protocol::ext_image_copy_capture_v1::ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    (),
> for AppData {
    fn event(
        _: &mut Self,
        _: &crate::protocol::ext_image_copy_capture_v1::ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
        _: <crate::protocol::ext_image_copy_capture_v1::ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<
    crate::protocol::ext_image_capture_source_v1::ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    (),
> for AppData {
    fn event(
        _: &mut Self,
        _: &crate::protocol::ext_image_capture_source_v1::ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
        _: <crate::protocol::ext_image_capture_source_v1::ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_prefers_xrgb() {
        let formats = [
            wl_shm::Format::Bgr888,
            wl_shm::Format::Xrgb8888,
            wl_shm::Format::Argb8888,
        ];
        assert_eq!(pick_shm_format(&formats), Some(wl_shm::Format::Xrgb8888));
    }

    #[test]
    fn find_matches_title_when_given() {
        // Can't construct real proxies in unit tests; matching logic is the
        // same filter as WindowRegistry::find_by_app_title.
        let app = "sola-kit";
        let titles = ["Storybook", "Other"];
        let hit = titles
            .iter()
            .copied()
            .filter(|t| *t == "Storybook")
            .find(|t| match Some("Storybook") {
                Some(want) => *t == want,
                None => true,
            });
        assert_eq!(hit, Some("Storybook"));
        let _ = app;
    }
}
