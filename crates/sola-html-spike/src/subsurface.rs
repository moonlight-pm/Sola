//! Same-process `wl_subsurface` in the CSS hole. SHM buffer, not the parent swapchain.

use std::fs::File;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use memmap2::MmapMut;
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_subcompositor,
    wl_subsurface, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use winit::window::Window;

pub struct Hole {
    conn: Connection,
    queue: EventQueue<World>,
    world: World,
}

struct World {
    compositor: Option<wl_compositor::WlCompositor>,
    subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    shm: Option<wl_shm::WlShm>,
    child: Option<wl_surface::WlSurface>,
    sub: Option<wl_subsurface::WlSubsurface>,
    pool: Option<wl_shm_pool::WlShmPool>,
    file: Option<File>,
    mmap: Option<MmapMut>,
    pool_len: usize,
    slots: Vec<Slot>,
    pos: (i32, i32),
    logical: (i32, i32),
    scale: i32,
}

struct Slot {
    buffer: wl_buffer::WlBuffer,
    busy: Arc<AtomicBool>,
    offset: i32,
    w: i32,
    h: i32,
}

impl Hole {
    pub fn new(window: &Window) -> Option<Self> {
        let dpy = match window.display_handle().ok()?.as_raw() {
            RawDisplayHandle::Wayland(h) => h.display.as_ptr() as *mut _,
            _ => {
                tracing::warn!("subsurface: not a Wayland display");
                return None;
            }
        };
        let parent_ptr = match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Wayland(h) => h.surface.as_ptr() as *mut _,
            _ => {
                tracing::warn!("subsurface: not a Wayland surface");
                return None;
            }
        };

        let backend = unsafe { Backend::from_foreign_display(dpy) };
        let conn = Connection::from_backend(backend);
        let mut queue: EventQueue<World> = conn.new_event_queue();
        let qh = queue.handle();
        let display = conn.display();
        let _registry = display.get_registry(&qh, ());
        let mut world = World {
            compositor: None,
            subcompositor: None,
            shm: None,
            child: None,
            sub: None,
            pool: None,
            file: None,
            mmap: None,
            pool_len: 0,
            slots: Vec::new(),
            pos: (0, 0),
            logical: (0, 0),
            scale: 1,
        };
        if let Err(e) = queue.roundtrip(&mut world) {
            tracing::warn!(%e, "subsurface registry roundtrip");
            return None;
        }
        let compositor = world.compositor.clone()?;
        let subcompositor = world.subcompositor.clone()?;
        let _shm = world.shm.clone()?;

        let parent_id =
            unsafe { ObjectId::from_ptr(wl_surface::WlSurface::interface(), parent_ptr).ok()? };
        let parent = wl_surface::WlSurface::from_id(&conn, parent_id).ok()?;

        let child = compositor.create_surface(&qh, ());
        let region = compositor.create_region(&qh, ());
        child.set_input_region(Some(&region));
        region.destroy();
        let sub = subcompositor.get_subsurface(&child, &parent, &qh, ());
        sub.set_desync();
        sub.set_position(0, 0);
        let child_ptr = child.id().as_ptr() as *mut _;
        world.child = Some(child);
        world.sub = Some(sub);
        if let Err(e) = conn.flush() {
            tracing::warn!(%e, "subsurface flush");
            return None;
        }
        tracing::info!("wayland subsurface attached (desync, empty input region)");
        // Do not bind wgpu/Vulkan WSI to this wl_surface. River advertises
        // wp_linux_drm_syncobj; mixing that with SHM attach (foreign frames)
        // raises "Buffer attached but no acquire point set" and kills the
        // client. GPU stripes were a prior proof; the hole is SHM-only now.
        let _ = (dpy, child_ptr);
        Some(Self {
            conn,
            queue,
            world,
        })
    }

    pub fn live(&self) -> bool {
        self.world.sub.is_some()
    }

    /// `x,y,w,h` in CSS / parent surface coordinates. `scale` is buffer scale.
    pub fn update(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        scale: f32,
        time: f32,
        foreign: Option<(&[u32], u32, u32)>,
    ) {
        let _ = self.queue.dispatch_pending(&mut self.world);
        let scale = scale.round().max(1.0) as i32;
        let px = x.round() as i32;
        let py = y.round() as i32;
        let lw = w.round().max(1.0) as i32;
        let lh = h.round().max(1.0) as i32;
        let bw = (lw * scale).max(1);
        let bh = (lh * scale).max(1);
        if self.world.pos != (px, py) {
            if let Some(sub) = &self.world.sub {
                sub.set_position(px, py);
            }
            self.world.pos = (px, py);
        }
        let size_changed = self.world.logical != (lw, lh) || self.world.scale != scale;
        if size_changed {
            self.world.logical = (lw, lh);
            self.world.scale = scale;
            tracing::info!(
                x = px,
                y = py,
                css_w = lw,
                css_h = lh,
                scale,
                "subsurface hole (SHM)"
            );
        }
        if let Some((pix, fw, fh)) = foreign {
            self.present_pixels(pix, fw, fh);
        } else {
            if size_changed {
                self.world.rebuild_buffers(bw, bh, &self.queue.handle());
                if let Some(child) = &self.world.child {
                    child.set_buffer_scale(scale);
                }
            }
            self.world.paint_and_commit(time);
        }
        let _ = self.conn.flush();
    }

    pub fn present_pixels(&mut self, pix: &[u32], width: u32, height: u32) {
        let w = width.max(1) as i32;
        let h = height.max(1) as i32;
        let need = self
            .world
            .slots
            .first()
            .is_none_or(|s| s.w != w || s.h != h);
        if need {
            self.world.rebuild_buffers(w, h, &self.queue.handle());
        }
        let Some(idx) = self
            .world
            .slots
            .iter()
            .position(|s| !s.busy.load(Ordering::Acquire))
        else {
            return;
        };
        let (offset, bw, bh) = {
            let s = &self.world.slots[idx];
            (s.offset as usize, s.w as usize, s.h as usize)
        };
        let Some(mmap) = self.world.mmap.as_mut() else {
            return;
        };
        let n = bw * bh;
        let bytes = n * 4;
        if let Some(dst) = mmap.get_mut(offset..offset + bytes) {
            for i in 0..n.min(pix.len()) {
                let p = pix[i];
                let o = i * 4;
                dst[o] = (p & 0xff) as u8;
                dst[o + 1] = ((p >> 8) & 0xff) as u8;
                dst[o + 2] = ((p >> 16) & 0xff) as u8;
                dst[o + 3] = 0xFF;
            }
        }
        let slot = &self.world.slots[idx];
        slot.busy.store(true, Ordering::Release);
        if let Some(child) = &self.world.child {
            child.attach(Some(&slot.buffer), 0, 0);
            child.damage_buffer(0, 0, slot.w, slot.h);
            child.commit();
        }
        let _ = self.conn.flush();
    }
}

