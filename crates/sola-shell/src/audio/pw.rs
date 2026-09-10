//! PipeWire graph + WirePlumber `wpctl` helpers.
//! Parsing is unit-tested; the worker never runs on the iced thread.
//!
//! The device list comes from `pw-cli ls Node` (object properties). A full
//! `pw-dump` enumerates SPA params on every node and can stall forever when
//! the session manager is wedged — that used to hide the menubar chip.
//! Helpers are time-bounded; partial stdout still counts as a graph.

use super::{Device, Kind, Snapshot};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Bound for `pw-cli` / `wpctl`. Control-plane stalls (hung WirePlumber)
/// must not freeze the picker or hide the chip.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
/// After a `wpctl` timeout, skip further inspect/get-volume polls so a
/// wedged session manager does not add 8s to every 1s refresh.
const WPCTL_COOL: Duration = Duration::from_secs(20);

static WPCTL_UNREACHABLE_SINCE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn wpctl_slot() -> &'static Mutex<Option<Instant>> {
    WPCTL_UNREACHABLE_SINCE.get_or_init(|| Mutex::new(None))
}

fn wpctl_cooling() -> bool {
    let Ok(g) = wpctl_slot().lock() else {
        return false;
    };
    g.is_some_and(|t| t.elapsed() < WPCTL_COOL)
}

fn note_wpctl_timeout() {
    if let Ok(mut g) = wpctl_slot().lock() {
        *g = Some(Instant::now());
    }
}

fn note_wpctl_ok() {
    if let Ok(mut g) = wpctl_slot().lock() {
        *g = None;
    }
}

pub fn snapshot() -> Result<Snapshot, String> {
    let (sinks, sources) = match load_graph() {
        Ok(v) => v,
        Err(e) => {
            if pipewire_socket_present() {
                tracing::warn!("audio graph: {e} (pipewire socket present)");
                (Vec::new(), Vec::new())
            } else {
                return Err(e);
            }
        }
    };
    let mut snap = Snapshot {
        available: true,
        sinks,
        sources,
        ..Snapshot::default()
    };
    if wpctl_cooling() {
        return Ok(snap);
    }
    snap.default_sink = inspect_id("@DEFAULT_AUDIO_SINK@");
    snap.default_source = inspect_id("@DEFAULT_AUDIO_SOURCE@");
    if let Some(id) = snap.default_sink {
        if let Some((v, m)) = get_volume(id) {
            snap.sink_volume = v;
            snap.sink_mute = m;
        }
    }
    if let Some(id) = snap.default_source {
        if let Some((v, m)) = get_volume(id) {
            snap.source_volume = v;
            snap.source_mute = m;
        }
    }
    Ok(snap)
}

fn load_graph() -> Result<(Vec<Device>, Vec<Device>), String> {
    match run(&["pw-cli", "ls", "Node"]) {
        Ok(text) if !text.trim().is_empty() => return Ok(parse_cli_nodes(&text)),
        Ok(_) => tracing::warn!("audio pw-cli ls Node: empty"),
        Err(e) => tracing::warn!("audio pw-cli ls: {e}"),
    }
    // Last resort. Full dump enumerates SPA params and often never exits
    // when the session manager is wedged — the 2s timeout still applies.
    let dump = run(&["pw-dump"])?;
    match parse_nodes(&dump) {
        Ok(v) => Ok(v),
        Err(e) => {
            tracing::warn!("audio pw-dump parse: {e}");
            Ok((Vec::new(), Vec::new()))
        }
    }
}

pub fn pipewire_socket_present() -> bool {
    for key in ["PIPEWIRE_RUNTIME_DIR", "XDG_RUNTIME_DIR"] {
        let Some(dir) = std::env::var_os(key) else {
            continue;
        };
        let path = PathBuf::from(dir).join("pipewire-0");
        if path.exists() {
            return true;
        }
    }
    false
}

pub fn set_volume(id: u32, volume: f32) -> bool {
    let pct = ((volume.clamp(0.0, 1.0)) * 100.0).round();
    log_cmd(
        run_wpctl(&[
            "wpctl",
            "set-volume",
            &id.to_string(),
            &format!("{pct:.0}%"),
            "-l",
            "1.0",
        ]),
        "set-volume",
        id,
    )
}

pub fn set_mute(id: u32, mute: bool) -> bool {
    let v = if mute { "1" } else { "0" };
    log_cmd(
        run_wpctl(&["wpctl", "set-mute", &id.to_string(), v]),
        "set-mute",
        id,
    )
}

