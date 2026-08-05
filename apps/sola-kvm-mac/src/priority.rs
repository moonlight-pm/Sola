//! Best-effort CPU / QoS / power wake for the ember inject agent.
//!
//! Cold enter after the Mac has been idle is a common hitch source: display
//! idle, App Nap, and power-management clocks need a kick before CGEvent
//! inject feels snappy. Input Leap’s secondary `enter()` does the same
//! dance — wake `IODisplayWrangler` + `IOPMAssertionDeclareUserActivity`.
//!
//! Session-scoped assertions are taken on Enter and released on Leave so we
//! don’t pin the Mac awake while the pointer is on Linux.

use tracing::{debug, info, warn};

/// Raise process / thread priority so motion inject is not delayed by App Nap
/// or background QoS under desktop load.
pub fn boost_process() {
    // Niceness (works when launchd `Nice` is set, or if the user can renice).
    match set_nice(-10) {
        Ok(n) => info!(nice = n, "process niceness raised for KVM inject"),
        Err(e) => {
            debug!(%e, "setpriority(-10) failed");
            let _ = set_nice(-5);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Err(e) = set_user_interactive_qos() {
            debug!(%e, "pthread QoS USER_INTERACTIVE failed");
        } else {
            info!("thread QoS = USER_INTERACTIVE");
        }
        // Process-lifetime latency activity (App Nap). Session power wake is
        // separate — see on_enter_remote / on_leave_remote.
        if let Err(e) = begin_process_latency_activity() {
            debug!(%e, "NSProcessInfo latency activity failed (non-fatal)");
        }
    }

    let nice = get_nice();
    if nice >= 0 {
        warn!(
            nice,
            "sola-kvm-mac at default niceness — launchd Nice=-10 should apply on next agent reload"
        );
    }
}

/// Call on KVM Enter (crossing onto the Mac). Idempotent for re-Enter;
/// always re-wakes display / user activity so a long gap still warms up.
pub fn on_enter_remote() {
    #[cfg(target_os = "macos")]
    mac::on_enter_remote();
    #[cfg(not(target_os = "macos"))]
    debug!("on_enter_remote (stub)");
}

/// Call on KVM Leave. Idempotent (Leave is sprayed 3×).
pub fn on_leave_remote() {
    #[cfg(target_os = "macos")]
    mac::on_leave_remote();
    #[cfg(not(target_os = "macos"))]
    debug!("on_leave_remote (stub)");
}

fn set_nice(value: i32) -> std::io::Result<i32> {
    let rc = unsafe { libc_setpriority(value) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(get_nice())
}

fn get_nice() -> i32 {
    unsafe { libc_getpriority() }
}

// Avoid a libc crate dep on the Mac package: thin wrappers.
unsafe fn libc_setpriority(value: i32) -> i32 {
    #[link(name = "c")]
    extern "C" {
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    }
    // PRIO_PROCESS = 0
    unsafe { setpriority(0, 0, value) }
}

unsafe fn libc_getpriority() -> i32 {
    #[link(name = "c")]
    extern "C" {
        fn getpriority(which: i32, who: u32) -> i32;
    }
    unsafe { getpriority(0, 0) }
}

#[cfg(target_os = "macos")]
fn set_user_interactive_qos() -> std::io::Result<()> {
    // qos_class_t: QOS_CLASS_USER_INTERACTIVE = 0x21
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    #[link(name = "pthread")]
    #[link(name = "System")]
    extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    let rc = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if rc != 0 {
        Err(std::io::Error::from_raw_os_error(rc))
    } else {
        Ok(())
    }
}

/// Process-lifetime App Nap opt-out via NSProcessInfo.
///
/// Uses the ObjC runtime (no objc crate) so we stay a single-file binary.
#[cfg(target_os = "macos")]
fn begin_process_latency_activity() -> Result<(), String> {
    use std::sync::atomic::{AtomicU32, Ordering};

    // NSActivityUserInitiatedAllowingIdleSystemSleep = 0x00FFFFFFULL
    // NSActivityLatencyCritical                      = 0xFF00000000ULL
    const OPTIONS: u64 = 0x00FF_FFFF | 0xFF00_0000_00;
    // Hold forever — we intentionally never end this activity while the
    // agent process lives. Session power assertions are separate.
    static STARTED: AtomicU32 = AtomicU32::new(0);
    if STARTED.swap(1, Ordering::Relaxed) != 0 {
        return Ok(());
    }

    unsafe {
        #[link(name = "objc")]
        extern "C" {
            fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
            fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
            fn objc_msgSend();
        }

        // objc_msgSend is varargs; cast per call site.
        type MsgSend0 =
            unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void) -> *mut std::ffi::c_void;
        type MsgSend1 = unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const std::ffi::c_void,
            u64,
            *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        type MsgSendStr = unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const std::ffi::c_void,
            *const i8,
        ) -> *mut std::ffi::c_void;

        let msg0: MsgSend0 = std::mem::transmute(objc_msgSend as *const ());
        let msg1: MsgSend1 = std::mem::transmute(objc_msgSend as *const ());
        let msg_str: MsgSendStr = std::mem::transmute(objc_msgSend as *const ());

        let cls = objc_getClass(c"NSProcessInfo".as_ptr() as *const i8);
        if cls.is_null() {
            return Err("NSProcessInfo class missing".into());
        }
        let sel_pi = sel_registerName(c"processInfo".as_ptr() as *const i8);
        let pi = msg0(cls, sel_pi);
        if pi.is_null() {
            return Err("+[NSProcessInfo processInfo] returned nil".into());
        }

        let nsstring = objc_getClass(c"NSString".as_ptr() as *const i8);
        let sel_utf8 = sel_registerName(c"stringWithUTF8String:".as_ptr() as *const i8);
        let reason = msg_str(
            nsstring,
            sel_utf8,
            c"sola-kvm-mac inject agent".as_ptr() as *const i8,
        );
        if reason.is_null() {
            return Err("NSString reason alloc failed".into());
        }

        let sel_begin =
            sel_registerName(c"beginActivityWithOptions:reason:".as_ptr() as *const i8);
        let token = msg1(pi, sel_begin, OPTIONS, reason);
        if token.is_null() {
            return Err("beginActivityWithOptions:reason: returned nil".into());
        }
        // Intentionally retain for process lifetime (never endActivity:).
        let sel_retain = sel_registerName(c"retain".as_ptr() as *const i8);
        let _ = msg0(token, sel_retain);
        info!("NSProcessInfo latency-critical activity started (process lifetime)");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use std::ffi::{c_void, CStr};
    use std::sync::atomic::{AtomicU32, Ordering};

    type CFStringRef = *const c_void;
    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type IOReturn = i32;
    type IOPMAssertionID = u32;
    type IOPMUserActiveType = u32;
    type io_object_t = u32;
    type io_registry_entry_t = u32;
    type mach_port_t = u32;
    type kern_return_t = i32;
    type CFStringEncoding = u32;

    const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
    const K_IOPM_USER_ACTIVE_LOCAL: IOPMUserActiveType = 0;
    /// kIOPMAssertionLevelOn
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    /// kIOReturnSuccess
    const K_IO_RETURN_SUCCESS: IOReturn = 0;

    // 0 means “no assertion held”.
    static DISPLAY_ASSERT: AtomicU32 = AtomicU32::new(0);
    static SYSTEM_ASSERT: AtomicU32 = AtomicU32::new(0);
    static REMOTE_ACTIVE: AtomicU32 = AtomicU32::new(0);

    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "IOKit", kind = "framework")]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFAllocatorRef;

        fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const i8,
            encoding: CFStringEncoding,
        ) -> CFStringRef;
        fn CFRelease(cf: CFTypeRef);

        fn IOPMAssertionDeclareUserActivity(
            reason: CFStringRef,
            user_type: IOPMUserActiveType,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;

        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;

        fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;

        // macOS 12+ renamed kIOMasterPortDefault → kIOMainPortDefault; both
        // are 0 (MACH_PORT_NULL) meaning “use the default master port”.
        fn IORegistryEntryFromPath(
            master_port: mach_port_t,
            path: *const i8,
        ) -> io_registry_entry_t;
        fn IORegistryEntrySetCFProperty(
            entry: io_registry_entry_t,
            property_name: CFStringRef,
            property: CFTypeRef,
        ) -> kern_return_t;
        fn IOObjectRelease(object: io_object_t) -> kern_return_t;

        static kCFBooleanFalse: CFTypeRef;

        fn CGSetLocalEventsSuppressionInterval(seconds: f64);
    }

    fn cfstr(s: &CStr) -> CFStringRef {
        unsafe {
            CFStringCreateWithCString(
                kCFAllocatorDefault,
                s.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            )
        }
    }

    fn release_cf(r: CFStringRef) {
        if !r.is_null() {
            unsafe { CFRelease(r as CFTypeRef) }
        }
    }

    /// Wake the display pipeline after idle (Input Leap enter path).
    fn wake_display() {
        unsafe {
            let entry = IORegistryEntryFromPath(
                0, // kIOMainPortDefault
                c"IOService:/IOResources/IODisplayWrangler".as_ptr() as *const i8,
            );
            if entry == 0 {
                debug!("IODisplayWrangler path missing — skip display wake");
                return;
            }
            let key = cfstr(c"IORequestIdle");
            if key.is_null() {
                let _ = IOObjectRelease(entry);
                return;
            }
            let kr = IORegistryEntrySetCFProperty(entry, key, kCFBooleanFalse);
            release_cf(key);
            let _ = IOObjectRelease(entry);
            if kr != 0 {
                debug!(kr, "IORequestIdle=false failed");
            } else {
                debug!("display wake: IORequestIdle=false");
            }
        }
    }

    /// Announce local user activity so idle timers / App Nap reverse course.
    fn declare_user_activity() {
        unsafe {
            let reason = cfstr(c"sola-kvm-mac entering remote session");
            if reason.is_null() {
                return;
            }
            let mut id: IOPMAssertionID = 0;
            let rc =
                IOPMAssertionDeclareUserActivity(reason, K_IOPM_USER_ACTIVE_LOCAL, &mut id);
            release_cf(reason);
            if rc != K_IO_RETURN_SUCCESS {
                debug!(rc, "IOPMAssertionDeclareUserActivity failed");
            } else {
                debug!(assertion_id = id, "IOPM user activity declared");
            }
        }
    }

    fn create_named_assertion(type_name: &CStr, name: &CStr) -> Option<IOPMAssertionID> {
        unsafe {
            let ty = cfstr(type_name);
            let nm = cfstr(name);
            if ty.is_null() || nm.is_null() {
                release_cf(ty);
                release_cf(nm);
                return None;
            }
            let mut id: IOPMAssertionID = 0;
            let rc = IOPMAssertionCreateWithName(ty, K_IOPM_ASSERTION_LEVEL_ON, nm, &mut id);
            release_cf(ty);
            release_cf(nm);
            if rc != K_IO_RETURN_SUCCESS {
                debug!(rc, "IOPMAssertionCreateWithName failed");
                None
            } else {
                Some(id)
            }
        }
    }

    fn release_assertion(slot: &AtomicU32, label: &'static str) {
        let id = slot.swap(0, Ordering::Relaxed);
        if id == 0 {
            return;
        }
        unsafe {
            let rc = IOPMAssertionRelease(id);
            if rc != K_IO_RETURN_SUCCESS {
                debug!(rc, id, label, "IOPMAssertionRelease failed");
            } else {
                debug!(id, label, "IOPM assertion released");
            }
        }
    }

    fn take_session_assertions() {
        // Prevent display sleep + user-idle system sleep while pointer is on Mac.
        if DISPLAY_ASSERT.load(Ordering::Relaxed) == 0 {
            if let Some(id) = create_named_assertion(
                c"PreventUserIdleDisplaySleep",
                c"sola-kvm-mac remote session (display)",
            ) {
                DISPLAY_ASSERT.store(id, Ordering::Relaxed);
                info!(id, "IOPM PreventUserIdleDisplaySleep held for remote session");
            }
        }
        if SYSTEM_ASSERT.load(Ordering::Relaxed) == 0 {
            if let Some(id) = create_named_assertion(
                c"PreventUserIdleSystemSleep",
                c"sola-kvm-mac remote session (system)",
            ) {
                SYSTEM_ASSERT.store(id, Ordering::Relaxed);
                info!(id, "IOPM PreventUserIdleSystemSleep held for remote session");
            }
        }
    }

    fn release_session_assertions() {
        release_assertion(&DISPLAY_ASSERT, "display");
        release_assertion(&SYSTEM_ASSERT, "system");
    }

    fn zero_suppression_interval() {
        // Input Leap setZeroSuppressionInterval — drop post-warp event mute.
        unsafe {
            CGSetLocalEventsSuppressionInterval(0.0);
        }
        debug!("CGSetLocalEventsSuppressionInterval(0)");
    }

    pub fn on_enter_remote() {
        // Idempotent for session assertions; always re-wake on every Enter so
        // a long gap after idle still warms the pipeline.
        let was = REMOTE_ACTIVE.swap(1, Ordering::Relaxed);
        wake_display();
        declare_user_activity();
        zero_suppression_interval();
        // Re-apply QoS in case the inject thread was created after boost_process.
        if let Err(e) = super::set_user_interactive_qos() {
            debug!(%e, "re-apply USER_INTERACTIVE QoS on enter failed");
        }
        if was == 0 {
            take_session_assertions();
            info!("remote enter: display wake + IOPM activity + session assertions");
        } else {
            debug!("remote enter (already active): re-woke display + user activity");
        }
    }

    pub fn on_leave_remote() {
        if REMOTE_ACTIVE.swap(0, Ordering::Relaxed) == 0 {
            // Leave spray after already-left: no-op.
            return;
        }
        release_session_assertions();
        info!("remote leave: IOPM session assertions released");
    }
}
