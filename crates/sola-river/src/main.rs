use std::path::Path;
use std::process::exit;
use std::time::Duration;

use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod bus;
mod client;
mod pending;
mod protocol;
mod registry;
mod supervisor;
mod translator;

fn main() {
    init_tracing();
    info!("sola-river starting");

    let mut sup = match supervisor::RiverSupervisor::spawn(Path::new("/opt/sola/log/river.log"))
    {
        Ok(s) => s,
        Err(e) => {
            error!(%e, "failed to spawn river");
            exit(1);
        }
    };

    if let Err(e) = sup.wait_for_socket() {
        error!(%e, "river socket never appeared");
        sup.shutdown();
        exit(1);
    }

    // Phase 5 replaces this poll loop with the wayland client.
    loop {
        match sup.try_wait() {
            Ok(Some(status)) => {
                error!(?status, "river exited; sola-river exiting");
                exit(1);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                error!(%e, "try_wait failed");
                exit(1);
            }
        }
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_river=info".into());
    let _ = std::fs::create_dir_all("/opt/sola/log");
    let file_appender = tracing_appender::rolling::never("/opt/sola/log", "sola-river.log");
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();
}
