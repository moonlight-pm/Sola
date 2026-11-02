//! App groups in the bar (spec § App groups and keys): where windows land,
//! moving rows and groups, stepping, and the saved layout.

use std::time::Instant;

use sola_bus::topics::{TabLayout, TabLayoutGroup, TabLayoutZone, Workspace, Zone};

use super::*;

const BROWSER: &str = "sola-browser";
const TERMINAL: &str = "sola-terminal";
const PROJECTS: &str = "sola-projects";
const DESIGN: &str = "sola-design";
const PAINT: &str = "sola-paint";
const EDITOR: &str = "sola-editor";
const MAIL: &str = "sola-mail";
const SETTINGS: &str = "sola-settings";

fn bar(s: &Stacks) -> Vec<u32> {
    s.bar().iter().map(|t| t.window_id).collect()
}

/// The visible blocks: a group as its app and rows, a loose row alone.
fn blocks(s: &Stacks) -> Vec<(Option<String>, Vec<u32>)> {
    s.blocks()
        .iter()
        .filter(|b| !b.tabs().is_empty())
        .map(|b| {
            let app = match b {
                Block::Group { app, .. } => Some(app.clone()),
                Block::Loose(_) => None,
            };
            (app, b.tabs().iter().map(|t| t.window_id).collect())
        })
        .collect()
}

fn open(s: &mut Stacks, window_id: u32, app: &str, t: Instant) {
    s.place(window_id, app, app, false, false, t);
    s.focus(window_id);
}

#[test]
fn a_window_opened_from_its_group_follows_its_opener() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 2, BROWSER, t);
    open(&mut s, 3, MAIL, t);
    s.focus(1);
    s.place(4, BROWSER, BROWSER, false, false, t);
    assert_eq!(bar(&s), vec![3, 1, 4, 2]);
}

#[test]
fn a_window_opened_from_elsewhere_goes_to_the_end_of_its_group() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 2, BROWSER, t);
    open(&mut s, 3, TERMINAL, t);
    assert_eq!(bar(&s), vec![3, 1, 2]);
    // A link from the terminal: a page at the end of Browser.
    s.place(4, BROWSER, BROWSER, false, false, t);
    assert_eq!(bar(&s), vec![3, 1, 2, 4]);
    // The next link from the same terminal follows the first.
    s.place(5, BROWSER, BROWSER, false, false, t);
    assert_eq!(bar(&s), vec![3, 1, 2, 4, 5]);
}

#[test]
fn loose_windows_go_above_every_group() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 2, BROWSER, t);
    open(&mut s, 3, SETTINGS, t);
    s.focus(1);
    // Focus in Browser: Mail goes to the end of the loose rows.
    s.place(4, MAIL, MAIL, false, false, t);
    assert_eq!(
        blocks(&s),
        vec![
            (None, vec![3]),
            (None, vec![4]),
            (Some(BROWSER.into()), vec![1, 2]),
        ]
    );
    // Focus on a loose row: right after it.
    s.focus(3);
    s.place(5, "steam", "steam", false, false, t);
    assert_eq!(bar(&s), vec![3, 5, 4, 1, 2]);
}

#[test]
fn a_new_group_lands_after_the_focused_group() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 6, BROWSER, t);
    open(&mut s, 2, TERMINAL, t);
    open(&mut s, 7, TERMINAL, t);
    open(&mut s, 3, MAIL, t);
    open(&mut s, 4, DESIGN, t);
    // Terminal's second window came from its lone row: the top of the groups.
    assert_eq!(bar(&s), vec![3, 4, 2, 7, 1, 6]);
    // Design's second makes the group, after the group that has focus.
    s.focus(2);
    s.place(8, DESIGN, DESIGN, false, false, t);
    assert_eq!(bar(&s), vec![3, 2, 7, 4, 8, 1, 6]);
    // From a loose row: the top of the groups.
    s.focus(3);
    s.place(5, PAINT, PAINT, false, false, t);
    assert_eq!(bar(&s), vec![3, 5, 2, 7, 4, 8, 1, 6]);
    s.place(9, PAINT, PAINT, false, false, t);
    assert_eq!(bar(&s), vec![3, 5, 9, 2, 7, 4, 8, 1, 6]);
}

#[test]
fn the_landing_slot_is_where_a_grouped_window_lands() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, MAIL, t);
    open(&mut s, 2, TERMINAL, t);
    open(&mut s, 4, TERMINAL, t);
    open(&mut s, 3, BROWSER, t);
    open(&mut s, 5, BROWSER, t);
    s.focus(3);
    // Terminal's group down to one: the next makes it again, in its place.
    s.remove(2, t);
    assert_eq!(bar(&s), vec![1, 4, 3, 5]);
    assert_eq!(s.landing_slot(TERMINAL, false).1, 4);
    // None left: its next window is a loose row, at their end.
    s.remove(4, t);
    assert_eq!(s.landing_slot(TERMINAL, false).1, 1);
    // A loose app from a group: the end of the loose rows.
    assert_eq!(s.landing_slot(SETTINGS, false).1, 1);
}

