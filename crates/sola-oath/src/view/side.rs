//! The sidebar: the three views, then the standings as filters with their
//! counts, then where the facts came from and when.

use iced::widget::{column, container, svg};
use iced::{Element, Length, Padding};
use sola_kit::components::icon::icon_svg;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};

use super::ink::Ink;
use super::mark::{self, Kind};
use super::{CAPTION, Filter, SIDEBAR_W, View, ui};
use crate::model::Ecosystem;
use crate::standing::Standing;
use crate::{Msg, when};

pub struct Icons {
    pub list: svg::Handle,
    pub map: svg::Handle,
    pub tree: svg::Handle,
}

impl Icons {
    pub fn load() -> Self {
        let h = sola_kit::components::icon::icon_handle;
        Icons { list: h("lucide/list"), map: h("lucide/chart-column"), tree: h("lucide/git-fork") }
    }
}

fn filter_mark<'a>(f: Filter) -> Element<'a, Msg> {
    match f {
        Filter::All => mark::standing(Standing::Current),
        Filter::Is(s) => mark::standing(s),
        Filter::Advisories => mark::mark(Kind::Advisory),
        Filter::BehindUpstream | Filter::NotAssessed | Filter::NotInRelease => mark::mark(Kind::Unknown),
    }
}

pub fn view<'a>(e: &Ecosystem, ink: &Ink, icons: &Icons, view: View, filter: Filter) -> Element<'a, Msg> {
    let nav = [
        (View::Packages, "Packages", &icons.list),
        (View::Map, "Map", &icons.map),
        (View::Tree, "Tree", &icons.tree),
    ]
    .into_iter()
    .map(|(v, label, icon)| {
        SidebarItem::new(label, Msg::Show(v)).leading(icon_svg(icon.clone(), 15)).active(view == v)
    })
    .collect();
    let filters = Filter::ALL
        .into_iter()
        .filter_map(|f| {
            let n = e.count(|p| f.keeps(p));
            // A filter that keeps nothing stays out of the way, but for
            // the ones that say something is fine.
            if n == 0 && !matches!(f, Filter::All | Filter::Is(Standing::NotPushed) | Filter::Advisories) {
                return None;
            }
            Some(
                SidebarItem::new(f.label(), Msg::Filter(f))
                    .leading(filter_mark(f))
                    .secondary(n.to_string())
                    .active(view == View::Packages && filter == f),
            )
        })
        .collect();
    let mut foot = column![].spacing(2);
    if let Some(r) = &e.release {
        foot = foot.push(ui(format!("Release {} · {}", r.output, when::ago(&r.published)), CAPTION, ink.fg3));
    }
    if let Some((origin, _)) = &e.listing {
        foot = foot.push(ui(format!("Origin {}", origin.trim_start_matches("https://")), CAPTION, ink.fg3));
    }
    foot = foot.push(ui(format!("Read {}", when::ago(&e.read_at)), CAPTION, ink.fg3));
    let bg = ink.raised;
    let rule = ink.rule;
    container(
        column![
            sidebar(vec![SidebarSection::unlabeled(nav), SidebarSection::new("Standing", filters)]).height(Length::Fill),
            container(foot).padding(Padding { top: 0.0, right: 16.0, bottom: 14.0, left: 18.0 }),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(SIDEBAR_W))
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(bg.into()),
        border: iced::Border { color: rule, width: 0.0, ..Default::default() },
        ..Default::default()
    })
    .into()
}
