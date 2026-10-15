//! sola-design — HTML artboards on a canvas, edited by agents.
//!
//! The design is plain HTML/CSS under `<project>/design/<canvas>/`. Each
//! artboard is one CEF board tab painted onto an iced plane. There are no
//! editing tools: agents edit the files and the app reloads.
//!
//! `sola-design` is the chrome; `sola-design --engine --profile=sola-design`
//! is the headless CEF helper it spawns. CEF subprocesses are the same binary.

mod annot;
mod annotate;
mod app;
mod boards;
mod calls;
mod camera;
mod canvas;
mod detail;
mod frames;
mod glow;
mod ime;
mod inspect;
mod instance;
mod loads;
mod marks;
mod menu;
mod overlay;
mod pace;
mod plane;
mod profile;
mod projects;
mod rail;
mod report;
mod routes;
mod scene;
mod select;
mod serve;
mod share;
mod tour;
mod watch;

use std::process::ExitCode;

use sola_browser::{CefEngine, Engine};

pub const APP_ID: &str = "sola-design";

fn main() -> ExitCode {
    // CEF renderer / GPU / utility workers — before log, Wayland, or iced.
    if let Some(code) = CefEngine::dispatch_subprocess(APP_ID) {
        return code;
    }

    if sola_browser::cef::host::is_engine_process() {
        sola_core::log::init(APP_ID);
        if let Err(e) = profile::bind() {
            eprintln!("sola-design: bind profile: {e}");
            return ExitCode::FAILURE;
        }
        return sola_browser::cef::host::try_run(APP_ID).unwrap_or(ExitCode::FAILURE);
    }

    if let instance::Claim::Handoff = instance::claim() {
        return match instance::handoff() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("sola-design: {e}");
                ExitCode::FAILURE
            }
        };
    }

    sola_kit::app::startup(APP_ID);
    reap_inherited_children();
    if let Err(e) = profile::bind() {
        tracing::error!(error = %e, "bind design profile");
        return ExitCode::FAILURE;
    }
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "iced");
            ExitCode::FAILURE
        }
    }
}

/// Children of the image before a self-restart (`execv` keeps the pid, so
/// they stay ours): the engine helper and the tunnel connector, already
/// exited. Reap them so they do not linger as zombies, one set per install.
fn reap_inherited_children() {
    loop {
        // SAFETY: waitpid with WNOHANG only collects exited children.
        let pid = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if pid <= 0 {
            break;
        }
        tracing::debug!(pid, "reaped a child from before the restart");
    }
}
