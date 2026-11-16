//! Sola's Signal client. For now a spike with no window: `link` joins the
//! phone's account as a linked device, `receive` prints what arrives, `send`
//! sends one text.

mod account;
mod devicename;
mod engine;
mod http;
mod keys;
mod link;
mod net;
mod proto;
mod receive;
mod send;
mod store;

use anyhow::{Context, Result, anyhow, bail};

use crate::net::Net;
use crate::store::Store;

const USAGE: &str = "usage: sola-signal [link [device name] | status]";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    sola_core::log::init("sola-signal");
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sola-signal: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: &[String]) -> Result<()> {
    let store = Store::open(&Store::path())?;
    match args.first().map(String::as_str) {
        Some("link") => link(&store, args.get(1).map_or("Sola", String::as_str)).await,
        Some("status") => status(&store),
        Some("run") => headless(store, args.get(1).and_then(|s| s.parse().ok()).unwrap_or(u64::MAX)).await,
        _ => bail!("{USAGE}"),
    }
}

/// One process signs in at a time: the server closes a device's connection
/// when it opens another, and two would write the same sessions.
fn alone() -> Result<std::fs::File> {
    let lock = std::fs::File::create(Store::path().with_extension("lock")).context("the lock file")?;
    lock.try_lock().map_err(|_| anyhow!("another sola-signal is signed in (`receive`?); stop it first"))?;
    Ok(lock)
}

async fn link(store: &Store, device_name: &str) -> Result<()> {
    let _alone = alone()?;
    if store.account()?.is_some() {
        bail!("already linked ({}); unlink it on the phone and remove that file first", Store::path().display());
    }
    let linked = link::link(&Net::new(), device_name, |url| {
        println!("{}\n", qr(url));
        println!("On the phone: Signal > Settings > Linked devices > Link a new device, and scan this.");
        println!("{url}");
    })
    .await?;
    store.save_link(&linked).context("keeping the link")?;
    println!("Linked as device {} of {}.", linked.account.device_id, linked.account.aci.service_id_string());
    Ok(())
}

/// The engine with no window, for a while: what it does is in the log and
/// the store. Events are printed without what was said or by whom.
async fn headless(store: Store, seconds: u64) -> Result<()> {
    let _alone = alone()?;
    let _engine = engine::start(store, |event| match event {
        engine::Event::Arrived { conversation, .. } => println!("Arrived in {conversation}"),
        engine::Event::Typing { conversation, started, .. } => println!("Typing in {conversation}: {started}"),
        engine::Event::LinkCode(_) => println!("LinkCode"),
        other => println!("{other:?}"),
    });
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    Ok(())
}

fn status(store: &Store) -> Result<()> {
    match store.account()? {
        Some(a) => println!(
            "linked: device {} of {} ({})",
            a.device_id,
            a.aci.service_id_string(),
            a.number.as_deref().unwrap_or("no phone number"),
        ),
        None => println!("not linked"),
    }
    let people = store.recipients()?;
    let named = people.iter().filter(|person| person.contact_name.is_some() || person.profile_name.is_some()).count();
    let keyed = people.iter().filter(|person| person.profile_key.is_some()).count();
    println!("people: {} ({named} with a name, {keyed} with a profile key)", people.len());
    println!("conversations shown: {}, unread messages: {}", store.chats()?.len(), store.unread_total()?);
    Ok(())
}

/// The URL as a QR code in half-block characters.
fn qr(url: &str) -> String {
    use qrcode::render::unicode::Dense1x2;
    match qrcode::QrCode::new(url) {
        Ok(code) => code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
        Err(e) => format!("(no QR code: {e})"),
    }
}
