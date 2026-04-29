use std::collections::HashMap;

use crate::topics::{Topic, TopicKind};

/// A bus delivery to a subscriber. Wraps the parsed `Topic` with a
/// `retracted` flag so handlers for `#[sticky]` / `#[persistent]` topic
/// kinds can branch on add/remove. For ephemeral topic kinds,
/// `retracted` is always `false`.
///
/// `source` is the emitter's `app_id`. Restored stickies replayed at
/// bus startup carry `source = "sola-bus"` (the `BUS_SOURCE` constant
/// in `state.rs`). Apps that emit and subscribe to the same topic can
/// filter their own self-echo by comparing `source` to their `APP_ID`.
#[derive(Debug)]
pub struct Delivery<'a> {
    pub topic: &'a Topic,
    pub retracted: bool,
    pub source: &'a str,
}

/// Handler signature for a registered topic. Receives the app state, the
/// delivery (parsed topic + retracted flag), and an app-supplied context
/// (e.g. a framework's AppCtx).
pub type BusHandler<A, C> = fn(&mut A, &Delivery, &mut C);

/// Per-topic handler registry. The set of registered topic kinds is the
/// app's bus subscription list.
///
/// Generic over the app state `A` and an app-supplied context `C` (often
/// a framework's per-app context, e.g. `sola_app::AppCtx`).
pub struct BusRegistry<A, C> {
    handlers: HashMap<TopicKind, BusHandler<A, C>>,
    subscribe_all: bool,
}

impl<A, C> BusRegistry<A, C> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            subscribe_all: false,
        }
    }

    /// Register a handler for a topic kind. Registering the same kind
    /// twice is a dev-build panic and a release-build warn-and-skip.
    pub fn on(&mut self, kind: TopicKind, handler: BusHandler<A, C>) {
        if self.handlers.insert(kind, handler).is_some() {
            if cfg!(debug_assertions) {
                panic!("duplicate bus handler for {:?}", kind);
            } else {
                tracing::warn!(?kind, "duplicate bus handler; last registration wins");
            }
        }
    }

    /// Subscribe to every topic kind. Used by diagnostic apps
    /// (sola-monitor). Handlers still dispatch only for registered kinds;
    /// everything else falls through to whatever raw-message hook the
    /// caller exposes.
    pub fn subscribe_all(&mut self) {
        self.subscribe_all = true;
    }

    /// Topic kinds this registry wants to subscribe to.
    pub fn kinds(&self) -> Vec<TopicKind> {
        if self.subscribe_all {
            TopicKind::ALL.to_vec()
        } else {
            self.handlers.keys().copied().collect()
        }
    }

    /// Dispatch a parsed delivery to its registered handler. No-op if no
    /// handler is registered for this kind.
    pub fn dispatch(&self, delivery: &Delivery, app: &mut A, ctx: &mut C) {
        if let Some(handler) = self.handlers.get(&delivery.topic.kind()) {
            handler(app, delivery, ctx);
        }
    }
}

impl<A, C> Default for BusRegistry<A, C> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test types — the registry doesn't care about A's or C's shape.
    struct TestApp {
        last: Option<TopicKind>,
        last_retracted: bool,
        last_source: String,
    }
    struct TestCtx;

    // A stub handler that panics if called — we only test kinds/subscribe_all
    // here.
    fn stub(_app: &mut TestApp, _delivery: &Delivery, _ctx: &mut TestCtx) {
        unreachable!("not dispatched in these tests");
    }

    fn record(app: &mut TestApp, delivery: &Delivery, _ctx: &mut TestCtx) {
        app.last = Some(delivery.topic.kind());
        app.last_retracted = delivery.retracted;
        app.last_source = delivery.source.to_string();
    }

    fn empty_app() -> TestApp {
        TestApp {
            last: None,
            last_retracted: false,
            last_source: String::new(),
        }
    }

    #[test]
    fn kinds_reflects_registered() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, stub);
        reg.on(TopicKind::Windows, stub);
        let mut kinds = reg.kinds();
        kinds.sort_by_key(|k| k.as_str());
        let mut expected = vec![TopicKind::CloseApp, TopicKind::Windows];
        expected.sort_by_key(|k| k.as_str());
        assert_eq!(kinds, expected);
    }

    #[test]
    fn subscribe_all_overrides_registered() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, stub);
        reg.subscribe_all();
        let kinds = reg.kinds();
        assert_eq!(kinds.len(), TopicKind::ALL.len());
    }

    #[test]
    fn dispatch_routes_to_registered_handler() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::Shutdown, record);
        let mut app = empty_app();
        let mut ctx = TestCtx;
        let topic = Topic::Shutdown;
        let delivery = Delivery {
            topic: &topic,
            retracted: false,
            source: "test",
        };
        reg.dispatch(&delivery, &mut app, &mut ctx);
        assert_eq!(app.last, Some(TopicKind::Shutdown));
        assert!(!app.last_retracted);
    }

    #[test]
    fn dispatch_passes_retracted_flag() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::Shutdown, record);
        let mut app = empty_app();
        let mut ctx = TestCtx;
        let topic = Topic::Shutdown;
        let delivery = Delivery {
            topic: &topic,
            retracted: true,
            source: "test",
        };
        reg.dispatch(&delivery, &mut app, &mut ctx);
        assert!(app.last_retracted);
    }

    #[test]
    fn dispatch_passes_source() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::Shutdown, record);
        let mut app = empty_app();
        let mut ctx = TestCtx;
        let topic = Topic::Shutdown;
        let delivery = Delivery {
            topic: &topic,
            retracted: false,
            source: "sola-test",
        };
        reg.dispatch(&delivery, &mut app, &mut ctx);
        assert_eq!(app.last_source, "sola-test");
    }
}
