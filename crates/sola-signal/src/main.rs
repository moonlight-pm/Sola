//! Sola's Signal client: a linked device of the phone, in a window of its
//! own. `sola-signal` opens it; `status` says what is linked; `link` joins
//! an account from a terminal; `run` is the engine with no window.

mod account;
mod demo;
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
mod ui;

use anyhow::{Context, Result, anyhow, bail};

use crate::net::Net;
use crate::store::Store;

const USAGE: &str = "usage: sola-signal [link [device name] | status | selftest | run [seconds] | demo]";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return window();
    }
    sola_core::log::init("sola-signal");
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    match runtime.block_on(run(&args)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sola-signal: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The app: the engine on its thread, the window on this one.
fn window() -> std::process::ExitCode {
    use sola_bus::topics::{MenuDefinition, MenuItem, TopicKind};
    use sola_core::KeyCode;
    use sola_kit::app::{BusSetup, startup, window_settings_transparent};

    startup(ui::APP_ID);
    let opened = alone().and_then(|lock| Ok((lock, Store::open(&Store::path())?)));
    let (_alone, store) = match opened {
        Ok(opened) => opened,
        Err(e) => {
            tracing::error!("{e:#}");
            eprintln!("sola-signal: {e:#}");
            return std::process::ExitCode::FAILURE;
        }
    };
    ui::bridge::start(store);

    let item = |id: &str, label: &str, shortcut| MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    };
    let menu = |label: &str, items| MenuDefinition { label: label.into(), items };
    let timer = |seconds: u32, label: &str| item(&format!("timer:{seconds}"), label, None);
    BusSetup::new(ui::APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowPlace,
        ])
        .app_menu_definition(menu(
            "Signal",
            vec![
                item("connect", "Connect Now", None),
                MenuItem::Divider,
                sola_kit::menu::close_window_menu_item(),
                sola_kit::menu::quit_menu_item("Quit Signal"),
            ],
        ))
        .app_menu_definition(menu(
            "Edit",
            vec![
                item("cut", "Cut", Some(KeyCode::X.meta())),
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("paste", "Paste", Some(KeyCode::V.meta())),
                MenuItem::Divider,
                item("select_all", "Select All", Some(KeyCode::A.meta())),
                MenuItem::Divider,
                item("find", "Search", Some(KeyCode::F.meta())),
            ],
        ))
        .app_menu_definition(menu(
            "Conversation",
            vec![
                item("attach", "Attach Files…", Some(KeyCode::A.meta_shift())),
                item("alias", "Set Alias…", None),
                MenuItem::Divider,
                timer(0, "Messages Do Not Disappear"),
                timer(30, "Disappear After 30 Seconds"),
                timer(300, "Disappear After 5 Minutes"),
                timer(3600, "Disappear After 1 Hour"),
                timer(28_800, "Disappear After 8 Hours"),
                timer(86_400, "Disappear After 1 Day"),
                timer(604_800, "Disappear After 1 Week"),
                timer(2_419_200, "Disappear After 4 Weeks"),
            ],
        ))
        .install();

    let ran = iced::application(ui::App::boot, ui::App::update, ui::App::view)
        .title(ui::App::title)
        .subscription(ui::App::subscription)
        .theme(ui::App::theme)
        .default_font(sola_kit::fonts::ui())
        .window(window_settings_transparent(ui::APP_ID))
        .run();
    match ran {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("the window: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: &[String]) -> Result<()> {
    let store = Store::open(&Store::path())?;
    match args.first().map(String::as_str) {
        Some("link") => link(&store, args.get(1).map_or("Sola", String::as_str)).await,
        Some("status") => status(&store),
        Some("demo") => demo::seed(&store),
        Some("selftest") => {
            let _alone = alone()?;
            tokio::task::LocalSet::new().run_until(engine::selftest(store)).await
        }
        Some("run") => headless(store, args.get(1).and_then(|s| s.parse().ok()).unwrap_or(u64::MAX)).await,
        _ => bail!("{USAGE}"),
    }
}

/// One process signs in at a time: the server closes a device's connection
/// when it opens another, and two would write the same sessions.
fn alone() -> Result<std::fs::File> {
    let path = Store::path().with_extension("lock");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    let lock = std::fs::File::create(&path).context("the lock file")?;
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
