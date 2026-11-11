//! A display the viewer's size, made with CoreGraphics' private
//! `CGVirtualDisplay` (as Sidecar, DeskPad and BetterDisplay make theirs),
//! and the main one while it lives: the menu bar and new windows go there.
//! Dropped, macOS takes it away, moves its windows back to the laptop's
//! screen and makes that main again; the arrangement is made for this
//! process only, so a crash undoes it too.

use std::ffi::CStr;
use std::ptr;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::CGSize;
use objc2_core_graphics::{
    CGBeginDisplayConfiguration, CGCompleteDisplayConfiguration, CGConfigureDisplayOrigin, CGConfigureOption,
    CGDirectDisplayID, CGDisplayBounds, CGError, CGGetOnlineDisplayList,
};
use objc2_foundation::{NSArray, NSString};

use super::shape::{MAX_PIXELS, Shape};

/// How long macOS may take to put a display at a new size. It tells no one
/// in this process, so this looks every [`LOOK`]; nothing is ordered by it.
const SETTLE: Duration = Duration::from_secs(2);
const LOOK: Duration = Duration::from_millis(20);
/// Points per inch, for the size the display says it is.
const PPI: f64 = 110.0;
const VENDOR: u32 = 0x5301;

/// Displays this process made and let go. macOS took them away, but this
/// process's own display list keeps them.
static GONE: Mutex<Vec<CGDirectDisplayID>> = Mutex::new(Vec::new());

pub fn gone(id: CGDirectDisplayID) -> bool {
    GONE.lock().unwrap_or_else(|e| e.into_inner()).contains(&id)
}

pub struct Virtual {
    display: Retained<AnyObject>,
    _queue: DispatchRetained<DispatchQueue>,
    id: CGDirectDisplayID,
    shape: Option<Shape>,
}

fn class(name: &CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("this macOS has no {}", name.to_string_lossy()))
}

impl Virtual {
    /// A display for `shape`, not yet at it.
    pub fn new(shape: Shape) -> Result<Self, String> {
        let mm = |points: u32| f64::from(points) / PPI * 25.4;
        autoreleasepool(|_| unsafe {
            let queue = DispatchQueue::new("sola.kvm.display", DispatchQueueAttr::SERIAL);
            let descriptor: Retained<AnyObject> = msg_send![class(c"CGVirtualDisplayDescriptor")?, new];
            let name = NSString::from_str("Sola Remote Desktop");
            let _: () = msg_send![&descriptor, setDispatchQueue: &*queue];
            let _: () = msg_send![&descriptor, setName: &*name];
            let _: () = msg_send![&descriptor, setMaxPixelsWide: MAX_PIXELS.0];
            let _: () = msg_send![&descriptor, setMaxPixelsHigh: MAX_PIXELS.1];
            let size = CGSize::new(mm(shape.points.0), mm(shape.points.1));
            let _: () = msg_send![&descriptor, setSizeInMillimeters: size];
            let _: () = msg_send![&descriptor, setVendorID: VENDOR];
            let _: () = msg_send![&descriptor, setProductID: VENDOR];
            let _: () = msg_send![&descriptor, setSerialNum: 1u32];
            let made: Allocated<AnyObject> = msg_send![class(c"CGVirtualDisplay")?, alloc];
            let display: Option<Retained<AnyObject>> = msg_send![made, initWithDescriptor: &*descriptor];
            let display = display.ok_or_else(|| "macOS would not make a display".to_string())?;
            let id: CGDirectDisplayID = msg_send![&display, displayID];
            Ok(Self {
                display,
                _queue: queue,
                id,
                shape: None,
            })
        })
    }

    pub fn id(&self) -> CGDirectDisplayID {
        self.id
    }

    /// Put it at `shape`, and make it the main display.
    pub fn fit(&mut self, shape: Shape) -> Result<(), String> {
        if self.shape != Some(shape) {
            self.apply(shape)?;
            self.shape = Some(shape);
            self.settle(shape)?;
        }
        self.make_main(shape)
    }

    fn apply(&self, shape: Shape) -> Result<(), String> {
        autoreleasepool(|_| unsafe {
            let settings: Retained<AnyObject> = msg_send![class(c"CGVirtualDisplaySettings")?, new];
            let _: () = msg_send![&settings, setHiDPI: u32::from(shape.scale > 1)];
            let mode: Allocated<AnyObject> = msg_send![class(c"CGVirtualDisplayMode")?, alloc];
            let (width, height) = (shape.points.0 as usize, shape.points.1 as usize);
            let mode: Option<Retained<AnyObject>> =
                msg_send![mode, initWithWidth: width, height: height, refreshRate: 60.0f64];
            let mode = mode.ok_or_else(|| "macOS would not make a display mode".to_string())?;
            let modes = NSArray::from_retained_slice(&[mode]);
            let _: () = msg_send![&settings, setModes: &*modes];
            let applied: bool = msg_send![&self.display, applySettings: &*settings];
            if applied { Ok(()) } else { Err(format!("macOS would not put the display at {shape:?}")) }
        })
    }

    /// Until macOS has it online at `shape`.
    fn settle(&self, shape: Shape) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let size = CGDisplayBounds(self.id).size;
            if online().contains(&self.id) && (size.width, size.height) == (f64::from(shape.points.0), f64::from(shape.points.1)) {
                return Ok(());
            }
            if started.elapsed() > SETTLE {
                return Err(format!("macOS did not bring the display online at {shape:?}"));
            }
            thread::sleep(LOOK);
        }
    }

    /// At (0, 0), which makes it main, with the other displays moved right
    /// as they were arranged among themselves.
    fn make_main(&self, shape: Shape) -> Result<(), String> {
        let others: Vec<_> = online().into_iter().filter(|&d| d != self.id && !gone(d)).collect();
        let left = others.iter().map(|&d| CGDisplayBounds(d).origin.x).fold(f64::INFINITY, f64::min);
        let shift = if left.is_finite() { f64::from(shape.points.0) - left } else { 0.0 };
        let mut config = ptr::null_mut();
        unsafe {
            check(CGBeginDisplayConfiguration(&mut config), "begin")?;
            let mut status = CGConfigureDisplayOrigin(config, self.id, 0, 0);
            for &other in &others {
                let origin = CGDisplayBounds(other).origin;
                if status == CGError::Success {
                    status = CGConfigureDisplayOrigin(config, other, (origin.x + shift) as i32, origin.y as i32);
                }
            }
            check(status, "arrange")?;
            check(CGCompleteDisplayConfiguration(config, CGConfigureOption::ForAppOnly), "complete")
        }
    }
}

impl Drop for Virtual {
    fn drop(&mut self) {
        GONE.lock().unwrap_or_else(|e| e.into_inner()).push(self.id);
    }
}

fn check(status: CGError, what: &str) -> Result<(), String> {
    if status == CGError::Success {
        Ok(())
    } else {
        Err(format!("macOS would not {what} the arrangement (status {})", status.0))
    }
}

/// This process's view of the displays online.
fn online() -> Vec<CGDirectDisplayID> {
    let mut ids = [0; 16];
    let mut count = 0;
    let status = unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if status != CGError::Success {
        return Vec::new();
    }
    ids[..count as usize].to_vec()
}
