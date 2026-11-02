//! The Common workspace: its windows are on every workspace, each with its
//! own row, in one zone, and they leave only from Common.

use std::time::{Duration, Instant};

use sola_bus::topics::{TabLayout, Zone};

use super::*;

const MAIN: usize = COMMON + 1;
const LEFT: Zone = Zone::Grid(1);
const TERMINAL: &str = "sola-terminal";
const MAIL: &str = "sola-mail";
const SCOPE: &str = "sola-scope";

fn bar_on(s: &Stacks, workspace: usize) -> Vec<u32> {
    s.bar_on(workspace).iter().map(|t| t.window_id).collect()
}

/// Common, Main and Mail, showing Main; window 1 of Terminal on Main, 2 of
/// Mail on Mail.
fn two_with_a_window_each(t: Instant) -> (Stacks, usize) {
    let mut s = Stacks::new();
    s.place(1, TERMINAL, TERMINAL, false, false, t);
    let mail = s.add_workspace("Mail");
    s.place(2, MAIL, MAIL, false, false, t);
    s.show_workspace(MAIN);
    (s, mail)
}

#[test]
fn common_is_first_and_a_new_desk_shows_the_one_after_it() {
    let s = Stacks::new();
    let list = s.workspaces();
    assert_eq!(list[COMMON].id, COMMON_WORKSPACE.0);
    assert_eq!(list[COMMON].name, COMMON_WORKSPACE.1);
    assert_eq!(list[MAIN].id, FIRST_WORKSPACE.0);
    assert_eq!(s.showing(), MAIN);
}

#[test]
fn common_cannot_be_renamed_moved_or_deleted_and_nothing_goes_before_it() {
    let mut s = Stacks::new();
    let other = s.add_workspace("Other");
    assert!(!s.rename_workspace(COMMON, "Shared"));
    assert!(!s.move_workspace(COMMON, false));
    assert!(!s.move_workspace(MAIN, true));
    assert!(!s.reorder_workspace(other, COMMON));
    assert!(!s.reorder_workspace(COMMON, other));
    assert!(!s.delete_workspace(COMMON));
    assert_eq!(s.workspaces()[COMMON].name, COMMON_WORKSPACE.1);
}

#[test]
fn a_window_sent_to_common_stays_where_it_is_and_joins_every_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    assert!(s.send(1, COMMON));
    assert_eq!(s.showing(), MAIN);
    assert_eq!(bar_on(&s, COMMON), vec![1]);
    assert_eq!(bar_on(&s, MAIN), vec![1]);
    assert_eq!(bar_on(&s, mail), vec![2, 1]);
    assert!(s.is_common(1));
    assert_eq!(s.workspace_of(1), Some(COMMON_WORKSPACE.0));
    // Where it lands it does not cover what shows.
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(2));
    assert!(s.on_showing(1));
    // Sending it there again does nothing.
    assert!(!s.send(1, COMMON));
}

