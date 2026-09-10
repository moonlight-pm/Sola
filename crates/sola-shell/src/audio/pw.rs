//! PipeWire graph + WirePlumber `wpctl` helpers.
//! Parsing is unit-tested; the worker never runs on the iced thread.
//!
//! Helpers are time-bounded. A stuck `pw-dump` / `wpctl` used to block the
//! audio worker forever, so device clicks never reached `set-default`.

use super::{Device, Kind, Snapshot};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Bound for `pw-dump` / `wpctl`. Control-plane stalls (hung WirePlumber)
/// must not freeze the picker.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);

pub fn snapshot() -> Result<Snapshot, String> {
    let dump = run(&["pw-dump"])?;
    let (sinks, sources) = match parse_nodes(&dump) {
        Ok(v) => v,
        Err(e) => {
            // Dump ran; PipeWire is up. A SPA params quirk must not hide
            // the chip (that path is for a missing graph).
            tracing::warn!("audio pw-dump parse: {e}");
            (Vec::new(), Vec::new())
        }
    };
    if sinks.is_empty() && sources.is_empty() {
        // Graph came back but no endpoints — still "available" so the
        // chip can show a quiet zero, unless dump itself failed.
        return Ok(Snapshot {
            available: true,
            ..Snapshot::default()
        });
    }
    let default_sink = inspect_id("@DEFAULT_AUDIO_SINK@");
    let default_source = inspect_id("@DEFAULT_AUDIO_SOURCE@");
    let (sink_volume, sink_mute) = default_sink
        .and_then(|id| get_volume(id))
        .unwrap_or((0.0, false));
    let (source_volume, source_mute) = default_source
        .and_then(|id| get_volume(id))
        .unwrap_or((0.0, false));
    Ok(Snapshot {
        available: true,
        sinks,
        sources,
        default_sink,
        default_source,
        sink_volume,
        sink_mute,
        source_volume,
        source_mute,
    })
}

pub fn set_volume(id: u32, volume: f32) -> bool {
    let pct = ((volume.clamp(0.0, 1.0)) * 100.0).round();
    log_cmd(
        run(&[
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
        run(&["wpctl", "set-mute", &id.to_string(), v]),
        "set-mute",
        id,
    )
}

pub fn set_default(id: u32) -> bool {
    log_cmd(
        run(&["wpctl", "set-default", &id.to_string()]),
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
    let out = run(&["wpctl", "inspect", spec]).ok()?;
    parse_inspect_id(&out)
}

fn get_volume(id: u32) -> Option<(f32, bool)> {
    let out = run(&["wpctl", "get-volume", &id.to_string()]).ok()?;
    parse_get_volume(&out)
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
                tracing::warn!(
                    bin,
                    cmd = %args.join(" "),
                    "audio helper timed out after {timeout:?}"
                );
                sola_core::process::graceful_shutdown(&mut child, Duration::from_millis(150));
                let _ = out_h.join();
                let _ = err_h.join();
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
}
