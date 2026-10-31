//! The places: All items and Favorites; the kinds; folders (new with +,
//! rename with a double click); each organization's collections; Trash.

use iced::widget::container;
use iced::{Element, Length};
use sola_kit::components::scroll;
use sola_kit::components::sidebar::style as sidebar_style;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};

use super::bar::format_count;
use super::widgets;
use super::{App, Msg, Section};
use crate::model::plate::kind_icon;
use crate::model::section::count;

impl App {
    pub(super) fn view_sidebar(&self) -> Element<'_, Msg> {
        let mut sections = vec![
            SidebarSection::unlabeled(vec![
                self.place(Section::All, "All items", "lucide/vault"),
                self.place(Section::Favorites, "Favorites", "lucide/star"),
            ]),
            SidebarSection::unlabeled(
                self.listed_kinds()
                    .into_iter()
                    .map(|k| self.place(Section::Kind(k), Section::kind_label(k), kind_icon(k)))
                    .collect(),
            ),
        ];

        let mut folders: Vec<SidebarItem<'_, Msg>> = self
            .folders
            .iter()
            .map(|f| {
                self.place(Section::Folder(f.id.clone()), &f.name, "lucide/folder")
                    .on_double_click(Msg::RenameFolder(f.id.clone()))
            })
            .collect();
        if !self.folders.is_empty() {
            folders.push(self.place(Section::NoFolder, "No folder", "lucide/folder"));
        }
        sections.push(SidebarSection::new("Folders", folders).on_add(Msg::NewFolder));

        for org in &self.orgs {
            let collections: Vec<SidebarItem<'_, Msg>> = self
                .collections
                .iter()
                .filter(|c| c.organization_id == org.id)
                .map(|c| self.place(Section::Collection(c.id.clone()), &c.name, "lucide/library"))
                .collect();
            if !collections.is_empty() {
                sections.push(SidebarSection::new(org.name.clone(), collections));
            }
        }

        sections.push(SidebarSection::unlabeled(vec![self.place(
            Section::Trash,
            "Trash",
            "lucide/trash",
        )]));

        container(scroll::scrollable(sidebar(sections).height(Length::Shrink)).height(Length::Fill))
            .width(Length::Fixed(sola_kit::components::SIDEBAR_WIDTH))
            .height(Length::Fill)
            .style(sidebar_style)
            .into()
    }

    fn place(&self, section: Section, label: &str, icon: &'static str) -> SidebarItem<'_, Msg> {
        let n = count(&self.items, &section);
        let mut item = SidebarItem::new(label, Msg::Show(section.clone()))
            .leading(widgets::muted(icon, 14.0))
            .active(self.section == section);
        if n > 0 {
            item = item.secondary(format_count(n));
        }
        item
    }
}