#[test]
fn a_window_opened_on_common_is_on_every_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(COMMON);
    s.place(3, SCOPE, SCOPE, false, false, t);
    assert_eq!(s.shown(FULL), Some(3));
    assert_eq!(bar_on(&s, MAIN), vec![1, 3]);
    assert_eq!(bar_on(&s, mail), vec![2, 3]);
    s.show_workspace(MAIN);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn a_common_window_shows_on_a_workspace_with_nothing_there() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, TERMINAL, TERMINAL, false, false, t);
    let empty = s.add_workspace("Empty");
    s.show_workspace(MAIN);
    assert!(s.send(1, COMMON));
    s.show_workspace(empty);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn a_common_window_counts_toward_its_app_s_group_on_each_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(COMMON);
    s.place(3, TERMINAL, TERMINAL, false, false, t);
    // Alone on Common and on Mail: a row. With Main's own: a group.
    let group_on = |s: &mut Stacks, i: usize| {
        s.show_workspace(i);
        s.blocks()
            .iter()
            .filter_map(|b| match b {
                Block::Group { app, tabs, .. } if !tabs.is_empty() => {
                    Some((app.clone(), tabs.iter().map(|t| t.window_id).collect::<Vec<_>>()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(group_on(&mut s, COMMON), vec![]);
    assert_eq!(group_on(&mut s, mail), vec![]);
    assert_eq!(group_on(&mut s, MAIN), vec![(TERMINAL.to_string(), vec![1, 3])]);
}

#[test]
fn order_and_what_is_on_top_are_each_workspace_s_own() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    assert!(s.send(1, COMMON));
    s.show_workspace(COMMON);
    s.place(3, MAIL, "other", false, false, t);
    s.show_workspace(mail);
    assert_eq!(bar_on(&s, mail), vec![2, 1, 3]);
    assert!(s.reorder(3, Some(2)));
    assert!(s.show(1));
    assert_eq!(s.showing(), mail, "a common row stays on this workspace");
    assert_eq!(s.shown(FULL), Some(1));
    assert_eq!(bar_on(&s, mail), vec![3, 2, 1]);
    assert_eq!(bar_on(&s, MAIN), vec![1, 3]);
    s.show_workspace(MAIN);
    assert_eq!(s.shown(FULL), Some(1));
    s.show(3);
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(1), "Main's choice stays on Main");
}

#[test]
fn moving_a_common_window_moves_it_everywhere_and_raises_it_only_here() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    assert!(s.move_to(1, LEFT));
    assert_eq!(s.shown(LEFT), Some(1));
    s.show_workspace(mail);
    assert_eq!(s.zone_of(1), Some(LEFT));
    assert_eq!(s.bar().iter().find(|t| t.window_id == 1).map(|t| t.zone), Some(LEFT));
    // Mail's Full goes on showing its own.
    assert_eq!(s.shown(FULL), Some(2));
    assert_eq!(s.on_screen(&|_| false), vec![FULL]);
    s.show_workspace(COMMON);
    assert_eq!(s.bar().iter().find(|t| t.window_id == 1).map(|t| t.zone), Some(LEFT));
    assert_eq!(s.shown(FULL), None);
}

#[test]
fn a_common_window_leaves_only_from_common() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    assert!(s.is_pinned_here(1));
    assert!(!s.send(1, mail), "sent from Main");
    s.show_workspace(mail);
    assert!(s.is_pinned_here(1));
    assert!(!s.send(1, MAIN), "sent from Mail");
    assert_eq!(bar_on(&s, COMMON), vec![1]);

    s.show_workspace(COMMON);
    assert!(!s.is_pinned_here(1));
    assert!(!s.send(1, COMMON));
    assert!(s.send(1, mail));
    assert!(!s.is_common(1));
    assert_eq!(bar_on(&s, COMMON), Vec::<u32>::new());
    assert_eq!(bar_on(&s, MAIN), Vec::<u32>::new());
    assert_eq!(bar_on(&s, mail), vec![2, 1]);
    assert_eq!(s.workspace_of(1), Some(s.workspaces()[mail].id.as_str()));
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(1), "it comes up where it went");
}

#[test]
fn a_dialog_goes_with_its_common_parent() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    s.place(9, TERMINAL, "dialog", false, false, t);
    s.set_parent(9, 1);
    assert!(s.is_common(9));
    s.show_workspace(mail);
    assert!(!s.is_pinned_here(9), "a dialog can close");
    assert!(s.is_pinned_here(1));
    assert!(s.on_showing(9));
    assert!(s.show(9));
    assert_eq!(s.showing(), mail);
}

#[test]
fn the_pinned_tabs_are_common_s_except_on_common() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    assert_eq!(s.pinned_keys(), Vec::<String>::new());
    s.send(1, COMMON);
    assert_eq!(s.pinned_keys(), vec![TERMINAL.to_string()]);
    s.show_workspace(mail);
    assert_eq!(s.pinned_keys(), vec![TERMINAL.to_string()]);
    s.show_workspace(COMMON);
    assert_eq!(s.pinned_keys(), Vec::<String>::new());
}

