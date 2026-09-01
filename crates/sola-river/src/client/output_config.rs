//! Pick an output mode on each head.
//!
//! Default: highest-resolution mode whose pixel aspect matches the
//! panel's physical mm (so a stacked 2560×2880@30 HDMI wins over
//! 4K@60 16:9). If physical size is unknown, highest-resolution ≥60Hz.
//!
//! Virtio-gpu advertises a long CVT list (1080p, 4K, …). Picking the
//! largest makes a QEMU window balloon past the host `xres`/`yres`.
//! `SOLA_OUTPUT_PICK=preferred` uses the EDID preferred mode instead
//! (Oath sets this so virtio-gpu `xres`/`yres` wins).
//! `SOLA_OUTPUT_MODE=WxH` requests an exact size (closest ≥60Hz / 0Hz
//! fallback).
//!
//! On startup we bind `zwlr_output_manager_v1`, collect modes per head,
//! and on the first `done` serial where any head isn't already running
//! its target mode, issue a configuration to apply it.

use std::collections::HashMap;

use tracing::{info, warn};
use wayland_client::backend::ObjectId;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use crate::client::AppData;
use crate::protocol::wlr_output_management_unstable_v1::{
    zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1,
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};

/// Hz * 1000 — wlr-output-management's refresh rate unit.
const MIN_REFRESH_MHZ: i32 = 60_000;

