//! sola-oath — Oath: the package ecosystem of this machine. Every package
//! here and on the origin, where each stands against its signed head and
//! the release (current, an update, a local build, not pushed, off, on the
//! origin only, with advisories over any of them), its tree up and down,
//! its weight, and what the security audit says. It runs `oath --json` as
//! home and changes nothing, but for **Add** on a package on the origin
//! only, which runs `sudo oath add`.
//!
//! The shell lists it as **Oath**, only on an Oath machine.

mod add;
mod audit;
mod check;
mod git;
mod graph;
mod heads;
mod load;
mod model;
mod oath;
mod origin;
mod standing;
mod view;
mod watch;
mod when;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use iced::widget::{Space, button, canvas, column, container, row, stack};
use iced::{Alignment, Element, Length, Padding, Subscription, Task, Theme};
use sola_bus::Message;
use sola_bus::topics::{MenuDefinition, MenuItem, Topic};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings_transparent};
use sola_kit::components::scroll;
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use audit::Report;
use check::Check;
use git::Pushed;
use model::{Ecosystem, Inputs};
use origin::Listing;
use standing::Standing;
use view::ink::Ink;
use view::mark::{self, Kind};
use view::side::Icons;
use view::{CAPTION, Filter, HEADING, PAGE, SMALL, Section, Take, View, display, ui};

const APP_ID: &str = "sola-oath";

fn item(id: &str, label: &str, shortcut: Option<sola_core::KeyChord>) -> MenuItem {
    MenuItem::Action { id: id.into(), label: label.into(), shortcut, disabled: false, checked: false }
}

fn main() -> iced::Result {
    startup(APP_ID);
    BusSetup::new(APP_ID)
        .app_kinds(&[])
        .app_menu_definition(MenuDefinition {
            label: "Oath".into(),
            items: vec![
                item("reload", "Read Again", Some(KeyCode::R.meta())),
                item("check", "Check for a Newer Release", None),
                MenuItem::Divider,
                sola_kit::menu::close_window_menu_item(),
                sola_kit::menu::quit_menu_item("Quit Oath"),
            ],
        })
        .app_menu_definition(MenuDefinition {
            label: "View".into(),
            items: vec![
                item("packages", "Packages", Some(KeyCode::KEY_1.chord())),
                item("map", "Map", Some(KeyCode::KEY_2.chord())),
                item("tree", "Tree", Some(KeyCode::KEY_3.chord())),
            ],
        })
        .install();

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}

struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    width: f32,
    icons: Icons,

    inputs: Option<Inputs>,
    pushed: BTreeMap<(String, String), Pushed>,
    report: Option<Result<Report, String>>,
    listing: Option<Result<Listing, String>>,
    check: Option<Check>,
    eco: Option<Ecosystem>,
    reading: bool,
    /// An apply landed while reading: read once more after.
    again: bool,
    checking: bool,
    /// The package being added.
    adding: Option<String>,
    /// What came of the last Add.
    added: Option<add::Outcome>,
    /// What adding each package on the origin only would make, by name,
    /// and the ones being asked about.
    previews: BTreeMap<String, add::Preview>,
    previewing: BTreeSet<String>,

    view: View,
    take: Take,
    disk: bool,
    sel: Option<String>,
    filter: Filter,
    query: String,
    open_categories: BTreeSet<String>,
    open_sections: BTreeSet<Section>,
    /// The packages the tree came through, oldest first.
    trail: Vec<String>,
    /// Every advisory in the sheet, not only the worst few.
    all_hits: bool,
    /// Reads that died, in words.
    failed: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    WindowReady(Option<iced::window::Id>),
    Resized(f32),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,

    Read,
    Applied,
    Local(Box<Inputs>),
    /// A read died (a panic): said in the list, and reading stops.
    Failed(String),
    Pushed(BTreeMap<(String, String), Pushed>),
    Audit(Result<Report, String>),
    Listing(Result<Listing, String>),
    Check,
    Checked(Box<Check>),
    Add(String),
    Added(Box<add::Outcome>),
    Previewed(String, Result<add::Preview, String>),

    Show(View),
    Take(Take),
    Disk(bool),
    Filter(Filter),
    Query(String),
    Select(String),
    Deselect,
    TreeTo(String),
    ToggleCategory(String),
    ToggleSection(Section),
    AllHits,
    Open(String),
}

