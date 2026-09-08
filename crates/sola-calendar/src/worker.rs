//! Background sync: local store + Google API + iCloud CalDAV.

use std::time::Duration;

use chrono::NaiveDate;
use sola_bus::topics::{CalendarAccount, CalendarAccountKind, CalendarConfig};
use sola_core::Encrypted;

use crate::bridge;
use crate::model::{
    Account, CalEvent, CalKind, Settings, Store, LOCAL_CAL_ID, new_id, next_palette,
};
use crate::paths::AppDirs;
use crate::{apple, google, store};

#[derive(Debug, Clone)]
pub enum CalCmd {
    Boot,
    Sync { start: NaiveDate, end: NaiveDate },
    SaveEvent(CalEvent),
    DeleteEvent(String),
    SetVisible { id: String, visible: bool },
    ApplyConfig(CalendarConfig),
    SaveSettings(Settings),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum CalNotice {
    Snapshot(Store),
    Settings(Settings),
    Status(String),
    Error(String),
    Toast(String),
}

pub fn start() {
    std::thread::Builder::new()
        .name("sola-calendar-worker".into())
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("sola-calendar-rt")
                .build()
                .expect("calendar tokio runtime");
            rt.block_on(run());
        })
        .expect("spawn calendar worker");
}

struct State {
    dirs: AppDirs,
    store: Store,
    settings: Settings,
    http: reqwest::Client,
    window: (NaiveDate, NaiveDate),
}

async fn run() {
    let dirs = AppDirs::discover();
    if let Err(e) = dirs.ensure() {
        tracing::warn!("calendar dirs: {e}");
    }
    let http = reqwest::Client::builder()
        .user_agent("sola-calendar/0.1")
        .timeout(Duration::from_secs(45))
        .build()
        .expect("http");
    let mut state = State {
        store: store::load_store(&dirs),
        settings: store::load_settings(&dirs),
        dirs,
        http,
        window: default_window(),
    };
    bridge::emit(CalNotice::Settings(state.settings.clone()));
    bridge::emit(CalNotice::Snapshot(state.store.clone()));

    let cmd_rx = bridge::take_cmd_rx();
    loop {
        let cmd = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => break,
        };
        match cmd {
            CalCmd::Shutdown => break,
            CalCmd::Boot => {
                sync_all(&mut state).await;
            }
            CalCmd::Sync { start, end } => {
                state.window = (start, end);
                sync_all(&mut state).await;
            }
            CalCmd::SaveSettings(s) => {
                state.settings = s;
                let _ = store::save_settings(&state.dirs, &state.settings);
            }
            CalCmd::SetVisible { id, visible } => {
                if let Some(cal) = state.store.calendars.iter_mut().find(|c| c.id == id) {
                    cal.visible = visible;
                }
                persist(&state);
            }
            CalCmd::SaveEvent(event) => save_event(&mut state, event).await,
            CalCmd::DeleteEvent(id) => delete_event(&mut state, &id).await,
            CalCmd::ApplyConfig(cfg) => apply_config(&mut state, cfg).await,
        }
    }
}

fn default_window() -> (NaiveDate, NaiveDate) {
    let today = crate::timeutil::today();
    let start = crate::timeutil::first_of_month(today)
        .checked_sub_months(chrono::Months::new(1))
        .unwrap_or(today);
    let end = crate::timeutil::first_of_month(today)
        .checked_add_months(chrono::Months::new(2))
        .unwrap_or(today);
    (start, end)
}

fn persist(state: &State) {
    if let Err(e) = store::save_store(&state.dirs, &state.store) {
        bridge::emit(CalNotice::Error(format!("could not save calendar: {e}")));
        return;
    }
    bridge::emit(CalNotice::Snapshot(state.store.clone()));
}

