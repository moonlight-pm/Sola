//! PipeWire graph + WirePlumber `wpctl` helpers.
//! Parsing is unit-tested; the worker never runs on the iced thread.

use super::{Device, Kind, Snapshot};
use std::process::Command;

pub fn snapshot() -> Snapshot {
    let Ok(dump) = run(&["pw-dump"]) else {
        return Snapshot::default();
    };
    let (sinks, sources) = match parse_nodes(&dump) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("audio pw-dump parse: {e}");
            return Snapshot::default();
        }
    };
    if sinks.is_empty() && sources.is_empty() {
        // Graph came back but no endpoints — still "available" so the
        // chip can show a quiet zero, unless dump itself failed.
        return Snapshot {
            available: true,
            ..Snapshot::default()
        };
    }
    let default_sink = inspect_id("@DEFAULT_AUDIO_SINK@");
    let default_source = inspect_id("@DEFAULT_AUDIO_SOURCE@");
    let (sink_volume, sink_mute) = default_sink
        .and_then(|id| get_volume(id))
        .unwrap_or((0.0, false));
    let (source_volume, source_mute) = default_source
        .and_then(|id| get_volume(id))
        .unwrap_or((0.0, false));
    Snapshot {
        available: true,
        sinks,
        sources,
        default_sink,
        default_source,
        sink_volume,
        sink_mute,
        source_volume,
        source_mute,
    }
}

pub fn set_volume(id: u32, volume: f32) -> bool {
    let pct = ((volume.clamp(0.0, 1.0)) * 100.0).round();
    run(&[
        "wpctl",
        "set-volume",
        &id.to_string(),
        &format!("{pct:.0}%"),
        "-l",
        "1.0",
    ])
    .is_ok()
}

pub fn set_mute(id: u32, mute: bool) -> bool {
    let v = if mute { "1" } else { "0" };
    run(&["wpctl", "set-mute", &id.to_string(), v]).is_ok()
}

pub fn set_default(id: u32) -> bool {
    run(&["wpctl", "set-default", &id.to_string()]).is_ok()
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
    let (bin, args) = cmd
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("{bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{bin} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
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

pub fn parse_nodes(dump: &str) -> Result<(Vec<Device>, Vec<Device>), String> {
    let objs: Vec<serde_json::Value> = serde_json::from_str(dump).map_err(|e| e.to_string())?;
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
}
