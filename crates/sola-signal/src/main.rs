//! Sola's Signal client. For now a spike with no window: `link` joins the
//! phone's account as a linked device.

mod account;
mod devicename;
mod link;
mod net;
mod proto;
mod store;

use anyhow::{Context, Result, bail};

use crate::net::Net;
use crate::store::Store;

const USAGE: &str = "usage: sola-signal link [device name] | status";

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
        _ => bail!("{USAGE}"),
    }
}

async fn link(store: &Store, device_name: &str) -> Result<()> {
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
