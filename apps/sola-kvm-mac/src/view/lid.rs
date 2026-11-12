//! The laptop's lid: whether it is closed (the Mac's own screen off), and
//! word when it opens or closes (IOKit's clamshell message from the power
//! root domain), so remote view can say whether there is a screen to mirror.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, NonNull};

use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2_core_foundation::{CFRetained, CFString, CFType, kCFBooleanTrue};
use tracing::warn;

type IoObject = u32;
type NotificationPort = *mut c_void;
type Changed = Box<dyn Fn() + Send + Sync>;

/// `kIOPMMessageClamshellStateChange`.
const CLAMSHELL_CHANGED: u32 = 0xE003_4100;
const ROOT_DOMAIN: &CStr = c"IOPower:/IOPowerConnection/IOPMrootDomain";

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegistryEntryFromPath(main_port: u32, path: *const c_char) -> IoObject;
    fn IORegistryEntryCreateCFProperty(entry: IoObject, key: &CFString, allocator: *const c_void, options: u32)
    -> *mut CFType;
    fn IONotificationPortCreate(main_port: u32) -> NotificationPort;
    fn IONotificationPortSetDispatchQueue(port: NotificationPort, queue: &DispatchQueue);
    fn IOServiceAddInterestNotification(
        port: NotificationPort,
        service: IoObject,
        interest: *const c_char,
        callback: unsafe extern "C" fn(*mut c_void, IoObject, u32, *mut c_void),
        refcon: *mut c_void,
        notification: *mut IoObject,
    ) -> i32;
    fn IOObjectRelease(object: IoObject) -> i32;
}

fn root_domain() -> Option<IoObject> {
    let root = unsafe { IORegistryEntryFromPath(0, ROOT_DOMAIN.as_ptr()) };
    (root != 0).then_some(root)
}

/// The lid is closed; false on a Mac with none.
pub fn closed() -> bool {
    let Some(root) = root_domain() else {
        return false;
    };
    let key = CFString::from_static_str("AppleClamshellState");
    let value = unsafe { IORegistryEntryCreateCFProperty(root, &key, ptr::null(), 0) };
    unsafe { IOObjectRelease(root) };
    let Some(value) = NonNull::new(value) else {
        return false;
    };
    let value = unsafe { CFRetained::from_raw(value) };
    let yes = unsafe { kCFBooleanTrue }.map(|t| t as *const _ as *const c_void);
    yes == Some(&*value as *const CFType as *const c_void)
}

/// Call `changed` whenever the lid opens or closes, as long as the agent
/// runs.
pub fn watch(changed: impl Fn() + Send + Sync + 'static) {
    let Some(root) = root_domain() else {
        return;
    };
    let port = unsafe { IONotificationPortCreate(0) };
    if port.is_null() {
        warn!("remote view: the lid cannot be watched");
        return;
    }
    let queue = DispatchQueue::new("sola.kvm.lid", DispatchQueueAttr::SERIAL);
    unsafe { IONotificationPortSetDispatchQueue(port, &queue) };
    let refcon = Box::into_raw(Box::new(Box::new(changed) as Changed)).cast();
    let mut notifier = 0;
    let status = unsafe {
        IOServiceAddInterestNotification(port, root, c"IOGeneralInterest".as_ptr(), on_power, refcon, &mut notifier)
    };
    if status != 0 {
        warn!(status, "remote view: the lid cannot be watched");
    }
    // The port, its queue and the callback live as long as the agent.
    std::mem::forget(queue);
}

unsafe extern "C" fn on_power(refcon: *mut c_void, _service: IoObject, message: u32, _argument: *mut c_void) {
    if message == CLAMSHELL_CHANGED {
        let changed = unsafe { &*refcon.cast::<Changed>() };
        changed();
    }
}