#[test]
fn only_common_counts_a_common_window_as_its_own() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    assert_eq!(s.windows_on(COMMON), vec![1]);
    assert_eq!(s.windows_on(MAIN), Vec::<u32>::new());
    assert_eq!(s.windows_on(mail), vec![2]);
}

#[test]
fn a_new_workspace_starts_with_common_s_windows() {
    let t = Instant::now();
    let (mut s, _) = two_with_a_window_each(t);
    s.send(1, COMMON);
    let new = s.add_workspace("New");
    assert_eq!(bar_on(&s, new), vec![1]);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn deleting_a_workspace_moves_only_its_own_windows_and_never_to_common() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(COMMON);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.show_workspace(mail);
    // The first after Common goes: the next one is its heir.
    assert!(s.delete_workspace(MAIN));
    assert_eq!(s.workspace_count(), 2);
    assert_eq!(bar_on(&s, COMMON), vec![3]);
    assert_eq!(bar_on(&s, MAIN), vec![2, 3, 1]);
    assert!(!s.delete_workspace(MAIN), "the last ordinary one stays");
}

#[test]
fn a_closed_common_window_leaves_every_workspace_and_comes_back_to_each_place() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    s.show_workspace(COMMON);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.show_workspace(mail);
    s.reorder(1, None);
    assert_eq!(bar_on(&s, mail), vec![2, 3, 1]);
    assert!(s.remove(3, t).is_some());
    for i in [COMMON, MAIN, mail] {
        assert!(!bar_on(&s, i).contains(&3));
    }
    // It restarts, just after: every place back.
    s.place(7, SCOPE, SCOPE, false, false, t + Duration::from_secs(1));
    assert!(s.is_common(7));
    assert_eq!(bar_on(&s, COMMON), vec![1, 7]);
    assert_eq!(bar_on(&s, MAIN), vec![1, 7]);
    assert_eq!(bar_on(&s, mail), vec![2, 7, 1]);
}

/// A saved desk: Main shows Terminal's common window under its own, Mail its
/// own over it, in each one's order.
fn saved_desk(t: Instant) -> (TabLayout, usize) {
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(COMMON);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.place(4, "foot", "foot", false, false, t);
    s.show_workspace(mail);
    s.reorder(4, Some(2));
    s.show(3);
    s.show_workspace(MAIN);
    s.show(4);
    s.reorder(3, Some(1));
    assert_eq!(bar_on(&s, COMMON), vec![3, 4]);
    assert_eq!(bar_on(&s, MAIN), vec![3, 1, 4]);
    assert_eq!(bar_on(&s, mail), vec![4, 2, 3]);
    (s.layout(t), mail)
}

#[test]
fn a_common_window_comes_back_to_its_place_on_each_workspace_after_a_restart() {
    let t = Instant::now();
    let (saved, mail) = saved_desk(t);
    // Every order the windows may come back in.
    let orders: [[(u32, &str); 4]; 3] = [
        [(21, TERMINAL), (22, MAIL), (23, SCOPE), (24, "foot")],
        [(24, "foot"), (23, SCOPE), (22, MAIL), (21, TERMINAL)],
        [(23, SCOPE), (21, TERMINAL), (24, "foot"), (22, MAIL)],
    ];
    for order in orders {
        let mut s = Stacks::new();
        s.load(&saved, t);
        for (w, app) in order {
            s.place(w, app, app, false, true, t);
        }
        assert_eq!(s.showing(), MAIN);
        assert!(s.is_common(23) && s.is_common(24));
        assert!(!s.is_common(21) && !s.is_common(22));
        assert_eq!(bar_on(&s, COMMON), vec![23, 24], "{order:?}");
        assert_eq!(bar_on(&s, MAIN), vec![23, 21, 24], "{order:?}");
        assert_eq!(bar_on(&s, mail), vec![24, 22, 23], "{order:?}");
        assert_eq!(s.shown(FULL), Some(24), "Main's choice: {order:?}");
        s.show_workspace(mail);
        assert_eq!(s.shown(FULL), Some(23), "Mail's choice: {order:?}");
        // And it saves back the same.
        s.show_workspace(MAIN);
        assert_eq!(s.layout(t).zones, saved.zones, "{order:?}");
    }
}