#[test]
fn a_session_lands_after_its_project() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, PROJECTS, t);
    open(&mut s, 2, PROJECTS, t);
    open(&mut s, 3, PROJECTS, t);
    open(&mut s, 4, MAIL, t);
    // Window 1 is the new one's project; focus is on Mail.
    s.place_near(5, PROJECTS, PROJECTS, false, false, &[1], t);
    assert_eq!(bar(&s), vec![4, 1, 5, 2, 3]);
    // Opened from its project's own window: after that one.
    s.focus(5);
    s.place_near(6, PROJECTS, PROJECTS, false, false, &[1, 5], t);
    assert_eq!(bar(&s), vec![4, 1, 5, 6, 2, 3]);
}

#[test]
fn rows_move_within_their_group_and_groups_among_groups() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 2, BROWSER, t);
    open(&mut s, 3, TERMINAL, t);
    open(&mut s, 5, TERMINAL, t);
    open(&mut s, 4, MAIL, t);
    assert_eq!(bar(&s), vec![4, 3, 5, 1, 2]);
    assert!(s.reorder(2, Some(1)));
    assert_eq!(bar(&s), vec![4, 3, 5, 2, 1]);
    // Not out of its group: before Terminal is its group's end.
    assert!(s.reorder(2, Some(3)));
    assert_eq!(bar(&s), vec![4, 3, 5, 1, 2]);
    assert!(s.move_block(&BlockId::Group(BROWSER.into()), Some(&BlockId::Group(TERMINAL.into()))));
    assert_eq!(bar(&s), vec![4, 1, 2, 3, 5]);
    // Not above the loose rows, and Mail not below the groups.
    assert!(!s.move_block(&BlockId::Group(BROWSER.into()), Some(&BlockId::Window(4))));
    assert!(!s.move_block(&BlockId::Window(4), None));
}

#[test]
fn super_grave_steps_the_app_s_windows_only() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    open(&mut s, 2, MAIL, t);
    open(&mut s, 3, BROWSER, t);
    assert_eq!(s.step_app(Some(1), false), Some(3));
    assert_eq!(s.step_app(Some(3), false), Some(1));
    assert_eq!(s.step_app(Some(1), true), Some(3));
    // One window: nowhere to go.
    assert_eq!(s.step_app(Some(2), false), None);
    assert_eq!(s.step_app(None, false), None);
}

#[test]
fn stepping_among_some_windows_keeps_bar_order() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for (w, app) in [(1, BROWSER), (2, MAIL), (3, BROWSER), (4, SETTINGS)] {
        open(&mut s, w, app, t);
    }
    // The loose rows first, then Browser: 2, 4, 1, 3.
    assert_eq!(bar(&s), vec![2, 4, 1, 3]);
    let here = [2, 3, 4];
    let among = |t: &Tab| here.contains(&t.window_id);
    assert_eq!(s.step_among(Some(4), false, &among), Some(3));
    assert_eq!(s.step_among(Some(3), false, &among), Some(2));
    assert_eq!(s.step_among(Some(2), true, &among), Some(3));
    // From a window not among them: the first.
    assert_eq!(s.step_among(Some(1), false, &among), Some(2));
}

#[test]
fn a_window_sent_away_joins_its_group_there() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, BROWSER, t);
    let main = s.showing();
    let other = s.add_workspace("Other");
    open(&mut s, 2, BROWSER, t);
    open(&mut s, 3, TERMINAL, t);
    s.show_workspace(main);
    assert!(s.send(1, other));
    // There it is the browser's second window: a group, under the terminal.
    assert_eq!(s.bar_on(other).iter().map(|t| t.window_id).collect::<Vec<_>>(), vec![3, 2, 1]);
    // Here it was a lone window: nothing left, no group to keep.
    assert!(s.blocks().is_empty());
}

fn main_layout(zones: Vec<(Zone, Vec<&str>, Option<u32>)>, groups: Vec<(&str, u32)>) -> TabLayout {
    TabLayout {
        zones: zones
            .into_iter()
            .map(|(zone, apps, shown)| TabLayoutZone {
                zone,
                apps: apps.into_iter().map(String::from).collect(),
                shown,
                workspace: "main".into(),
            })
            .collect(),
        workspaces: vec![
            Workspace {
                id: COMMON_WORKSPACE.0.into(),
                name: COMMON_WORKSPACE.1.into(),
            },
            Workspace {
                id: "main".into(),
                name: "Main".into(),
            },
        ],
        showing: "main".into(),
        groups: groups
            .into_iter()
            .map(|(app, at)| TabLayoutGroup {
                workspace: "main".into(),
                app: app.into(),
                at,
            })
            .collect(),
        ..TabLayout::default()
    }
}

