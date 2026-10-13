//! Board frames: engine mailbox → per-tab pending map → plane shader.
//!
//! One blocking thread drains the engine's frame mailbox. Frames wait in
//! [`BoardFrames`] (latest per tab, dirty rects merged) until the plane's
//! next `prepare` uploads them. iced is woken once per batch.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use iced::futures::{SinkExt, StreamExt as _};
use iced::{Subscription, stream};
use iced_futures::subscription::{self, EventStream, Recipe};
use sola_browser::cef::engine::CefFrame;
use sola_browser::engine::{AbsorbDamage, FrameReceiver};

#[derive(Default)]
pub struct BoardFrames {
    pending: Mutex<HashMap<u64, CefFrame>>,
    painted: Mutex<HashSet<u64>>,
    /// Board tabs the app has open. Late frames from closed tabs drop.
    open: Mutex<HashSet<u64>>,
    /// A wake is queued in iced; further frames only update `pending`.
    wake_queued: AtomicBool,
}

impl BoardFrames {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn open(&self, tab: u64) {
        self.open.lock().unwrap().insert(tab);
    }

    /// Closed tab: stop accepting frames; drop its pending frame and mark.
    pub fn close(&self, tab: u64) {
        self.open.lock().unwrap().remove(&tab);
        self.pending.lock().unwrap().remove(&tab);
        self.painted.lock().unwrap().remove(&tab);
    }

    /// Keep the newest frame per open tab; a replaced frame donates its
    /// damage. False when the tab is not open (nothing to wake for).
    pub fn push(&self, tab: u64, mut frame: CefFrame) -> bool {
        if !self.open.lock().unwrap().contains(&tab) {
            return false;
        }
        let mut pending = self.pending.lock().unwrap();
        if let Some(old) = pending.remove(&tab) {
            frame.absorb_damage(&old);
        }
        pending.insert(tab, frame);
        self.painted.lock().unwrap().insert(tab);
        true
    }

    pub fn take_all(&self) -> HashMap<u64, CefFrame> {
        std::mem::take(&mut *self.pending.lock().unwrap())
    }

    /// The board has delivered at least one frame.
    pub fn is_painted(&self, tab: u64) -> bool {
        self.painted.lock().unwrap().contains(&tab)
    }

    /// iced handled the wake; the next frame may wake again.
    pub fn wake_done(&self) {
        self.wake_queued.store(false, Ordering::Release);
    }

    fn claim_wake(&self) -> bool {
        self.wake_queued
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }
}

/// Emits `on_frame` when frames are waiting for the plane.
pub fn subscription<M: Clone + Send + 'static>(
    mailbox: FrameReceiver<CefFrame>,
    frames: Arc<BoardFrames>,
    on_frame: M,
) -> Subscription<M> {
    subscription::from_recipe(FrameRecipe {
        mailbox,
        frames,
        on_frame,
    })
}

struct FrameRecipe<M> {
    mailbox: FrameReceiver<CefFrame>,
    frames: Arc<BoardFrames>,
    on_frame: M,
}

impl<M: Clone + Send + 'static> Recipe for FrameRecipe<M> {
    type Output = M;

    fn hash(&self, state: &mut subscription::Hasher) {
        // One frame stream per process.
        "design-frames".hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<M> {
        let Self {
            mailbox,
            frames,
            on_frame,
        } = *self;
        stream::channel(16, async move |mut output| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
            let frames_thread = frames.clone();
            std::thread::Builder::new()
                .name("design-frames".into())
                .spawn(move || {
                    while let Ok(tagged) = mailbox.recv() {
                        if !frames_thread.push(tagged.tab_id.0, tagged.frame) {
                            continue;
                        }
                        if frames_thread.claim_wake() && tx.send(()).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn design-frames thread");
            while rx.recv().await.is_some() {
                if output.send(on_frame.clone()).await.is_err() {
                    break;
                }
            }
        })
        .boxed()
    }
}