#[derive(Clone, Default)]
pub struct OutputModeInfo {
    pub width: i32,
    pub height: i32,
    pub refresh_mhz: i32,
    pub preferred: bool,
    pub head: Option<ObjectId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModePick {
    Max,
    Preferred,
    Exact { width: i32, height: i32 },
}

fn parse_wxh(s: &str) -> Option<(i32, i32)> {
    let s = s.trim().replace('×', "x");
    let (w, h) = s.split_once('x')?;
    let width: i32 = w.trim().parse().ok()?;
    let height: i32 = h.trim().parse().ok()?;
    (width >= 640 && height >= 480).then_some((width, height))
}

fn mode_pick_from_env() -> ModePick {
    if let Ok(s) = std::env::var("SOLA_OUTPUT_MODE") {
        if let Some((width, height)) = parse_wxh(&s) {
            return ModePick::Exact { width, height };
        }
    }
    match std::env::var("SOLA_OUTPUT_PICK").ok().as_deref() {
        Some("preferred") => ModePick::Preferred,
        _ => ModePick::Max,
    }
}

fn mode_ok_60(m: &OutputModeInfo) -> bool {
    m.refresh_mhz >= MIN_REFRESH_MHZ
}

fn aspect(w: i32, h: i32) -> f64 {
    if h == 0 {
        0.0
    } else {
        w as f64 / h as f64
    }
}

/// Relative error between a mode's pixels and the panel's physical mm.
fn aspect_err(m: &OutputModeInfo, phys_w: i32, phys_h: i32) -> f64 {
    let target = aspect(phys_w, phys_h).abs().max(1e-6);
    (aspect(m.width, m.height) - aspect(phys_w, phys_h)).abs() / target
}

/// Modes whose pixel aspect matches the physical panel (e.g. 2560×2880
/// stacked 16:9 on a ~square 470×520 mm HDMI). 8% slack.
const ASPECT_SLACK: f64 = 0.08;

fn pick_max(modes: &[OutputModeInfo], phys: Option<(i32, i32)>) -> Option<usize> {
    if let Some((pw, ph)) = phys.filter(|(w, h)| *w > 0 && *h > 0) {
        let matching: Vec<usize> = modes
            .iter()
            .enumerate()
            .filter(|(_, m)| aspect_err(m, pw, ph) <= ASPECT_SLACK)
            .map(|(i, _)| i)
            .collect();
        if !matching.is_empty() {
            // Prefer 60Hz among aspect matches, else the native 30Hz stacked
            // mode beats a 16:9 4K@60 that does not fit the panel.
            return matching.into_iter().max_by_key(|&i| {
                let m = &modes[i];
                let hz60 = i32::from(mode_ok_60(m) || m.refresh_mhz == 0);
                (hz60, m.width as i64 * m.height as i64, m.refresh_mhz)
            });
        }
    }
    modes
        .iter()
        .enumerate()
        .filter(|(_, m)| mode_ok_60(m) || m.refresh_mhz == 0)
        .max_by_key(|(_, m)| (m.width as i64 * m.height as i64, m.refresh_mhz))
        .map(|(i, _)| i)
}

fn pick_mode(modes: &[OutputModeInfo], pick: ModePick, phys: Option<(i32, i32)>) -> Option<usize> {
    match pick {
        ModePick::Exact { width, height } => {
            let exact: Vec<usize> = modes
                .iter()
                .enumerate()
                .filter(|(_, m)| m.width == width && m.height == height)
                .map(|(i, _)| i)
                .collect();
            if !exact.is_empty() {
                return exact.into_iter().max_by_key(|&i| modes[i].refresh_mhz);
            }
            let usable: Vec<usize> = modes
                .iter()
                .enumerate()
                .filter(|(_, m)| m.refresh_mhz == 0 || mode_ok_60(m))
                .map(|(i, _)| i)
                .collect();
            let want = width as i64 * height as i64;
            usable.into_iter().min_by_key(|&i| {
                (modes[i].width as i64 * modes[i].height as i64 - want).unsigned_abs()
            })
        }
        ModePick::Preferred => modes
            .iter()
            .enumerate()
            .filter(|(_, m)| m.preferred && (m.refresh_mhz == 0 || mode_ok_60(m)))
            .max_by_key(|(_, m)| m.width as i64 * m.height as i64)
            .map(|(i, _)| i)
            .or_else(|| pick_mode(modes, ModePick::Max, phys)),
        ModePick::Max => pick_max(modes, phys),
    }
}

#[derive(Default)]
pub struct OutputHeadInfo {
    pub name: String,
    pub enabled: bool,
    pub current_mode: Option<ObjectId>,
    pub modes: Vec<ObjectId>,
    pub proxy: Option<ZwlrOutputHeadV1>,
    pub phys_width: i32,
    pub phys_height: i32,
}

#[derive(Default)]
pub struct OutputConfigState {
    pub manager: Option<ZwlrOutputManagerV1>,
    pub heads: HashMap<ObjectId, OutputHeadInfo>,
    pub modes: HashMap<ObjectId, OutputModeInfo>,
    pub mode_proxies: HashMap<ObjectId, ZwlrOutputModeV1>,
    pub last_serial: u32,
    pub pending_configure: bool,
}

impl OutputConfigState {
    fn best_mode_for(&self, head_id: &ObjectId) -> Option<ObjectId> {
        let head = self.heads.get(head_id)?;
        let mut infos = Vec::new();
        let mut ids = Vec::new();
        for mode_id in &head.modes {
            let Some(m) = self.modes.get(mode_id) else {
                continue;
            };
            infos.push(m.clone());
            ids.push(mode_id.clone());
        }
        let phys = (head.phys_width > 0 && head.phys_height > 0)
            .then_some((head.phys_width, head.phys_height));
        let idx = pick_mode(&infos, mode_pick_from_env(), phys)?;
        let m = &infos[idx];
        info!(
            name = %head.name,
            width = m.width,
            height = m.height,
            refresh_mhz = m.refresh_mhz,
            phys_w = head.phys_width,
            phys_h = head.phys_height,
            "picked output mode"
        );
        Some(ids[idx].clone())
    }
}

/// Called on every `done` event. If any head isn't running its best mode,
/// build and apply a configuration that preserves all currently-enabled
/// heads (wlr-output-management disables any head not listed in the
/// configuration, so we must enable_head each one explicitly).
pub fn reconcile(state: &mut AppData, qh: &QueueHandle<AppData>) {
    if state.output_config.pending_configure {
        return;
    }
    let manager = match state.output_config.manager.clone() {
        Some(m) => m,
        None => return,
    };

    // Plan: for each enabled head, target_mode = best-or-current.
    // Only apply if at least one head's target differs from its current.
    struct Plan {
        head: ObjectId,
        target_mode: Option<ObjectId>,
        needs_change: bool,
    }
    let mut plans: Vec<Plan> = Vec::new();
    let mut any_change = false;
    for (head_id, head) in &state.output_config.heads {
        if !head.enabled {
            continue;
        }
        let best = state.output_config.best_mode_for(head_id);
        let needs_change = match (&best, &head.current_mode) {
            (Some(b), Some(c)) => b != c,
            _ => false,
        };
        if needs_change {
            any_change = true;
            if let Some(b) = &best {
                if let Some(m) = state.output_config.modes.get(b) {
                    info!(
                        head = %head.name,
                        width = m.width,
                        height = m.height,
                        refresh_mhz = m.refresh_mhz,
                        pick = ?mode_pick_from_env(),
                        "applying output mode"
                    );
                }
            }
        }
        plans.push(Plan {
            head: head_id.clone(),
            target_mode: best,
            needs_change,
        });
    }

    if !any_change {
        return;
    }

    let config = manager.create_configuration(state.output_config.last_serial, qh, ());
    for plan in plans {
        let Some(head_proxy) = state
            .output_config
            .heads
            .get(&plan.head)
            .and_then(|h| h.proxy.clone())
        else {
            continue;
        };
        let cfg_head = config.enable_head(&head_proxy, qh, ());
        if plan.needs_change {
            if let Some(mode_id) = plan.target_mode {
                if let Some(mode_proxy) = state.output_config.mode_proxies.get(&mode_id).cloned() {
                    cfg_head.set_mode(&mode_proxy);
                }
            }
        }
    }
    config.apply();
    state.output_config.pending_configure = true;
}

// ---------- Dispatches ----------

impl Dispatch<ZwlrOutputManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputManagerV1,
        event: <ZwlrOutputManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => {
                let id = head.id();
                state.output_config.heads.entry(id).or_default().proxy = Some(head);
            }
            zwlr_output_manager_v1::Event::Done { serial } => {
                state.output_config.last_serial = serial;
                reconcile(state, qh);
            }
            zwlr_output_manager_v1::Event::Finished => {
                state.output_config.manager = None;
            }
        }
    }

    event_created_child!(AppData, ZwlrOutputManagerV1, [
        0 => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for AppData {
    fn event(
        state: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: <ZwlrOutputHeadV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let head_id = head.id();
        let entry = state
            .output_config
            .heads
            .entry(head_id.clone())
            .or_default();
        match event {
            zwlr_output_head_v1::Event::Name { name } => {
                entry.name = name;
            }
            zwlr_output_head_v1::Event::Mode { mode } => {
                let mode_id = mode.id();
                entry.modes.push(mode_id.clone());
                state
                    .output_config
                    .modes
                    .entry(mode_id.clone())
                    .or_default()
                    .head = Some(head_id);
                state.output_config.mode_proxies.insert(mode_id, mode);
            }
            zwlr_output_head_v1::Event::Enabled { enabled } => {
                entry.enabled = enabled != 0;
            }
            zwlr_output_head_v1::Event::CurrentMode { mode } => {
                entry.current_mode = Some(mode.id());
            }
            zwlr_output_head_v1::Event::PhysicalSize { width, height } => {
                entry.phys_width = width;
                entry.phys_height = height;
            }
            zwlr_output_head_v1::Event::Finished => {
                if let Some(h) = state.output_config.heads.remove(&head_id) {
                    for m in h.modes {
                        state.output_config.modes.remove(&m);
                        state.output_config.mode_proxies.remove(&m);
                    }
                    if let Some(p) = h.proxy {
                        p.release();
                    }
                }
            }
            _ => {}
        }
    }

    event_created_child!(AppData, ZwlrOutputHeadV1, [
        3 => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for AppData {
    fn event(
        state: &mut Self,
        mode: &ZwlrOutputModeV1,
        event: <ZwlrOutputModeV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mode_id = mode.id();
        let entry = state.output_config.modes.entry(mode_id).or_default();
        match event {
            zwlr_output_mode_v1::Event::Size { width, height } => {
                entry.width = width;
                entry.height = height;
            }
            zwlr_output_mode_v1::Event::Refresh { refresh } => {
                entry.refresh_mhz = refresh;
            }
            zwlr_output_mode_v1::Event::Preferred => {
                entry.preferred = true;
            }
            zwlr_output_mode_v1::Event::Finished => {
                // Mode may outlive Finished briefly; leave entry until the
                // head that owns it reports Finished too.
            }
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for AppData {
    fn event(
        state: &mut Self,
        cfg: &ZwlrOutputConfigurationV1,
        event: <ZwlrOutputConfigurationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_configuration_v1::Event::Succeeded => {
                info!("output configuration applied");
                state.output_config.pending_configure = false;
                cfg.destroy();
            }
            zwlr_output_configuration_v1::Event::Failed => {
                warn!("output configuration failed");
                state.output_config.pending_configure = false;
                cfg.destroy();
                // Try again next reconcile tick in case state drifted.
                reconcile(state, qh);
            }
            zwlr_output_configuration_v1::Event::Cancelled => {
                warn!("output configuration cancelled (serial outdated)");
                state.output_config.pending_configure = false;
                cfg.destroy();
                reconcile(state, qh);
            }
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputConfigurationHeadV1,
        _: <ZwlrOutputConfigurationHeadV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(w: i32, h: i32, hz: i32, preferred: bool) -> OutputModeInfo {
        OutputModeInfo {
            width: w,
            height: h,
            refresh_mhz: hz,
            preferred,
            head: None,
        }
    }

    #[test]
    fn parse_wxh_accepts_ascii_and_multiply() {
        assert_eq!(parse_wxh("1280x800"), Some((1280, 800)));
        assert_eq!(parse_wxh(" 1280×800 "), Some((1280, 800)));
        assert_eq!(parse_wxh("640x480"), Some((640, 480)));
        assert_eq!(parse_wxh("100x100"), None);
        assert_eq!(parse_wxh("nope"), None);
    }

    #[test]
    fn max_picks_largest_60hz_not_preferred_720p() {
        let modes = [
            mode(1280, 800, 60_000, true),
            mode(1920, 1080, 60_000, false),
            mode(3840, 2160, 60_000, false),
            mode(1024, 768, 85_000, false),
        ];
        let i = pick_mode(&modes, ModePick::Max, None).unwrap();
        assert_eq!((modes[i].width, modes[i].height), (3840, 2160));
    }

    #[test]
    fn preferred_uses_edid_not_4k_virtio_list() {
        let modes = [
            mode(1280, 800, 60_000, true),
            mode(1920, 1080, 60_000, false),
            mode(3840, 2160, 60_000, false),
        ];
        let i = pick_mode(&modes, ModePick::Preferred, None).unwrap();
        assert_eq!((modes[i].width, modes[i].height), (1280, 800));
    }

    #[test]
    fn preferred_falls_back_to_max_when_none_flagged() {
        let modes = [
            mode(1920, 1080, 60_000, false),
            mode(1280, 800, 60_000, false),
        ];
        let i = pick_mode(&modes, ModePick::Preferred, None).unwrap();
        assert_eq!((modes[i].width, modes[i].height), (1920, 1080));
    }

    #[test]
    fn exact_hits_1280x800_on_virtio_list() {
        let modes = [
            mode(1024, 768, 60_000, false),
            mode(1280, 800, 60_000, true),
            mode(3840, 2160, 60_000, false),
        ];
        let i = pick_mode(
            &modes,
            ModePick::Exact {
                width: 1280,
                height: 800,
            },
            None,
        )
        .unwrap();
        assert_eq!((modes[i].width, modes[i].height), (1280, 800));
    }

    #[test]
    fn exact_allows_30hz_stacked_mode() {
        let modes = [
            mode(3840, 2160, 60_000, true),
            mode(2560, 2880, 29_987, false),
            mode(1920, 1080, 60_000, false),
        ];
        let i = pick_mode(
            &modes,
            ModePick::Exact {
                width: 2560,
                height: 2880,
            },
            None,
        )
        .unwrap();
        assert_eq!((modes[i].width, modes[i].height), (2560, 2880));
    }

    #[test]
    fn max_picks_stacked_30hz_when_physical_is_square() {
        // Canto HDMI: two 1440p 16:9 stacked. EDID 4K@60 is 16:9; native
        // 2560×2880 is ~30Hz and matches 470×520 mm.
        let modes = [
            mode(3840, 2160, 60_000, true),
            mode(2560, 2880, 29_987, false),
            mode(1920, 1080, 60_000, false),
        ];
        let i = pick_mode(&modes, ModePick::Max, Some((470, 520))).unwrap();
        assert_eq!((modes[i].width, modes[i].height), (2560, 2880));
    }

    #[test]
    fn max_still_picks_4k_on_16x9_panel() {
        let modes = [
            mode(3840, 2160, 60_000, true),
            mode(2560, 2880, 29_987, false),
            mode(1920, 1080, 60_000, false),
        ];
        let i = pick_mode(&modes, ModePick::Max, Some((600, 340))).unwrap();
        assert_eq!((modes[i].width, modes[i].height), (3840, 2160));
    }
}
