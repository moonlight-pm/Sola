//! Sola's Signal client. For now a spike with no window: `link` joins the
//! phone's account as a linked device, `receive` prints what arrives, `send`
//! sends one text.

mod account;
mod devicename;
mod keys;
mod link;
mod net;
mod proto;
mod receive;
mod send;
mod show;
mod store;

use anyhow::{Context, Result, anyhow, bail};
use libsignal_core::{Aci, ServiceId};

use crate::net::Net;
use crate::store::Store;

const USAGE: &str = "usage: sola-signal link [device name] | status | receive | send <me|account id> <text>";

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
        Some("receive") => receive(&store).await,
        Some("send") if args.len() > 2 => send(&store, &args[1], &args[2..].join(" ")).await,
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

/// Print what arrives until the socket closes.
async fn receive(store: &Store) -> Result<()> {
    let _alone = alone()?;
    let account = store.account()?.context("not linked: run `sola-signal link` first")?;
    receive::Receiver::new(store, account)?
        .run(&Net::new(), |received| println!("{}", show::line(&received)))
        .await
}

/// Send one text to `who`: `me`, or an account id as `receive` prints it.
async fn send(store: &Store, who: &str, text: &str) -> Result<()> {
    let _alone = alone()?;
    let account = store.account()?.context("not linked: run `sola-signal link` first")?;
    let to = match who {
        "me" => account.aci,
        _ => Aci::parse_from_service_id_string(who).context("who: `me`, or an account id as `receive` prints it")?,
    };
    // What the server delivers on this connection is left unanswered, so
    // it comes again to the next `receive`.
    let chat = Net::new()
        .connect(net::CHAT_WEBSOCKET_PATH, Some(net::sign_in(&account)), Box::new(|_| {}), "chat")
        .await
        .context("signing in")?;
    send::Sender::new(store, &account, &chat).text(ServiceId::from(to), text).await?;
    chat.disconnect().await;
    println!("Sent.");
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