impl World {
    fn rebuild_buffers(&mut self, w: i32, h: i32, qh: &QueueHandle<Self>) {
        for slot in self.slots.drain(..) {
            slot.buffer.destroy();
        }
        let stride = w.saturating_mul(4);
        let one = (stride as usize).saturating_mul(h as usize);
        let need = one.saturating_mul(2).max(4096);
        if self.pool.is_none() {
            let Some(shm) = self.shm.clone() else {
                return;
            };
            let Ok(fd) = memfd(need as u64) else {
                tracing::warn!("subsurface memfd");
                return;
            };
            let file = File::from(fd);
            let pool = shm.create_pool(file.as_fd(), need as i32, qh, ());
            let mmap = match unsafe { MmapMut::map_mut(&file) } {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(%e, "subsurface mmap");
                    return;
                }
            };
            self.file = Some(file);
            self.pool = Some(pool);
            self.mmap = Some(mmap);
            self.pool_len = need;
        } else if need > self.pool_len {
            if let Some(file) = &self.file {
                let _ = file.set_len(need as u64);
            }
            if let Some(pool) = &self.pool {
                pool.resize(need as i32);
            }
            if let Some(file) = &self.file {
                match unsafe { MmapMut::map_mut(file) } {
                    Ok(m) => self.mmap = Some(m),
                    Err(e) => tracing::warn!(%e, "subsurface remap"),
                }
            }
            self.pool_len = need;
        }
        let Some(pool) = self.pool.clone() else {
            return;
        };
        for i in 0..2 {
            let offset = (i * one) as i32;
            let busy = Arc::new(AtomicBool::new(false));
            let buffer = pool.create_buffer(
                offset,
                w,
                h,
                stride,
                wl_shm::Format::Argb8888,
                qh,
                busy.clone(),
            );
            self.slots.push(Slot {
                buffer,
                busy,
                offset,
                w,
                h,
            });
        }
    }

    fn paint_and_commit(&mut self, time: f32) {
        let Some(idx) = self
            .slots
            .iter()
            .position(|s| !s.busy.load(Ordering::Acquire))
        else {
            return;
        };
        let (offset, bw, bh) = {
            let s = &self.slots[idx];
            (s.offset as usize, s.w as u32, s.h as u32)
        };
        let Some(mmap) = self.mmap.as_mut() else {
            return;
        };
        let stride = (bw * 4) as usize;
        let bytes = stride * bh as usize;
        let Some(dst) = mmap.get_mut(offset..offset + bytes) else {
            return;
        };
        fill_stripes(dst, bw, bh, time);
        let slot = &self.slots[idx];
        slot.busy.store(true, Ordering::Release);
        let Some(child) = &self.child else {
            return;
        };
        child.attach(Some(&slot.buffer), 0, 0);
        child.damage_buffer(0, 0, slot.w, slot.h);
        child.commit();
    }
}

fn fill_stripes(dst: &mut [u8], w: u32, h: u32, time: f32) {
    let off = (time * 48.0) as i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let band = ((x + y + off) / 16) & 1 == 0;
            let (r, g, b) = if band {
                (0xE0u8, 0x6A, 0x1A)
            } else {
                (0x12, 0x1C, 0x2A)
            };
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            dst[i] = b;
            dst[i + 1] = g;
            dst[i + 2] = r;
            dst[i + 3] = 0xFF;
        }
    }
}

fn memfd(len: u64) -> std::io::Result<OwnedFd> {
    let fd = rustix::fs::memfd_create("html-spike-sub", rustix::fs::MemfdFlags::CLOEXEC)?;
    rustix::fs::ftruncate(&fd, len)?;
    Ok(fd)
}

impl Dispatch<wl_registry::WlRegistry, ()> for World {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_subcompositor" => {
                    state.subcompositor = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_subcompositor::WlSubcompositor,
        _: wl_subcompositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_subsurface::WlSubsurface, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_subsurface::WlSubsurface,
        _: wl_subsurface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_region::WlRegion, ()> for World {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for World {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        busy: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            busy.store(false, Ordering::Release);
        }
    }
}


