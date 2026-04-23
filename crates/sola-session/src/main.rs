mod session;

use tracing::info;

fn main() {
    sola_core::log::init("sola-session");

    info!("sola-session starting");
    session::run();
}