/// Run `f` off the UI thread; `what` names it if it fails.
fn blocking<T: Send + 'static>(
    what: &'static str,
    f: impl FnOnce() -> T + Send + 'static,
    msg: impl Fn(T) -> Msg + Send + 'static,
) -> Task<Msg> {
    Task::perform(async move { tokio::task::spawn_blocking(f).await }, move |r| match r {
        Ok(v) => msg(v),
        Err(e) => {
            tracing::error!(what, error = %e, "a read failed");
            Msg::Failed(format!("{what}: {e}"))
        }
    })
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let app = App {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            width: 2420.0,
            icons: Icons::load(),
            inputs: None,
            pushed: BTreeMap::new(),
            report: None,
            listing: None,
            check: check::last(),
            eco: None,
            reading: true,
            again: false,
            checking: false,
            adding: None,
            added: None,
            previews: BTreeMap::new(),
            previewing: BTreeSet::new(),
            view: View::Packages,
            take: Take::Strata,
            disk: true,
            sel: None,
            filter: Filter::All,
            query: String::new(),
            open_categories: BTreeSet::new(),
            open_sections: BTreeSet::from([Section::Where, Section::Tree, Section::Security]),
            trail: Vec::new(),
            all_hits: false,
            failed: Vec::new(),
        };
        let tasks = Task::batch([
            sola_kit::window_ready_task(Msg::WindowReady),
            blocking("the catalog", load::local, |i| Msg::Local(Box::new(i))),
            blocking("the audit", load::audit, Msg::Audit),
        ]);
        (app, tasks)
    }

    fn title(&self) -> String {
        "Oath".into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.chrome_any(), &self.theme)
    }

    fn rebuild(&mut self) {
        let Some(inputs) = &self.inputs else {
            return;
        };
        let mut inputs = inputs.clone();
        inputs.problems.extend(self.failed.iter().cloned());
        if let Some(Err(e)) = &self.report {
            inputs.problems.push(format!("The security audit: {e}"));
        }
        if let Some(Err(e)) = &self.listing {
            inputs.problems.push(format!("The origin's listing: {e}"));
        }
        let report = self.report.as_ref().and_then(|r| r.as_ref().ok());
        let listing = self.listing.as_ref().and_then(|l| l.as_ref().ok());
        let eco = Ecosystem::build(&inputs, report, listing, self.check.as_ref(), &self.pushed);
        if self.sel.as_ref().is_some_and(|s| eco.get(s).is_none()) {
            self.sel = None;
        }
        self.eco = Some(eco);
    }

    /// The package the sheet shows when none is picked: the first that
    /// needs a look, else the desk.
    fn shown(&self) -> Option<String> {
        if let Some(s) = &self.sel {
            return Some(s.clone());
        }
        let e = self.eco.as_ref()?;
        let first = |f: &dyn Fn(&model::Package) -> bool| e.pkgs.values().find(|p| f(p)).map(|p| p.name.clone());
        first(&|p| p.standing == Standing::NotPushed)
            .or_else(|| first(&|p| p.advised()))
            .or_else(|| first(&|p| p.standing == Standing::Update))
            .or_else(|| e.get("sola").map(|p| p.name.clone()))
            .or_else(|| e.pkgs.keys().next().cloned())
    }

    /// Ask Oath what adding `name` would make, once, if it is on the
    /// origin only and not a recipe.
    fn preview(&mut self, name: &str) -> Task<Msg> {
        let addable = self.eco.as_ref().and_then(|e| e.get(name)).is_some_and(|p| {
            p.standing == Standing::OriginOnly && !p.listed.as_ref().is_some_and(|l| l.recipe)
        });
        if !addable || self.previews.contains_key(name) || !self.previewing.insert(name.to_string()) {
            return Task::none();
        }
        let n = name.to_string();
        blocking("oath add (preview)", move || add::preview(&n), {
            let name = name.to_string();
            move |r| Msg::Previewed(name.clone(), r)
        })
    }

    fn read(&mut self) -> Task<Msg> {
        if self.reading {
            self.again = true;
            return Task::none();
        }
        self.reading = true;
        blocking("the catalog", load::local, |i| Msg::Local(Box::new(i)))
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                if let Some(Topic::MenuAction(p)) = Topic::parse(&message)
                    && p.app_id == APP_ID
                {
                    return match p.action_id.as_str() {
                        "reload" => self.update(Msg::Read),
                        "check" => self.update(Msg::Check),
                        "packages" => self.update(Msg::Show(View::Packages)),
                        "map" => self.update(Msg::Show(View::Map)),
                        "tree" => self.update(Msg::Show(View::Tree)),
                        _ => Task::none(),
                    };
                }
            }
            Msg::WindowReady(id) => {
                self.window_id = id;
                if let Some(id) = id {
                    return iced::window::size(id).map(|s| Msg::Resized(s.width));
                }
            }
            Msg::Resized(w) => self.width = w,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),

            Msg::Read => {
                let audit = blocking("the audit", load::audit, Msg::Audit);
                return Task::batch([self.read(), audit]);
            }
            Msg::Applied => return self.read(),
            Msg::Local(inputs) => {
                self.reading = false;
                self.failed.clear();
                let sources = inputs.sources_to_check();
                let origin = inputs.origin();
                let first = self.inputs.is_none();
                self.inputs = Some(*inputs);
                self.rebuild();
                let mut tasks = vec![blocking("git", move || load::pushed(sources), Msg::Pushed)];
                if first {
                    tasks.push(blocking("the origin's listing", move || load::listing(&origin), Msg::Listing));
                }
                if std::mem::take(&mut self.again) {
                    tasks.push(self.read());
                }
                return Task::batch(tasks);
            }
            Msg::Failed(why) => {
                self.reading = false;
                self.checking = false;
                self.adding = None;
                self.failed.push(why);
                self.rebuild();
            }
            Msg::Pushed(p) => {
                self.pushed = p;
                self.rebuild();
            }
            Msg::Audit(r) => {
                self.report = Some(r);
                self.rebuild();
            }
            Msg::Listing(l) => {
                self.listing = Some(l);
                self.rebuild();
            }
            Msg::Check => {
                if !self.checking {
                    self.checking = true;
                    return blocking("the update check", check::run, |c| Msg::Checked(Box::new(c)));
                }
            }
            Msg::Checked(c) => {
                self.checking = false;
                self.check = Some(*c);
                self.rebuild();
            }
            Msg::Add(name) => {
                if self.adding.is_none() {
                    self.adding = Some(name.clone());
                    self.added = None;
                    return blocking("oath add", move || add::run(name), |o| Msg::Added(Box::new(o)));
                }
            }
            Msg::Added(o) => {
                self.adding = None;
                let moved = o.result.as_ref().is_ok_and(|a| !a.made.is_empty());
                self.added = Some(*o);
                self.previews.clear();
                if moved {
                    return self.read();
                }
            }
            Msg::Previewed(name, r) => {
                self.previewing.remove(&name);
                match r {
                    Ok(p) => {
                        self.previews.insert(name, p);
                    }
                    // An Oath before 0995609 refuses as home: the sheet says less.
                    Err(e) => tracing::debug!(name, error = %e, "no add preview"),
                }
            }

            Msg::Show(v) => {
                if v != View::Map && self.sel.is_none() {
                    self.sel = self.shown();
                }
                if v == View::Tree && self.view != View::Tree {
                    self.trail.clear();
                }
                self.view = v;
            }
            Msg::Take(t) => self.take = t,
            Msg::Disk(d) => self.disk = d,
            Msg::Filter(f) => {
                self.filter = f;
                self.view = View::Packages;
            }
            Msg::Query(q) => self.query = q,
            Msg::Select(name) => {
                if self.sel.as_ref() != Some(&name) {
                    self.all_hits = false;
                }
                let preview = self.preview(&name);
                self.sel = Some(name);
                return preview;
            }
            Msg::AllHits => self.all_hits = true,
            Msg::Deselect => self.sel = None,
            Msg::TreeTo(name) => {
                if let Some(from) = self.sel.take()
                    && from != name
                {
                    self.trail.retain(|t| *t != name && *t != from);
                    self.trail.push(from);
                    let excess = self.trail.len().saturating_sub(6);
                    self.trail.drain(..excess);
                }
                self.all_hits = false;
                let preview = self.preview(&name);
                self.sel = Some(name);
                return preview;
            }
            Msg::ToggleCategory(c) => {
                if !self.open_categories.remove(&c) {
                    self.open_categories.insert(c);
                }
            }
            Msg::ToggleSection(s) => {
                if !self.open_sections.remove(&s) {
                    self.open_sections.insert(s);
                }
            }
            Msg::Open(url) => {
                if url.starts_with("https://") || url.starts_with("http://") {
                    sola_kit::open(&url);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let ink = Ink::of(&self.theme());
        let content: Element<'_, Msg> = match &self.eco {
            None => empty(
                if self.reading { "Reading the catalog…" } else { "Nothing to show" },
                "oath ls, oath get, oath reap",
                &ink,
            ),
            Some(e) if e.pkgs.is_empty() => {
                let why = e.problems.first().cloned().unwrap_or_else(|| "oath ls listed no packages.".into());
                empty("No packages", &why, &ink)
            }
            Some(e) => self.body(e, ink),
        };
        let bg = ink.bg;
        let page = container(content).width(Length::Fill).height(Length::Fill).style(move |_| container::Style {
            background: Some(bg.into()),
            ..Default::default()
        });
        sola_kit::window_frame(self.float.chrome_any(), "Oath", Msg::TitleDrag, Msg::TitleClose, Msg::TitleResize, page.into())
    }

    fn sheet_width(&self) -> f32 {
        (self.width * 0.314).clamp(380.0, 760.0)
    }

    fn body<'a>(&'a self, e: &'a Ecosystem, ink: Ink) -> Element<'a, Msg> {
        let side = view::side::view(e, &ink, &self.icons, self.view, self.filter);
        let shown: Option<&'a str> = self.shown().and_then(|n| e.get(&n)).map(|p| p.name.as_str());
        let sheet = || -> Element<'a, Msg> {
            match shown.and_then(|n| e.get(n)) {
                Some(p) => view::sheet::view(
                    e,
                    p,
                    ink,
                    &self.open_sections,
                    self.sheet_width(),
                    self.all_hits,
                    view::sheet::Adding {
                        now: self.adding.as_deref(),
                        last: self.added.as_ref(),
                        previews: &self.previews,
                    },
                ),
                None => Space::new().into(),
            }
        };
        match self.view {
            View::Packages => {
                let list = view::list::view(view::list::Args {
                    e,
                    ink,
                    sel: shown,
                    filter: self.filter,
                    query: &self.query,
                    open: &self.open_categories,
                    checking: self.checking,
                    room: self.width - view::SIDEBAR_W - self.sheet_width(),
                });
                row![side, list, sheet()].height(Length::Fill).into()
            }
            View::Tree => row![side, self.tree(e, ink), sheet()].height(Length::Fill).into(),
            View::Map => row![side, self.map(e, ink)].height(Length::Fill).into(),
        }
    }

    fn tree<'a>(&'a self, e: &'a Ecosystem, ink: Ink) -> Element<'a, Msg> {
        let Some(id) = self.sel.as_deref().filter(|n| e.get(n).is_some()) else {
            return empty("No package", "Pick one in Packages or on the Map.", &ink);
        };
        let (below, above) = e.reach(id);
        let mut col = column![
            row![
                display(id.to_string(), HEADING, ink.fg),
                ui(format!("{below} below, {above} above. Each column uses the one to its right."), view::BODY, ink.fg3),
            ]
            .spacing(12)
            .align_y(Alignment::End)
        ]
        .spacing(16);
        if !self.trail.is_empty() {
            let mut crumbs = row![].spacing(6).align_y(Alignment::Center);
            for (i, t) in self.trail.iter().enumerate() {
                if i > 0 {
                    crumbs = crumbs.push(ui("›", SMALL, ink.fg3));
                }
                let to = e.get(t).map(|p| (p.standing, p.advised())).unwrap_or((Standing::Current, false));
                crumbs = crumbs.push(view::sheet::chip(t.clone(), Some(to), false, &ink));
            }
            col = col.push(crumbs);
        }
        let flow = view::flow::Flow { e, id };
        let h = flow.height();
        col = col.push(canvas(flow).width(Length::Fill).height(Length::Fixed(h)));
        container(scroll::scrollable(container(col).padding(Padding { top: 22.0, right: PAGE, bottom: 40.0, left: PAGE })))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn map<'a>(&'a self, e: &'a Ecosystem, ink: Ink) -> Element<'a, Msg> {
        let blurb = match (self.take, self.disk) {
            (Take::Strata, _) => "Each column a category, each row how far it stands from the ground. Brighter blocks hold up more.",
            (Take::Weight, true) => "Area is what each package takes on disk. Solid is the build in use, hatched is older versions.",
            (Take::Weight, false) => "Area is the size of the build in use.",
        };
        let mut head = row![display("Map", HEADING, ink.fg), ui(blurb, view::BODY, ink.fg3), Space::new().width(Length::Fill)]
            .spacing(14)
            .align_y(Alignment::Center);
        if self.take == Take::Weight {
            let disk: u64 = e.pkgs.values().map(model::Package::on_disk).sum();
            let used: u64 = e.pkgs.values().map(model::Package::in_use).sum();
            head = head.push(tabs(
                vec![
                    (format!("On disk {}", view::words::size(disk)), Msg::Disk(true), self.disk),
                    (format!("In use {}", view::words::size(used)), Msg::Disk(false), !self.disk),
                ],
                &ink,
            ));
        }
        head = head.push(tabs(
            vec![
                ("Strata".into(), Msg::Take(Take::Strata), self.take == Take::Strata),
                ("Weight".into(), Msg::Take(Take::Weight), self.take == Take::Weight),
            ],
            &ink,
        ));
        let sel = self.sel.as_deref();
        let stage: Element<'a, Msg> = match self.take {
            Take::Strata => canvas(view::strata::Strata { e, sel }).width(Length::Fill).height(Length::Fill).into(),
            Take::Weight => {
                canvas(view::weight::Weight { e, sel, disk: self.disk }).width(Length::Fill).height(Length::Fill).into()
            }
        };
        let mut layers = stack![container(stage).padding(Padding { top: 0.0, right: PAGE, bottom: 16.0, left: PAGE })];
        if let Some(p) = sel.and_then(|n| e.get(n)) {
            layers = layers.push(
                container(view::card::view(e, p, ink))
                    .width(Length::Fill)
                    .align_x(Alignment::End)
                    .padding(Padding { top: 12.0, right: PAGE, bottom: 0.0, left: 0.0 }),
            );
        }
        column![
            container(head).padding(Padding { top: 16.0, right: PAGE, bottom: 12.0, left: PAGE }),
            container(layers.height(Length::Fill)).height(Length::Fill),
            container(legend(&ink)).padding(Padding { top: 0.0, right: PAGE, bottom: 14.0, left: PAGE }),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            watch::applies().map(|()| Msg::Applied),
            iced::window::resize_events().map(|(_, s)| Msg::Resized(s.width)),
        ])
    }
}