pub fn set_default(id: u32) -> bool {
    log_cmd(
        run_wpctl(&["wpctl", "set-default", &id.to_string()]),
        "set-default",
        id,
    )
}

fn log_cmd(result: Result<String, String>, op: &str, id: u32) -> bool {
    match result {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("wpctl {op} {id}: {e}");
            false
        }
    }
}

fn inspect_id(spec: &str) -> Option<u32> {
    let out = run_wpctl(&["wpctl", "inspect", spec]).ok()?;
    parse_inspect_id(&out)
}

fn get_volume(id: u32) -> Option<(f32, bool)> {
    let out = run_wpctl(&["wpctl", "get-volume", &id.to_string()]).ok()?;
    parse_get_volume(&out)
}

fn run_wpctl(cmd: &[&str]) -> Result<String, String> {
    match run(cmd) {
        Ok(s) => {
            note_wpctl_ok();
            Ok(s)
        }
        Err(e) if e.contains("timed out") => {
            note_wpctl_timeout();
            Err(e)
        }
        Err(e) => Err(e),
    }
}

fn run(cmd: &[&str]) -> Result<String, String> {
    run_with_timeout(cmd, HELPER_TIMEOUT)
}

fn run_with_timeout(cmd: &[&str], timeout: Duration) -> Result<String, String> {
    let (bin, args) = cmd
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: `set_pdeathsig_sigterm` is the documented pre_exec hook.
    unsafe {
        command.pre_exec(sola_core::process::set_pdeathsig_sigterm);
    }
    let mut child = command.spawn().map_err(|e| format!("{bin}: {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{bin}: no stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{bin}: no stderr"))?;
    let out_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() >= deadline => {
                sola_core::process::graceful_shutdown(&mut child, Duration::from_millis(150));
                let stdout = out_h.join().unwrap_or_default();
                let _ = err_h.join();
                // `pw-cli ls` streams properties then waits on a stuck
                // node. Partial stdout is the graph; an empty timeout is not.
                if !stdout.is_empty() {
                    tracing::debug!(
                        bin,
                        cmd = %args.join(" "),
                        bytes = stdout.len(),
                        "audio helper timed out; using partial stdout"
                    );
                    return String::from_utf8(stdout).map_err(|e| e.to_string());
                }
                tracing::warn!(
                    bin,
                    cmd = %args.join(" "),
                    "audio helper timed out after {timeout:?}"
                );
                return Err(format!("{bin} {}: timed out", args.join(" ")));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                let _ = out_h.join();
                let _ = err_h.join();
                return Err(format!("{bin}: {e}"));
            }
        }
    };
    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "{bin} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr)
        ));
    }
    String::from_utf8(stdout).map_err(|e| e.to_string())
}

pub fn parse_inspect_id(out: &str) -> Option<u32> {
    let line = out.lines().next()?.trim();
    let rest = line.strip_prefix("id ")?;
    rest.split(',').next()?.trim().parse().ok()
}

pub fn parse_get_volume(out: &str) -> Option<(f32, bool)> {
    // `Volume: 0.90` or `Volume: 0.90 [MUTED]`
    let line = out.lines().find(|l| l.contains("Volume:"))?;
    let after = line.split("Volume:").nth(1)?.trim();
    let muted = after.contains("MUTED");
    let num = after.split_whitespace().next()?;
    let vol: f32 = num.parse().ok()?;
    Some((vol.clamp(0.0, 1.5), muted))
}

