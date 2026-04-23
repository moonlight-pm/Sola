//! Pick the highest-resolution mode ≥60Hz on each head.
//!
//! On startup we bind `zwlr_output_manager_v1`, collect modes per head,
//! and on the first `done` serial where any head isn't already running
//! its best mode, issue a configuration to apply it.

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

#[derive(Default)]
pub struct OutputModeInfo {
    pub width: i32,
    pub height: i32,
    pub refresh_mhz: i32,
    pub preferred: bool,
    pub head: Option<ObjectId>,
}

#[derive(Default)]
pub struct OutputHeadInfo {
    pub name: String,
    pub enabled: bool,
    pub current_mode: Option<ObjectId>,
    pub modes: Vec<ObjectId>,
    pub proxy: Option<ZwlrOutputHeadV1>,
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
        let mut best: Option<(ObjectId, i64, i32)> = None; // (id, pixels, refresh)
        for mode_id in &head.modes {
            let Some(m) = self.modes.get(mode_id) else {
                continue;
            };
            if m.refresh_mhz < MIN_REFRESH_MHZ {
                continue;
            }
            let pixels = (m.width as i64) * (m.height as i64);
            let score = (pixels, m.refresh_mhz);
            let better = match &best {
                None => true,
                Some((_, bp, br)) => score > (*bp, *br),
            };
            if better {
                best = Some((mode_id.clone(), pixels, m.refresh_mhz));
            }
        }
        best.map(|(id, _, _)| id)
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
                        "applying best output mode"
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
