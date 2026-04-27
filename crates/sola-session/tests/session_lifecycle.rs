use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_bus::BusClient;
use sola_bus::topics::{LaunchAppPayload, Topic, TopicKind};

/// Locate a binary in the same target directory as the current test executable.
fn target_bin(name: &str) -> PathBuf {
    // The test binary lives at <target>/<profile>/deps/<test-name>-<hash>.
    // The built binaries live at <target>/<profile>/<name>.
    let mut dir = std::env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("parent (deps)")
        .parent()
        .expect("parent (profile)")
        .to_path_buf();
    dir.push(name);
    dir
}

fn start_bus(path: PathBuf) -> std::process::Child {
    let exe = target_bin("sola-bus");
    std::process::Command::new(&exe)
        .env("SOLA_BUS_PATH", &path)
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn bus ({exe:?}): {e}"))
}

fn start_session(bus_path: PathBuf) -> std::process::Child {
    let exe = target_bin("sola-session");
    std::process::Command::new(&exe)
        .env("SOLA_BUS_PATH", &bus_path)
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn sola-session ({exe:?}): {e}"))
}

fn wait_for_socket(path: &PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("bus socket never appeared");
}

/// We rely on `systemd-run --user --scope`. Without a working user
/// systemd manager (e.g. inside a minimal CI sandbox) the launch path
/// can't run at all, so skip rather than fail.
fn user_systemd_available() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success() || s.code() == Some(1)) // "degraded" returns 1 but is fine
        .unwrap_or(false)
}

fn pid_alive(pid: i32) -> bool {
    // kill(0) just probes whether the pid exists & is signalable by us.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[test]
fn spawn_close_kills_double_forked_grandchild() {
    if !user_systemd_available() {
        eprintln!("skipping: user systemd not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let bus_path = dir.path().join("bus");
    let pid_file = dir.path().join("grandchild.pid");
    let script_path = dir.path().join("double-fork.sh");

    // Script double-forks a sleep so the grandchild ends up reparented
    // to PID 1 in a fresh session — exactly the pattern Wine and Steam
    // use, and exactly what pdeathsig can't reach. The cgroup *can*.
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\n\
             setsid sh -c 'sleep 600 & echo $! > {pid}' &\n\
             # Wait for the inner sh to exit (it forks and detaches).\n\
             wait\n\
             # Keep the parent alive too, so we have a process to stop.\n\
             sleep 600\n",
            pid = pid_file.display(),
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    let mut bus = start_bus(bus_path.clone());
    wait_for_socket(&bus_path);
    let mut session = start_session(bus_path.clone());

    let socket = bus_path.to_string_lossy().to_string();
    let mut client = BusClient::new();
    client.set_app_id("test");
    client.connect_to(&socket).unwrap();
    client
        .subscribe(&[
            TopicKind::LaunchResult,
            TopicKind::UserAppExited,
            TopicKind::ClientConnected,
        ])
        .unwrap();

    // Wait for sola-session to be rostered.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut session_ready = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::ClientConnected(a)) = Topic::parse(&m) {
                if a == "sola-session" {
                    session_ready = true;
                    break;
                }
            }
        }
    }
    assert!(session_ready, "sola-session never identified on the bus");

    let app_id = "test-double-fork";
    client
        .emit(Topic::LaunchApp(LaunchAppPayload {
            app_id: app_id.into(),
            command: script_path.to_string_lossy().into_owned(),
        }))
        .unwrap();

    // LaunchResult ok.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut launched = false;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(100)) {
            if let Some(Topic::LaunchResult(p)) = Topic::parse(&m) {
                assert!(p.ok, "LaunchResult not ok: {:?}", p.error);
                launched = true;
                break;
            }
        }
    }
    assert!(launched, "never saw LaunchResult");

    // Wait for the grandchild PID to land in the file.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut grandchild_pid: Option<i32> = None;
    while Instant::now() < deadline {
        if let Ok(s) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = s.trim().parse::<i32>() {
                grandchild_pid = Some(pid);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let grandchild_pid = grandchild_pid.expect("grandchild pidfile never appeared");
    assert!(
        pid_alive(grandchild_pid),
        "grandchild pid {grandchild_pid} not alive immediately after launch"
    );

    // Close the app — first call schedules systemctl stop --no-block,
    // second call should be a no-op (already closing). A no-op means
    // exactly one UserAppExited eventually fires.
    client.emit(Topic::CloseApp(app_id.into())).unwrap();
    client.emit(Topic::CloseApp(app_id.into())).unwrap();

    // UserAppExited should fire within ~6s (5s TimeoutStopSec + slack).
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut exited_count = 0;
    while Instant::now() < deadline {
        if let Some(m) = client.recv_timeout(Duration::from_millis(200)) {
            if matches!(Topic::parse(&m), Some(Topic::UserAppExited(_))) {
                exited_count += 1;
                // Don't break — keep draining a moment longer to confirm
                // the second CloseApp didn't double-fire.
                let extra_deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < extra_deadline {
                    if let Some(m2) = client.recv_timeout(Duration::from_millis(100)) {
                        if matches!(Topic::parse(&m2), Some(Topic::UserAppExited(_))) {
                            exited_count += 1;
                        }
                    }
                }
                break;
            }
        }
    }
    assert_eq!(
        exited_count, 1,
        "expected exactly one UserAppExited (second CloseApp must be a no-op)"
    );

    // The grandchild — re-`setsid`'d, reparented to init — must be dead
    // because the cgroup was torn down. This is the whole point.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && pid_alive(grandchild_pid) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !pid_alive(grandchild_pid),
        "grandchild pid {grandchild_pid} survived the scope teardown — cgroup did not contain it"
    );

    let _ = session.kill();
    let _ = bus.kill();
}
