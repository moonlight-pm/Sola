/// Minimal X11 test client for Sola's XWayland integration.
///
/// Draws a red window and prints all pointer/keyboard events to stdout.
/// Used to verify that XWayland input forwarding works before testing
/// complex apps like Steam.
///
/// Usage: DISPLAY=:0 sola-xtest
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    let win_id = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win_id,
        screen.root,
        100, 100,   // x, y
        400, 300,   // width, height
        2,          // border width
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new()
            .background_pixel(0xFF0000) // red
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::POINTER_MOTION
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::ENTER_WINDOW
                    | EventMask::LEAVE_WINDOW
                    | EventMask::KEY_PRESS,
            ),
    )?;

    conn.change_property8(
        PropMode::REPLACE,
        win_id,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"X11 Test",
    )?;

    conn.map_window(win_id)?;
    conn.flush()?;

    println!("sola-xtest: window opened, waiting for events...");
    println!("Move mouse over window, click, press keys. Press 'q' to quit.");

    loop {
        let event = conn.wait_for_event()?;
        match event {
            Event::Expose(_) => {
                println!("Expose");
            }
            Event::MotionNotify(ev) => {
                println!("Motion: ({}, {})", ev.event_x, ev.event_y);
            }
            Event::ButtonPress(ev) => {
                println!("ButtonPress: button={} at ({}, {})", ev.detail, ev.event_x, ev.event_y);
            }
            Event::ButtonRelease(ev) => {
                println!("ButtonRelease: button={} at ({}, {})", ev.detail, ev.event_x, ev.event_y);
            }
            Event::EnterNotify(ev) => {
                println!("Enter: ({}, {})", ev.event_x, ev.event_y);
            }
            Event::LeaveNotify(ev) => {
                println!("Leave: ({}, {})", ev.event_x, ev.event_y);
            }
            Event::KeyPress(ev) => {
                println!("KeyPress: keycode={}", ev.detail);
                // 'q' is typically keycode 24
                if ev.detail == 24 {
                    println!("Quit");
                    break;
                }
            }
            _ => {}
        }
    }

    conn.destroy_window(win_id)?;
    conn.flush()?;
    Ok(())
}