/// `pw-cli ls Node` property dump. Streams without enumerating SPA params.
pub fn parse_cli_nodes(text: &str) -> (Vec<Device>, Vec<Device>) {
    let mut sinks = Vec::new();
    let mut sources = Vec::new();
    let mut cur_id: Option<u32> = None;
    let mut class = String::new();
    let mut description = String::new();
    let mut nick = String::new();
    let mut name = String::new();

    let flush = |sinks: &mut Vec<Device>,
                 sources: &mut Vec<Device>,
                 id: Option<u32>,
                 class: &str,
                 description: &str,
                 nick: &str,
                 name: &str| {
        let Some(id) = id else {
            return;
        };
        if name.ends_with(".monitor") {
            return;
        }
        if class.contains("Internal") {
            return;
        }
        let label = [description, nick, name]
            .into_iter()
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or("Audio device")
            .to_string();
        match class {
            "Audio/Sink" => sinks.push(Device {
                id,
                name: label,
                kind: Kind::Output,
            }),
            "Audio/Source" => sources.push(Device {
                id,
                name: label,
                kind: Kind::Input,
            }),
            _ => {}
        }
    };

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("id ") {
            flush(
                &mut sinks,
                &mut sources,
                cur_id,
                &class,
                &description,
                &nick,
                &name,
            );
            cur_id = rest.split(',').next().and_then(|s| s.trim().parse().ok());
            class.clear();
            description.clear();
            nick.clear();
            name.clear();
            continue;
        }
        let Some((k, v)) = parse_cli_prop(line) else {
            continue;
        };
        match k {
            "media.class" => class = v,
            "node.description" => description = v,
            "node.nick" => nick = v,
            "node.name" => name = v,
            _ => {}
        }
    }
    flush(
        &mut sinks,
        &mut sources,
        cur_id,
        &class,
        &description,
        &nick,
        &name,
    );
    sinks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    sources.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    (sinks, sources)
}

fn parse_cli_prop(line: &str) -> Option<(&str, String)> {
    let (k, rest) = line.split_once(" = ")?;
    let v = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some((k.trim(), v.to_string()))
}

