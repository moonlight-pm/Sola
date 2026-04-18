use std::collections::HashMap;

use sola_bus::topics::{Topic, TopicKind};

use crate::ctx::AppCtx;

/// Handler signature for a registered topic. The full `Topic` is passed
/// so the handler can destructure the variant it registered for.
pub type BusHandler<A> = fn(&mut A, &Topic, &mut AppCtx);

/// Per-topic handler registry. The set of registered topic kinds is
/// the app's bus subscription list.
pub struct BusRegistry<A> {
    handlers: HashMap<TopicKind, BusHandler<A>>,
    subscribe_all: bool,
}

impl<A> BusRegistry<A> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            subscribe_all: false,
        }
    }

    /// Register a handler for a topic kind. Registering the same kind
    /// twice is a dev-build panic and a release-build warn-and-skip.
    pub fn on(&mut self, kind: TopicKind, handler: BusHandler<A>) {
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
    /// everything else falls through to `on_raw_bus_message`.
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

    /// Dispatch a parsed topic to its registered handler. No-op if no
    /// handler is registered for this kind.
    pub fn dispatch(&self, topic: &Topic, app: &mut A, ctx: &mut AppCtx) {
        if let Some(handler) = self.handlers.get(&topic.kind()) {
            handler(app, topic, ctx);
        }
    }
}

impl<A> Default for BusRegistry<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test type — the registry doesn't care about A's shape.
    struct TestApp {
        count: u32,
    }

    // A stub handler that panics if called — we only test kinds/subscribe_all
    // here. Actual dispatch is covered indirectly via integration tests
    // on app conversions in Phase 3.
    fn stub(_app: &mut TestApp, _topic: &Topic, _ctx: &mut AppCtx) {
        unreachable!("not dispatched in these tests");
    }

    #[test]
    fn kinds_reflects_registered() {
        let mut reg: BusRegistry<TestApp> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, stub);
        reg.on(TopicKind::Apps, stub);
        let mut kinds = reg.kinds();
        kinds.sort_by_key(|k| k.as_str());
        let mut expected = vec![TopicKind::CloseApp, TopicKind::Apps];
        expected.sort_by_key(|k| k.as_str());
        assert_eq!(kinds, expected);
    }

    #[test]
    fn subscribe_all_overrides_registered() {
        let mut reg: BusRegistry<TestApp> = BusRegistry::new();
        reg.on(TopicKind::CloseApp, stub);
        reg.subscribe_all();
        let kinds = reg.kinds();
        assert_eq!(kinds.len(), TopicKind::ALL.len());
    }
}