async fn sync_all(state: &mut State) {
    bridge::emit(CalNotice::Status("Syncing…".into()));
    let accounts = state.store.accounts.clone();
    for account in accounts {
        match account.kind {
            CalKind::Google => {
                if let Err(e) = sync_google(state, &account.id).await {
                    bridge::emit(CalNotice::Error(format!("Google: {e}")));
                }
            }
            CalKind::Apple => {
                if let Err(e) = sync_apple(state, &account.id).await {
                    bridge::emit(CalNotice::Error(format!("iCloud: {e}")));
                }
            }
            CalKind::Local => {}
        }
    }
    persist(state);
    bridge::emit(CalNotice::Status("Synced".into()));
}

async fn google_token(state: &mut State, account_id: &str) -> anyhow::Result<String> {
    let idx = state
        .store
        .accounts
        .iter()
        .position(|a| a.id == account_id)
        .ok_or_else(|| anyhow::anyhow!("Google account gone"))?;
    let account = &state.store.accounts[idx];
    let mut access = account.access();
    let refresh = account.refresh();
    let client_id = account.google_client_id.clone();
    if access.is_empty() {
        anyhow::bail!("not signed in");
    }
    if google::access_expired(account.expiry_unix) && !refresh.is_empty() {
        let tok = google::refresh(&state.http, &client_id, &refresh).await?;
        let acc = &mut state.store.accounts[idx];
        acc.access_token = Some(Encrypted(tok.access_token.clone()));
        if let Some(r) = tok.refresh_token.filter(|s| !s.is_empty()) {
            acc.refresh_token = Some(Encrypted(r));
        }
        acc.expiry_unix = google::expiry_unix(tok.expires_in);
        access = tok.access_token;
        let _ = store::save_store(&state.dirs, &state.store);
    }
    Ok(access)
}

async fn sync_google(state: &mut State, account_id: &str) -> anyhow::Result<()> {
    let token = google_token(state, account_id).await?;
    let listed = google::list_calendars(&state.http, &token, account_id).await?;
    merge_calendars(&mut state.store, account_id, listed);
    let cals: Vec<_> = state
        .store
        .calendars
        .iter()
        .filter(|c| c.account_id.as_deref() == Some(account_id))
        .cloned()
        .collect();
    for cal in cals {
        match google::list_events(&state.http, &token, &cal, state.window).await {
            Ok(events) => replace_calendar_events(&mut state.store, &cal.id, events),
            Err(e) => {
                tracing::warn!("google events {}: {e}", cal.name);
                bridge::emit(CalNotice::Error(format!("{}: {e}", cal.name)));
            }
        }
    }
    Ok(())
}

async fn sync_apple(state: &mut State, account_id: &str) -> anyhow::Result<()> {
    let account = state
        .store
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("iCloud account gone"))?;
    let password = account.secret();
    if password.is_empty() {
        anyhow::bail!("missing app-specific password");
    }
    let cals: Vec<_> = state
        .store
        .calendars
        .iter()
        .filter(|c| c.account_id.as_deref() == Some(account_id))
        .cloned()
        .collect();
    for cal in cals {
        match apple::fetch_events(
            &state.http,
            &account.apple_id,
            &password,
            &cal,
            state.window,
        )
        .await
        {
            Ok(events) => replace_calendar_events(&mut state.store, &cal.id, events),
            Err(e) => {
                tracing::warn!("icloud events {}: {e}", cal.name);
                bridge::emit(CalNotice::Error(format!("{}: {e}", cal.name)));
            }
        }
    }
    Ok(())
}

fn merge_calendars(store: &mut Store, account_id: &str, incoming: Vec<crate::model::Calendar>) {
    for mut cal in incoming {
        if let Some(existing) = store.calendars.iter_mut().find(|c| {
            c.account_id.as_deref() == Some(account_id)
                && c.remote_id == cal.remote_id
                && c.kind == cal.kind
        }) {
            existing.name = cal.name;
            existing.color = cal.color;
            existing.read_only = cal.read_only;
            existing.href = cal.href;
        } else {
            if store.calendars.iter().any(|c| c.id == cal.id) {
                cal.id = new_id("cal");
            }
            if cal.color.is_empty() {
                cal.color = next_palette(&store.calendars);
            }
            store.calendars.push(cal);
        }
    }
}

