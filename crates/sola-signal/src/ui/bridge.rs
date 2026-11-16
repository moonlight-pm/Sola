//! The store and the engine for the window (process-wide, as the kit's
//! bus bridge is), and the engine's events as an iced subscription.

use std::sync::{Mutex, OnceLock, mpsc};

use iced::Subscription;
use iced::futures::Stream;

use crate::engine::{self, Command, Event, Handle};
use crate::store::Store;

static STORE: OnceLock<Store> = OnceLock::new();
static ENGINE: OnceLock<Handle> = OnceLock::new();
static EVENTS: Mutex<Option<mpsc::Receiver<Event>>> = Mutex::new(None);

/// Start the engine over `store`. Once, before the window.
pub fn start(store: Store) {
    let (events, receiver) = mpsc::channel();
    *EVENTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);
    let handle = engine::start(store.clone(), move |event| drop(events.send(event)));
    let _ = STORE.set(store);
    let _ = ENGINE.set(handle);
}

pub fn store() -> &'static Store {
    STORE.get().expect("the store, set before the window")
}

pub fn tell(command: Command) {
    match ENGINE.get() {
        Some(engine) => engine.send(command),
        None => tracing::warn!("a command before the engine started"),
    }
}

pub fn events() -> Subscription<Event> {
    Subscription::run(stream)
}

fn stream() -> impl Stream<Item = Event> {
    let receiver = EVENTS.lock().unwrap_or_else(|e| e.into_inner()).take();
    let (to_iced, from_engine) = iced::futures::channel::mpsc::unbounded::<Event>();
    match receiver {
        Some(receiver) => {
            std::thread::Builder::new()
                .name("sola-signal-events".into())
                .spawn(move || {
                    while let Ok(event) = receiver.recv() {
                        if to_iced.unbounded_send(event).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn the event forwarder");
        }
        None => tracing::warn!("the engine's events were already taken"),
    }
    from_engine
}
