//! Bus helpers for `solactl emit`. Call verbs go through `sola-call`.

use sola_bus::BusClient;
use sola_bus::topics::Topic;

/// Connect a fresh `BusClient` and identify as `solactl`. Exits the
/// process on failure (the CLI has nothing to do without the bus).
pub fn connect_or_exit() -> BusClient {
    let mut client = BusClient::new();
    client.set_app_id("solactl");
    if let Err(e) = client.connect() {
        eprintln!("solactl: bus connect failed: {e}");
        std::process::exit(3);
    }
    client
}

/// Emit a topic, exiting on failure.
pub fn emit(client: &mut BusClient, topic: Topic) {
    if let Err(e) = client.emit(topic) {
        eprintln!("solactl: bus emit failed: {e}");
        std::process::exit(3);
    }
}
