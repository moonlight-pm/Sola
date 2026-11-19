//! Waiting for a pairing: unpaired, the daemon waits for `sola-kvm pair` to
//! leave a key in its folder (inotify) rather than exit and be restarted,
//! so a machine paired after it started needs no restart.

use std::io;
use std::path::Path;

/// Return once `ready()` holds, looking again whenever something changes
/// in `dir` or its `viewers` folder. `dir` is made if it is missing.
pub fn until(dir: &Path, ready: impl Fn() -> bool) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let _close = Close(fd);
    let mask = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE | libc::IN_DELETE;
    watch(fd, dir, mask)?;
    let viewers = dir.join("viewers");
    let mut watching_viewers = false;
    let mut buf = [0u8; 4096];
    loop {
        // Watched before looking, so a key that lands in between is seen.
        if !watching_viewers && viewers.is_dir() {
            watching_viewers = watch(fd, &viewers, mask).is_ok();
        }
        if ready() {
            return Ok(());
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

fn watch(fd: i32, dir: &Path, mask: u32) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path with a NUL"))?;
    if unsafe { libc::inotify_add_watch(fd, path.as_ptr(), mask) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct Close(i32);

impl Drop for Close {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_key_saved_later_ends_the_wait() {
        let dir = std::env::temp_dir().join(format!("sola-kvm-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let waiting = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                until(&dir, || !crate::link::viewer_keys(&dir).unwrap_or_default().is_empty()).unwrap()
            })
        };
        // Not an ordering: the wait must end however late the key comes.
        std::thread::sleep(Duration::from_millis(50));
        crate::link::save_viewer(&dir, "novus", &[1; 32]).unwrap();
        waiting.join().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
