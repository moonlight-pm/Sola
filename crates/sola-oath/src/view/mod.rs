//! What the window shows: the sidebar, the package list, one package's
//! sheet, the map (Strata and Weight) and the tree (Flow).

pub mod card;
pub mod flow;
pub mod ink;
pub mod list;
pub mod mark;
pub mod paint;
pub mod sheet;
pub mod side;
pub mod squarify;
pub mod strata;
pub mod weight;
pub mod words;

use iced::widget::{Text, text};
use iced::{Color, Font};
use sola_kit::fonts;

use crate::model::Package;
use crate::standing::Standing;

pub const BODY: f32 = 13.0;
pub const SMALL: f32 = 12.0;
pub const CAPTION: f32 = 11.0;
pub const PROSE: f32 = 14.0;
pub const TITLE: f32 = 15.0;
pub const HEADING: f32 = 22.0;
/// Page margin.
pub const PAGE: f32 = 24.0;
pub const SIDEBAR_W: f32 = 220.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Packages,
    Map,
    Tree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    Strata,
    Weight,
}

/// The list's filters, in the sidebar's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Is(Standing),
    Advisories,
    BehindUpstream,
    NotInRelease,
    NotAssessed,
}

impl Filter {
    pub const ALL: [Filter; 11] = [
        Filter::All,
        Filter::Is(Standing::NotPushed),
        Filter::Is(Standing::Local),
        Filter::Is(Standing::Update),
        Filter::Advisories,
        Filter::BehindUpstream,
        Filter::NotInRelease,
        Filter::Is(Standing::Off),
        Filter::Is(Standing::OriginOnly),
        Filter::NotAssessed,
        Filter::Is(Standing::Current),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "Everything",
            Filter::Is(s) => s.heading(),
            Filter::Advisories => "Advisories",
            Filter::BehindUpstream => "Behind upstream",
            Filter::NotInRelease => "Not in the release",
            Filter::NotAssessed => "Not assessed",
        }
    }

    pub fn keeps(self, p: &Package) -> bool {
        match self {
            Filter::All => true,
            Filter::Is(s) => p.standing == s,
            Filter::Advisories => p.advised(),
            Filter::BehindUpstream => matches!(p.upstream, Some(crate::audit::Upstream::Newer(_))),
            Filter::NotInRelease => p.here && !p.in_release(),
            Filter::NotAssessed => {
                p.here && !p.security.assessed() && !matches!(p.security, crate::audit::Security::Source)
            }
        }
    }
}

/// A sheet section that folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Where,
    Tree,
    Restarts,
    Security,
    Programs,
    Versions,
    Source,
    Raw,
}

pub fn ui<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(s).size(size).font(fonts::ui()).color(color)
}

pub fn medium<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(s).size(size).font(fonts::ui_medium()).color(color)
}

pub fn mono<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(s).size(size).font(fonts::mono()).color(color)
}

pub fn display<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(s).size(size).font(Font { weight: iced::font::Weight::Semibold, ..fonts::display() }).color(color)
}
