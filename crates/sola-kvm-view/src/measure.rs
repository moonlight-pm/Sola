//! `--measure <pictures>`: the stream with no window. Decodes as the window
//! would, prints each second of pictures and then all of them, and exits.

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc;

use crate::net::{self, Event};
use crate::pictures::Pictures;
use crate::stats::Stats;

pub fn run(target: Result<(String, u16), String>, wanted: u32) -> ExitCode {
    let (host, port) = match target {
        Ok(target) => target,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    let pictures = Arc::new(Pictures::with_stats(Stats::whole_run()));
    let (tx, rx) = mpsc::channel();
    net::start(
        host,
        port,
        Arc::clone(&pictures),
        Arc::new(move |event| {
            let _ = tx.send(event);
        }),
    );
    let mut printed = 0;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Picture => {
                if let Some(frame) = pictures.take() {
                    pictures.give_back(frame);
                }
                pictures.drawn();
                if let (rolls, Some(second)) = pictures.stats.last() {
                    if rolls > printed {
                        printed = rolls;
                        println!("{}", second.line());
                    }
                }
                if let Some(all) = pictures.stats.run().filter(|all| all.pictures >= wanted) {
                    println!("all {} pictures: {}", all.pictures, all.line());
                    return ExitCode::SUCCESS;
                }
            }
            Event::Lost(why) => eprintln!("lost the host: {why}; connecting again"),
            Event::Refused(why) => {
                eprintln!("{why}");
                return ExitCode::FAILURE;
            }
            Event::Ready(_) | Event::Connected(_) | Event::Cursor => {}
        }
    }
    ExitCode::FAILURE
}
