//! A letter's pictures from the web, fetched only when the reader asks
//! (Load Images): a few threads, each picture bounded in time and size,
//! each sent to the UI as it arrives. No cookies are kept and nothing is
//! sent but the GET.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::cmds::MailEvent;
use crate::bridge;
use crate::protocol::MailId;
use crate::protocol::picture::{self, MAX_BYTES};

/// At most this many pictures per letter.
const MAX_PICTURES: usize = 60;
const THREADS: usize = 4;
const TIMEOUT: Duration = Duration::from_secs(15);

pub fn spawn(id: MailId, urls: Vec<String>) {
    let queue = Arc::new(Mutex::new(
        // Popped from the end: the letter's first pictures come first.
        urls.into_iter().take(MAX_PICTURES).rev().collect::<Vec<_>>(),
    ));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(5)
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) Sola Mail")
        .build()
        .into();
    // The last thread out says the letter is done.
    let running = Arc::new(AtomicUsize::new(THREADS));
    let finished = move |running: &AtomicUsize, id: &MailId| {
        if running.fetch_sub(1, Ordering::AcqRel) == 1 {
            bridge::emit(MailEvent::ImagesDone(id.clone()));
        }
    };
    for n in 0..THREADS {
        let queue = queue.clone();
        let agent = agent.clone();
        let letter = id.clone();
        let running_t = running.clone();
        let started = std::thread::Builder::new()
            .name(format!("sola-mail-img-{n}"))
            .spawn(move || {
                loop {
                    let Some(url) = queue.lock().ok().and_then(|mut q| q.pop()) else {
                        break;
                    };
                    match fetch(&agent, &url) {
                        Ok(bytes) => match picture::decode(url.clone(), &bytes) {
                            Some(image) => bridge::emit(MailEvent::Image {
                                id: letter.clone(),
                                image: Box::new(image),
                            }),
                            None => tracing::debug!(url = url.as_str(), "picture didn't decode"),
                        },
                        Err(e) => tracing::debug!(url = url.as_str(), "picture fetch failed: {e}"),
                    }
                }
                finished(&running_t, &letter);
            });
        if let Err(e) = started {
            tracing::warn!("couldn't start a picture thread: {e}");
            finished(&running, &id);
        }
    }
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, ureq::Error> {
    let mut response = agent.get(url).call()?;
    response.body_mut().with_config().limit(MAX_BYTES).read_to_vec()
}
