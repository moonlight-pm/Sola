//! `sola-call` — request/reply host. Not the bus.

fn main() {
    sola_core::log::init("sola-call");
    sola_call::host::bind_and_serve(&sola_call::socket_path());
}
