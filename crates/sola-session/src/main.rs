mod adopted;
mod display;
mod moved;
mod quit;
mod session;
mod tag;
mod watch;

use tracing::info;

fn main() {
    sola_core::log::init("sola-session");

    info!("sola-session starting");
    session::run();
}