#[test]
fn an_empty_group_is_saved_and_comes_back_in_its_place() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, MAIL, t);
    open(&mut s, 2, TERMINAL, t);
    open(&mut s, 4, TERMINAL, t);
    open(&mut s, 3, BROWSER, t);
    open(&mut s, 5, BROWSER, t);
    assert!(s.move_block(&BlockId::Group(TERMINAL.into()), Some(&BlockId::Group(BROWSER.into()))));
    assert_eq!(bar(&s), vec![1, 2, 4, 3, 5]);
    s.remove(2, t);
    s.remove(4, t);
    let later = t + GHOST_GRACE + std::time::Duration::from_secs(1);
    let saved = s.layout(later);
    assert_eq!(
        saved.groups,
        vec![TabLayoutGroup {
            workspace: FIRST_WORKSPACE.0.into(),
            app: TERMINAL.into(),
            at: 1,
        }]
    );

    // A fresh shell: Mail and Browser come back, then two new terminals:
    // the first a loose row, the second back in the group's place.
    let mut s = Stacks::new();
    s.load(&saved, later);
    s.clear();
    s.place(3, BROWSER, BROWSER, false, true, later);
    s.place(5, BROWSER, BROWSER, false, true, later);
    s.place(1, MAIL, MAIL, false, true, later);
    assert_eq!(bar(&s), vec![1, 3, 5]);
    s.place(9, TERMINAL, TERMINAL, false, false, later);
    assert_eq!(bar(&s), vec![1, 9, 3, 5]);
    s.place(10, TERMINAL, TERMINAL, false, false, later);
    assert_eq!(bar(&s), vec![1, 9, 10, 3, 5]);
}

#[test]
fn a_bar_saved_before_groups_regroups_at_each_app_s_first_row() {
    let t = Instant::now();
    let layout = main_layout(
        vec![(
            Zone::Fullscreen,
            vec!["sola-browser/a", MAIL, "sola-browser/b", SETTINGS],
            Some(0),
        )],
        Vec::new(),
    );
    let mut s = Stacks::new();
    s.load(&layout, t);
    s.clear();
    s.place(14, SETTINGS, SETTINGS, false, true, t);
    s.place(12, BROWSER, "sola-browser/b", false, true, t);
    s.place(13, MAIL, MAIL, false, true, t);
    s.place(11, BROWSER, "sola-browser/a", false, true, t);
    // The loose rows go first.
    assert_eq!(
        blocks(&s),
        vec![
            (None, vec![13]),
            (None, vec![14]),
            (Some(BROWSER.into()), vec![11, 12]),
        ]
    );
    // Saved again, the group is one run, after the loose rows.
    let zones = s.layout(t).zones;
    assert_eq!(zones[0].apps, vec![MAIL, SETTINGS, "sola-browser/a", "sola-browser/b"]);
}

#[test]
fn a_saved_bar_with_groups_restores_whatever_order_windows_come() {
    let t = Instant::now();
    let layout = main_layout(
        vec![(
            Zone::Fullscreen,
            vec![MAIL, SETTINGS, "sola-terminal/x", "sola-terminal/y"],
            Some(0),
        )],
        vec![(BROWSER, 4)],
    );
    let mut s = Stacks::new();
    s.load(&layout, t);
    s.clear();
    s.place(4, TERMINAL, "sola-terminal/y", false, true, t);
    s.place(2, SETTINGS, SETTINGS, false, true, t);
    s.place(1, MAIL, MAIL, false, true, t);
    s.place(3, TERMINAL, "sola-terminal/x", false, true, t);
    assert_eq!(bar(&s), vec![1, 2, 3, 4]);
    assert_eq!(s.layout(t), layout);
}

/// Editor opens a window per file, so its windows group, under the name its
/// launcher row gives the group's caption.
#[test]
fn the_editor_has_a_launcher_row_and_groups_under_it() {
    let row = crate::builtins::builtin_apps()
        .into_iter()
        .find(|a| a.app_id == EDITOR)
        .expect("Editor is in the launcher");
    assert_eq!(row.label, "Editor");

    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, EDITOR, t);
    open(&mut s, 2, MAIL, t);
    open(&mut s, 3, EDITOR, t);
    assert_eq!(
        blocks(&s),
        vec![(None, vec![2]), (Some(EDITOR.to_string()), vec![1, 3])]
    );
}