/// PipeWire `pw-dump` sometimes emits unnamed empty arrays as object
/// members (`"Tag": [ ], [ ], [ ]`) which is not JSON. Drop `, [ ]`
/// so serde can read the rest. Named empty arrays (`"Format": [ ]`) stay.
pub fn strip_unnamed_empty_arrays(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b',' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'[' {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < b.len() && b[k] == b']' {
                    i = k + 1;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

pub fn parse_nodes(dump: &str) -> Result<(Vec<Device>, Vec<Device>), String> {
    let objs: Vec<serde_json::Value> = match serde_json::from_str(dump) {
        Ok(v) => v,
        Err(_) => {
            let cleaned = strip_unnamed_empty_arrays(dump);
            serde_json::from_str(&cleaned).map_err(|e| e.to_string())?
        }
    };
    let mut sinks = Vec::new();
    let mut sources = Vec::new();
    for o in objs {
        let id = match o.get("id").and_then(|v| v.as_u64()) {
            Some(n) => n as u32,
            None => continue,
        };
        let ty = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !ty.contains("Node") {
            continue;
        }
        let props = o
            .pointer("/info/props")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let class = props
            .get("media.class")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = props
            .get("node.name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.ends_with(".monitor") {
            continue;
        }
        if class.contains("Internal") {
            continue;
        }
        let label = props
            .get("node.description")
            .and_then(|v| v.as_str())
            .or_else(|| props.get("node.nick").and_then(|v| v.as_str()))
            .or_else(|| props.get("node.name").and_then(|v| v.as_str()))
            .unwrap_or("Audio device")
            .to_string();
        match class {
            "Audio/Sink" => sinks.push(Device {
                id,
                name: label,
                kind: Kind::Output,
            }),
            "Audio/Source" => sources.push(Device {
                id,
                name: label,
                kind: Kind::Input,
            }),
            _ => {}
        }
    }
    sinks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    sources.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok((sinks, sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const DUMP: &str = r#"[
      {"id": 66, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink",
        "node.description": "HDMI",
        "node.name": "alsa_output.hdmi"
      }}},
      {"id": 45, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink",
        "node.description": "WH-CH520",
        "node.name": "bluez_output.xx"
      }}},
      {"id": 168, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Source",
        "node.description": "WH-CH520",
        "node.name": "bluez_input.xx"
      }}},
      {"id": 99, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Source",
        "node.description": "Monitor of HDMI",
        "node.name": "alsa_output.hdmi.monitor"
      }}},
      {"id": 145, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Stream/Input/Audio/Internal",
        "node.description": "internal",
        "node.name": "bluez_capture_internal.xx"
      }}}
    ]"#;

    #[test]
    fn parse_tolerates_unnamed_empty_param_arrays() {
        let dump = r#"[
      {"id": 45, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink",
        "node.description": "WH-CH520",
        "node.name": "bluez_output.xx"
      }}},
      {"id": 48, "type": "PipeWire:Interface:Port", "info": {"params": {
        "EnumFormat": [{"mediaType": "application"}],
        "Format": [ ],
        "Buffers": [ ],
        "Tag": [ ],
        [ ],
        [ ]
      }}}
    ]"#;
        assert!(serde_json::from_str::<serde_json::Value>(dump).is_err());
        let (sinks, sources) = parse_nodes(dump).expect("dump");
        assert_eq!(
            sinks.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["WH-CH520"]
        );
        assert!(sources.is_empty());
    }

    #[test]
    fn parse_sinks_and_sources_skips_monitors_and_internal() {
        let (sinks, sources) = parse_nodes(DUMP).expect("dump");
        let names: Vec<&str> = sinks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["HDMI", "WH-CH520"]);
        assert_eq!(
            sources.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["WH-CH520"]
        );
        assert!(!sources.iter().any(|d| d.id == 99 || d.id == 145));
    }

    #[test]
    fn inspect_id_line() {
        assert_eq!(
            parse_inspect_id("id 66, type PipeWire:Interface:Node\n    alsa.card = \"0\"\n"),
            Some(66)
        );
        assert_eq!(parse_inspect_id(""), None);
    }

    #[test]
    fn get_volume_plain_and_muted() {
        assert_eq!(parse_get_volume("Volume: 0.90\n"), Some((0.90, false)));
        assert_eq!(
            parse_get_volume("Volume: 0.40 [MUTED]\n"),
            Some((0.40, true))
        );
    }

    #[test]
    fn helper_captures_stdout() {
        let out = super::run_with_timeout(&["echo", "ok"], Duration::from_secs(1)).expect("echo");
        assert_eq!(out.trim(), "ok");
    }

    #[test]
    fn helper_times_out() {
        let start = Instant::now();
        let err = super::run_with_timeout(&["sleep", "5"], Duration::from_millis(120)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn helper_keeps_partial_stdout_on_timeout() {
        let out = super::run_with_timeout(
            &["sh", "-c", "printf 'hello\n'; exec sleep 5"],
            Duration::from_millis(250),
        )
        .expect("partial stdout");
        assert_eq!(out.trim(), "hello");
    }

    const CLI: &str = r#"	id 53, type PipeWire:Interface:Node/3
		node.description = "LifeCam Cinema Mono"
		node.name = "alsa_input.usb-Microsoft_Microsoft___LifeCam_Cinema_TM_-02.mono-fallback"
		media.class = "Audio/Source"
	id 56, type PipeWire:Interface:Node/3
		node.description = "GA102 High Definition Audio Controller Digital Stereo (HDMI)"
		node.nick = "DELL U4025QW"
		node.name = "alsa_output.pci-0000_3d_00.1.hdmi-stereo"
		media.class = "Audio/Sink"
	id 138, type PipeWire:Interface:Node/3
		node.description = "WH-CH520"
		node.name = "bluez_input_internal.14_06_A7_0E_A5_DA.0"
		media.class = "Audio/Source/Internal"
	id 148, type PipeWire:Interface:Node/3
		node.description = "WH-CH520"
		node.name = "bluez_input.14:06:A7:0E:A5:DA"
		media.class = "Audio/Source"
	id 163, type PipeWire:Interface:Node/3
		node.description = "WH-CH520"
		node.name = "bluez_output.14_06_A7_0E_A5_DA.1"
		media.class = "Audio/Sink"
	id 99, type PipeWire:Interface:Node/3
		node.description = "Monitor of HDMI"
		node.name = "alsa_output.hdmi.monitor"
		media.class = "Audio/Source"
"#;

    #[test]
    fn parse_cli_ls_skips_internal_and_monitors() {
        let (sinks, sources) = parse_cli_nodes(CLI);
        assert_eq!(
            sinks
                .iter()
                .map(|d| (d.id, d.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    56,
                    "GA102 High Definition Audio Controller Digital Stereo (HDMI)"
                ),
                (163, "WH-CH520"),
            ]
        );
        assert_eq!(
            sources
                .iter()
                .map(|d| (d.id, d.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(53, "LifeCam Cinema Mono"), (148, "WH-CH520")]
        );
    }

    #[test]
    fn parse_cli_partial_dump_still_lists_endpoints() {
        // Same shape as a 2s timeout: properties arrived, process did not exit.
        let (sinks, sources) = parse_cli_nodes(
            "\tid 56, type PipeWire:Interface:Node/3\n\t\tmedia.class = \"Audio/Sink\"\n\t\tnode.nick = \"DELL U4025QW\"\n",
        );
        assert_eq!(sinks.len(), 1);
        assert_eq!(sinks[0].name, "DELL U4025QW");
        assert!(sources.is_empty());
    }
}
