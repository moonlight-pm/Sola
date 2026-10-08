//! Bots daemon — catalog, ACP children, call owner `bots`.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use sola_bots::calls;
use sola_bots::host::Host;
use sola_call::Incoming;

fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        // Crate is `sola_bots`; process name `sola-botsd` would otherwise
        // filter out host/acp logs.
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                "sola_bots=info,sola_botsd=info,sola_core=info,sola_call=info",
            );
        }
    }
    sola_core::log::init("sola-botsd");
    tracing::info!("sola-botsd starting");

    wait_for_call();

    let host = Host::start();
    sola_bots::http::spawn(std::sync::Arc::clone(&host));
    let rx = sola_call::start_provider(calls::OWNER, "sola-botsd", calls::methods());

    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(incoming) => dispatch_async(&host, incoming),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::error!("call provider channel closed");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn dispatch_async(host: &std::sync::Arc<Host>, incoming: Incoming) {
    let host = std::sync::Arc::clone(host);
    thread::Builder::new()
        .name("bots-call".into())
        .spawn(move || calls::dispatch(&host, incoming))
        .ok();
}

fn wait_for_call() {
    let path = sola_call::socket_path();
    for _ in 0..50 {
        if std::path::Path::new(&path).exists() {
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
    tracing::warn!("call socket not up yet; provider will retry");
}
