/// Minimal Wayland test client for Sola.
///
/// Connects to the Wayland compositor, creates a solid blue window, and
/// prints all pointer/keyboard events to stdout. Used to verify native
/// Wayland client support without XWayland in the path.
///
/// Usage: WAYLAND_DISPLAY=wayland-0 sola-wtest
use std::os::unix::io::{AsFd, AsRawFd, FromRawFd};

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

const WIDTH: i32 = 400;
const HEIGHT: i32 = 300;

fn main() {
    let conn = Connection::connect_to_env().expect("failed to connect to Wayland display");
    let display = conn.display();

    let mut event_queue = conn.new_event_queue::<App>();
    let qh = event_queue.handle();

    display.get_registry(&qh, ());

    let mut app = App::default();

    // Round-trip to bind globals.
    event_queue.roundtrip(&mut app).expect("roundtrip failed");

    let compositor = app.compositor.clone().expect("wl_compositor not found");
    let shm = app.shm.clone().expect("wl_shm not found");
    let xdg_wm_base = app.xdg_wm_base.clone().expect("xdg_wm_base not found");
    let seat = app.seat.clone().expect("wl_seat not found");

    // Get pointer and keyboard from seat.
    seat.get_pointer(&qh, ());
    seat.get_keyboard(&qh, ());

    // Create surface + xdg_surface + xdg_toplevel.
    let surface = compositor.create_surface(&qh, ());
    let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title("Wayland Test".into());
    toplevel.set_app_id("sola-wtest".into());
    surface.commit();

    // Wait for configure.
    event_queue.roundtrip(&mut app).expect("roundtrip failed");

    // Create a shared memory buffer with solid blue pixels.
    let stride = WIDTH * 4;
    let size = (stride * HEIGHT) as usize;

    let file = create_shm_file(size);
    let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, stride, wl_shm::Format::Xrgb8888, &qh, ());

    // Fill with blue (XRGB8888: XX RR GG BB in little-endian = bytes BB GG RR XX).
    unsafe {
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_fd().as_raw_fd(),
            0,
        ) as *mut u8;
        assert!(!ptr.is_null(), "mmap failed");
        let buf = std::slice::from_raw_parts_mut(ptr, size);
        for pixel in buf.chunks_exact_mut(4) {
            pixel[0] = 0xFF; // B
            pixel[1] = 0x00; // G
            pixel[2] = 0x00; // R
            pixel[3] = 0xFF; // X
        }
        libc::munmap(ptr as *mut _, size);
    }

    surface.attach(Some(&buffer), 0, 0);
    surface.commit();

    app.running = true;
    println!("sola-wtest: window opened, waiting for events...");
    println!("Move mouse over window, click, press keys. Press 'q' to quit.");

    while app.running {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("dispatch failed");
    }
}

/// Create an anonymous shared memory file for wl_shm.
fn create_shm_file(size: usize) -> std::fs::File {
    use std::ffi::CString;
    let name = CString::new("/sola-wtest-shm").unwrap();

    unsafe {
        // Clean up any stale file, then create fresh.
        libc::shm_unlink(name.as_ptr());
        let fd = libc::shm_open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        );
        assert!(fd >= 0, "shm_open failed");
        libc::shm_unlink(name.as_ptr()); // Unlink immediately; fd keeps it alive.
        libc::ftruncate(fd, size as libc::off_t);
        std::fs::File::from_raw_fd(fd)
    }
}

// -- State --

#[derive(Default)]
struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    xdg_wm_base: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    running: bool,
}

// -- Dispatch implementations --

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
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
                    state.compositor =
                        Some(registry.bind::<wl_compositor::WlCompositor, _, _>(name, version.min(6), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ()));
                }
                "xdg_wm_base" => {
                    state.xdg_wm_base =
                        Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, version.min(6), qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(9), qh, ()));
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore wl_callback::WlCallback);
delegate_noop!(App: ignore wl_seat::WlSeat);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        _state: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            proxy.ack_configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            println!("Close requested");
            state.running = false;
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            } => {
                println!("Pointer enter: ({surface_x}, {surface_y})");
            }
            wl_pointer::Event::Leave { .. } => {
                println!("Pointer leave");
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                println!("Pointer motion: ({surface_x}, {surface_y})");
            }
            wl_pointer::Event::Button {
                button,
                state: btn_state,
                ..
            } => {
                let pressed = matches!(btn_state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                println!("Pointer button: {button} pressed={pressed}");
            }
            wl_pointer::Event::Axis { axis, value, .. } => {
                println!("Pointer axis: {axis:?} value={value}");
            }
            wl_pointer::Event::Frame => {}
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                println!("Key: {key} pressed={pressed}");
                // 'q' is keycode 16 in evdev
                if key == 16 && pressed {
                    println!("Quit");
                    state.running = false;
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed, ..
            } => {
                println!("Modifiers: {mods_depressed:#x}");
            }
            wl_keyboard::Event::Keymap { .. } => {
                println!("Keymap received");
            }
            _ => {}
        }
    }
}