#[test]
fn a_tab_paired_after_it_opens_goes_back_to_every_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(COMMON);
    s.place(3, TERMINAL, "sola-terminal/a", false, false, t);
    s.show_workspace(mail);
    s.reorder(3, Some(2));
    assert_eq!(bar_on(&s, mail), vec![3, 2]);
    let saved = s.layout(t);

    // The tabs come up as the app, then pair.
    let mut s = Stacks::new();
    s.load(&saved, t);
    s.place(1, TERMINAL, TERMINAL, false, true, t);
    s.place(2, MAIL, MAIL, false, true, t);
    s.place(3, TERMINAL, TERMINAL, false, true, t);
    assert!(s.set_key(3, "sola-terminal/a", t).is_some());
    assert!(s.is_common(3));
    assert_eq!(bar_on(&s, COMMON), vec![3]);
    assert_eq!(bar_on(&s, mail), vec![3, 2]);
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
}

#[test]
fn a_new_key_reaches_every_row_of_a_common_window() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.send(1, COMMON);
    assert!(s.set_key(1, "sola-terminal/a", t).is_none());
    for i in [COMMON, MAIN, mail] {
        let tabs = s.bar_on(i);
        let row = tabs.iter().find(|t| t.window_id == 1).unwrap();
        assert_eq!(row.key, "sola-terminal/a");
    }
}

#[test]
fn a_workspace_saved_without_a_common_window_s_row_gets_it_at_the_end() {
    let t = Instant::now();
    let (saved, mail) = saved_desk(t);
    let mut saved = saved;
    let mail_id = saved.workspaces[mail].id.clone();
    saved.zones.retain(|z| z.workspace != mail_id);
    let mut s = Stacks::new();
    s.load(&saved, t);
    s.place(23, SCOPE, SCOPE, false, true, t);
    s.place(24, "foot", "foot", false, true, t);
    assert_eq!(bar_on(&s, mail), vec![23, 24]);
}

#[test]
fn a_layout_from_before_common_keeps_its_windows_off_it() {
    let t = Instant::now();
    let old = TabLayout {
        zones: vec![sola_bus::topics::TabLayoutZone {
            zone: FULL,
            apps: vec![TERMINAL.into()],
            shown: Some(0),
            workspace: "ws-1".into(),
        }],
        workspaces: vec![sola_bus::topics::Workspace {
            id: "ws-1".into(),
            name: "One".into(),
        }],
        showing: "ws-1".into(),
        ..TabLayout::default()
    };
    let mut s = Stacks::new();
    s.load(&old, t);
    assert_eq!(s.workspaces()[COMMON].id, COMMON_WORKSPACE.0);
    assert_eq!(s.workspaces()[MAIN].id, "ws-1");
    assert_eq!(s.showing(), MAIN);
    s.place(1, TERMINAL, TERMINAL, false, true, t);
    assert!(!s.is_common(1));
    assert_eq!(bar_on(&s, MAIN), vec![1]);
    // Older still: no workspaces at all, the zones are the first ordinary one's.
    let older = TabLayout {
        zones: vec![sola_bus::topics::TabLayoutZone {
            zone: FULL,
            apps: vec![TERMINAL.into()],
            shown: Some(0),
            workspace: String::new(),
        }],
        ..TabLayout::default()
    };
    let mut s = Stacks::new();
    s.load(&older, t);
    s.place(1, TERMINAL, TERMINAL, false, true, t);
    assert!(!s.is_common(1));
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
}