fn replace_calendar_events(store: &mut Store, calendar_id: &str, events: Vec<CalEvent>) {
    store.events.retain(|e| e.calendar_id != calendar_id);
    store.events.extend(events);
}

async fn save_event(state: &mut State, mut event: CalEvent) {
    let Some(cal) = state.store.calendar(&event.calendar_id).cloned() else {
        bridge::emit(CalNotice::Error("that calendar is gone".into()));
        return;
    };
    if cal.read_only {
        bridge::emit(CalNotice::Error("this calendar is read-only".into()));
        return;
    }
    match cal.kind {
        CalKind::Local => {
            upsert_event(&mut state.store, event);
            persist(state);
            bridge::emit(CalNotice::Toast("Saved".into()));
        }
        CalKind::Google => {
            let Some(account_id) = cal.account_id.clone() else {
                return;
            };
            match google_token(state, &account_id).await {
                Ok(token) => {
                    let result = if event.remote_id.is_some() {
                        google::patch_event(&state.http, &token, &cal, &event).await
                    } else {
                        google::insert_event(&state.http, &token, &cal, &event).await
                    };
                    match result {
                        Ok(saved) => {
                            upsert_event(&mut state.store, saved);
                            persist(state);
                            bridge::emit(CalNotice::Toast("Saved".into()));
                        }
                        Err(e) => bridge::emit(CalNotice::Error(format!("Google save: {e}"))),
                    }
                }
                Err(e) => bridge::emit(CalNotice::Error(format!("Google: {e}"))),
            }
        }
        CalKind::Apple => {
            let Some(account) = state
                .store
                .accounts
                .iter()
                .find(|a| Some(a.id.as_str()) == cal.account_id.as_deref())
                .cloned()
            else {
                return;
            };
            match apple::put_event(
                &state.http,
                &account.apple_id,
                &account.secret(),
                &cal,
                &event,
            )
            .await
            {
                Ok(href) => {
                    event.href = Some(href);
                    event.remote_id = event.remote_id.clone().or(Some(event.id.clone()));
                    upsert_event(&mut state.store, event);
                    persist(state);
                    bridge::emit(CalNotice::Toast("Saved".into()));
                }
                Err(e) => bridge::emit(CalNotice::Error(format!("iCloud save: {e}"))),
            }
        }
    }
}

async fn delete_event(state: &mut State, id: &str) {
    let Some(event) = state.store.events.iter().find(|e| e.id == id).cloned() else {
        return;
    };
    if event.master_id.is_some() && event.rrule.is_some() {
        bridge::emit(CalNotice::Error(
            "repeating iCloud events cannot be deleted from one day yet".into(),
        ));
        return;
    }
    let cal = state.store.calendar(&event.calendar_id).cloned();
    if let Some(cal) = cal {
        match cal.kind {
            CalKind::Google => {
                if let Some(account_id) = &cal.account_id {
                    match google_token(state, account_id).await {
                        Ok(token) => {
                            if let Err(e) =
                                google::delete_event(&state.http, &token, &cal, &event).await
                            {
                                bridge::emit(CalNotice::Error(format!("Google delete: {e}")));
                                return;
                            }
                        }
                        Err(e) => {
                            bridge::emit(CalNotice::Error(format!("Google: {e}")));
                            return;
                        }
                    }
                }
            }
            CalKind::Apple => {
                if let Some(account) = state
                    .store
                    .accounts
                    .iter()
                    .find(|a| Some(a.id.as_str()) == cal.account_id.as_deref())
                {
                    if let Err(e) = apple::delete_event(
                        &state.http,
                        &account.apple_id,
                        &account.secret(),
                        &event,
                    )
                    .await
                    {
                        bridge::emit(CalNotice::Error(format!("iCloud delete: {e}")));
                        return;
                    }
                }
            }
            CalKind::Local => {}
        }
    }
    state.store.events.retain(|e| e.id != id);
    persist(state);
    bridge::emit(CalNotice::Toast("Deleted".into()));
}