fn tabs<'a>(items: Vec<(String, Msg, bool)>, ink: &Ink) -> Element<'a, Msg> {
    let (face, on) = (ink.face2, ink.face3);
    let (fg, fg2) = (ink.fg, ink.fg2);
    let buttons = items.into_iter().map(|(label, msg, active)| {
        button(container(ui(label, SMALL, if active { fg } else { fg2 })).height(Length::Fixed(24.0)).align_y(Alignment::Center))
            .padding(Padding::from([0, 10]))
            .style(move |_, status| button::Style {
                background: (active || matches!(status, button::Status::Hovered)).then(|| on.into()),
                border: iced::Border { radius: 5.0.into(), ..Default::default() },
                ..Default::default()
            })
            .on_press(msg)
            .into()
    });
    container(row(buttons).spacing(2))
        .padding(2)
        .style(move |_| container::Style {
            background: Some(face.into()),
            border: iced::Border { radius: 7.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn legend<'a>(ink: &Ink) -> Element<'a, Msg> {
    let item = |m: Element<'a, Msg>, label: &'a str| -> Element<'a, Msg> {
        row![m, ui(label, SMALL, ink.fg2)].spacing(7).align_y(Alignment::Center).into()
    };
    let bar = |c: iced::Color| -> Element<'a, Msg> { mark::led(c, None, 18.0, 1.5) };
    row![
        item(mark::standing(Standing::Local), "Local build"),
        item(mark::standing(Standing::NotPushed), "Not pushed"),
        item(mark::standing(Standing::Update), "Update"),
        item(mark::mark(Kind::Advisory), "Advisory"),
        item(mark::standing(Standing::Off), "Off"),
        item(mark::standing(Standing::OriginOnly), "On origin only"),
        item(bar(ink.up), "Uses it"),
        item(bar(ink.accent), "It needs"),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}

fn empty<'a>(title: &'a str, caption: &str, ink: &Ink) -> Element<'a, Msg> {
    container(
        column![ui(title, view::TITLE, ink.fg2), ui(caption.to_string(), CAPTION, ink.fg3)]
            .spacing(6)
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}