fn upsert_event(store: &mut Store, event: CalEvent) {
    if let Some(slot) = store.events.iter_mut().find(|e| e.id == event.id) {
        *slot = event;
    } else {
        store.events.push(event);
    }
}

async fn apply_config(state: &mut State, cfg: CalendarConfig) {
    let new_ids: std::collections::HashSet<String> =
        cfg.accounts.iter().map(|a| a.id.clone()).collect();
    let old_ids: Vec<String> = state.store.accounts.iter().map(|a| a.id.clone()).collect();
    for id in old_ids {
        if !new_ids.contains(&id) {
            disconnect(state, &id);
        }
    }
    state.settings.google_client_id = cfg.google_client_id.clone();
    let _ = store::save_settings(&state.dirs, &state.settings);
    state.store.accounts = cfg
        .accounts
        .iter()
        .map(|a| account_from_bus(a, &cfg.google_client_id))
        .collect();
    persist(state);
    let apple: Vec<Account> = state
        .store
        .accounts
        .iter()
        .filter(|a| a.kind == CalKind::Apple)
        .cloned()
        .collect();
    for acc in apple {
        let has_cals = state
            .store
            .calendars
            .iter()
            .any(|c| c.account_id.as_deref() == Some(acc.id.as_str()));
        if has_cals {
            continue;
        }
        match apple::discover(
            &state.http,
            &acc.apple_id,
            &acc.secret(),
            &acc.id,
        )
        .await
        {
            Ok((principal, calendars)) => {
                if let Some(slot) = state.store.accounts.iter_mut().find(|a| a.id == acc.id) {
                    slot.principal_url = principal;
                }
                merge_calendars(&mut state.store, &acc.id, calendars);
            }
            Err(e) => bridge::emit(CalNotice::Error(format!("iCloud: {e}"))),
        }
    }
    persist(state);
    sync_all(state).await;
}

fn account_from_bus(a: &CalendarAccount, google_client_id: &str) -> Account {
    Account {
        id: a.id.clone(),
        kind: match a.kind {
            CalendarAccountKind::Google => CalKind::Google,
            CalendarAccountKind::Apple => CalKind::Apple,
        },
        label: a.label.clone(),
        email: a.email.clone(),
        apple_id: a.apple_id.clone(),
        app_password: a.app_password.clone(),
        google_client_id: match a.kind {
            CalendarAccountKind::Google => google_client_id.to_string(),
            CalendarAccountKind::Apple => String::new(),
        },
        access_token: a.access_token.clone(),
        refresh_token: a.refresh_token.clone(),
        expiry_unix: a.expiry_unix,
        principal_url: String::new(),
    }
}

pub fn account_to_bus(a: &Account) -> Option<CalendarAccount> {
    let kind = match a.kind {
        CalKind::Google => CalendarAccountKind::Google,
        CalKind::Apple => CalendarAccountKind::Apple,
        CalKind::Local => return None,
    };
    Some(CalendarAccount {
        id: a.id.clone(),
        kind,
        label: a.label.clone(),
        email: a.email.clone(),
        apple_id: a.apple_id.clone(),
        app_password: a.app_password.clone(),
        access_token: a.access_token.clone(),
        refresh_token: a.refresh_token.clone(),
        expiry_unix: a.expiry_unix,
    })
}

fn disconnect(state: &mut State, account_id: &str) {
    let cal_ids: Vec<String> = state
        .store
        .calendars
        .iter()
        .filter(|c| c.account_id.as_deref() == Some(account_id))
        .map(|c| c.id.clone())
        .collect();
    state.store.events.retain(|e| !cal_ids.contains(&e.calendar_id));
    state
        .store
        .calendars
        .retain(|c| c.account_id.as_deref() != Some(account_id));
    state.store.accounts.retain(|a| a.id != account_id);
    if state.settings.default_calendar != LOCAL_CAL_ID
        && !state
            .store
            .calendars
            .iter()
            .any(|c| c.id == state.settings.default_calendar)
    {
        state.settings.default_calendar = LOCAL_CAL_ID.into();
        let _ = store::save_settings(&state.dirs, &state.settings);
    }
    persist(state);
}
