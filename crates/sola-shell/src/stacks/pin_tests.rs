//! Pinned windows: on every workspace, each with its own row, in one zone.

use std::time::{Duration, Instant};

use sola_bus::topics::{PINNED_LAYOUT_ID, TabLayout, TabLayoutZone, Workspace, Zone};

use super::*;

const MAIN: usize = 0;
const LEFT: Zone = Zone::Grid(1);
const TERMINAL: &str = "sola-terminal";
const MAIL: &str = "sola-mail";
const SCOPE: &str = "sola-scope";

fn bar_on(s: &Stacks, workspace: usize) -> Vec<u32> {
    s.bar_on(workspace).iter().map(|t| t.window_id).collect()
}

/// Main and Mail, showing Main; window 1 of Terminal on Main, 2 of Mail on
/// Mail.
fn two_with_a_window_each(t: Instant) -> (Stacks, usize) {
    let mut s = Stacks::new();
    s.place(1, TERMINAL, TERMINAL, false, false, t);
    let mail = s.add_workspace("Mail");
    s.place(2, MAIL, MAIL, false, false, t);
    s.show_workspace(MAIN);
    (s, mail)
}

#[test]
fn a_new_desk_is_one_workspace_and_no_other() {
    let s = Stacks::new();
    let list = s.workspaces();
    assert_eq!(list.len(), 1);
    assert_eq!(list[MAIN].id, FIRST_WORKSPACE.0);
    assert_eq!(s.showing(), MAIN);
}

#[test]
fn a_pinned_window_stays_where_it_is_and_joins_every_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    assert_eq!(s.toggle_pin(1), Some(true));
    assert_eq!(s.showing(), MAIN, "you stay where you are");
    assert_eq!(bar_on(&s, MAIN), vec![1]);
    assert_eq!(bar_on(&s, mail), vec![2, 1]);
    assert!(s.is_pinned(1));
    assert_eq!(s.workspace_of(1), None, "on them all");
    // Where it lands it does not cover what shows.
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(2));
    assert!(s.on_showing(1));
}

#[test]
fn unpinned_it_stays_on_the_workspace_showing_and_leaves_the_rest() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    s.show_workspace(mail);
    assert_eq!(s.toggle_pin(1), Some(false));
    assert!(!s.is_pinned(1));
    assert_eq!(bar_on(&s, mail), vec![2, 1]);
    assert_eq!(bar_on(&s, MAIN), Vec::<u32>::new());
    assert_eq!(s.workspace_of(1), Some(s.workspaces()[mail].id.as_str()));
}

#[test]
fn pinning_a_window_that_is_not_placed_does_nothing() {
    let mut s = Stacks::new();
    assert_eq!(s.toggle_pin(9), None);
    assert!(!s.is_pinned(9));
}

/// Steam's two windows have one key: pinning one leaves the other.
#[test]
fn only_the_window_pinned_is_pinned_when_two_have_one_key() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "steam", "steam", false, false, t);
    s.place(2, "steam", "steam", false, false, t);
    let other = s.add_workspace("Other");
    s.show_workspace(MAIN);
    s.toggle_pin(2);
    assert!(s.is_pinned(2) && !s.is_pinned(1));
    assert_eq!(bar_on(&s, other), vec![2]);
}

#[test]
fn a_pinned_window_shows_on_a_workspace_with_nothing_there() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, TERMINAL, TERMINAL, false, false, t);
    let empty = s.add_workspace("Empty");
    s.show_workspace(MAIN);
    s.toggle_pin(1);
    s.show_workspace(empty);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn a_pinned_window_counts_toward_its_app_s_group_on_each_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.show_workspace(mail);
    s.place(3, TERMINAL, TERMINAL, false, false, t);
    s.toggle_pin(3);
    // Alone on Mail: a row. With Main's own: a group.
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
    assert_eq!(group_on(&mut s, mail), vec![]);
    assert_eq!(group_on(&mut s, MAIN), vec![(TERMINAL.to_string(), vec![1, 3])]);
}

#[test]
fn order_and_what_is_on_top_are_each_workspace_s_own() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    s.place(3, MAIL, "other", false, false, t);
    s.toggle_pin(3);
    s.show_workspace(mail);
    assert_eq!(bar_on(&s, mail), vec![2, 1, 3]);
    assert!(s.reorder(3, Some(2)));
    assert!(s.show(1));
    assert_eq!(s.showing(), mail, "a pinned row stays on this workspace");
    assert_eq!(s.shown(FULL), Some(1));
    assert_eq!(bar_on(&s, mail), vec![3, 2, 1]);
    assert_eq!(bar_on(&s, MAIN), vec![1, 3]);
    s.show_workspace(MAIN);
    s.show(3);
    assert_eq!(s.shown(FULL), Some(3));
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(1), "Main's choice stays on Main");
}

#[test]
fn moving_a_pinned_window_moves_it_everywhere_and_raises_it_only_here() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    assert!(s.move_to(1, LEFT));
    assert_eq!(s.shown(LEFT), Some(1));
    s.show_workspace(mail);
    assert_eq!(s.zone_of(1), Some(LEFT));
    assert_eq!(s.bar().iter().find(|t| t.window_id == 1).map(|t| t.zone), Some(LEFT));
    // Mail's Full goes on showing its own.
    assert_eq!(s.shown(FULL), Some(2));
    assert_eq!(s.on_screen(&|_| false), vec![FULL]);
}

#[test]
fn sent_to_a_workspace_a_pinned_window_is_unpinned_and_goes_there() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    assert!(s.send(1, mail));
    assert!(!s.is_pinned(1));
    assert_eq!(bar_on(&s, MAIN), Vec::<u32>::new());
    assert_eq!(bar_on(&s, mail), vec![2, 1]);
    assert_eq!(s.workspace_of(1), Some(s.workspaces()[mail].id.as_str()));
    s.show_workspace(mail);
    assert_eq!(s.shown(FULL), Some(1), "it comes up where it went");
}

#[test]
fn sent_to_the_workspace_showing_a_pinned_window_stays_here_alone() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    assert!(s.send(1, MAIN));
    assert!(!s.is_pinned(1));
    assert_eq!(bar_on(&s, MAIN), vec![1]);
    assert_eq!(bar_on(&s, mail), vec![2]);
}

#[test]
fn a_dialog_goes_with_its_pinned_parent_and_pins_it() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    s.place(9, TERMINAL, "dialog", false, false, t);
    s.set_parent(9, 1);
    assert!(s.is_pinned(9));
    s.show_workspace(mail);
    assert!(s.on_showing(9));
    assert!(s.show(9));
    assert_eq!(s.showing(), mail);
    // Its menu's item acts on the window it rides on.
    assert_eq!(s.toggle_pin(9), Some(false));
    assert!(!s.is_pinned(1));
}

#[test]
fn a_pinned_window_is_no_workspace_s_own() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    assert_eq!(s.windows_on(MAIN), Vec::<u32>::new());
    assert_eq!(s.windows_on(mail), vec![2]);
}

#[test]
fn a_new_workspace_starts_with_the_pinned_windows() {
    let t = Instant::now();
    let (mut s, _) = two_with_a_window_each(t);
    s.toggle_pin(1);
    let new = s.add_workspace("New");
    assert_eq!(bar_on(&s, new), vec![1]);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn deleting_a_workspace_moves_only_its_own_windows() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.toggle_pin(3);
    assert_eq!(bar_on(&s, MAIN), vec![1, 3]);
    s.show_workspace(mail);
    // The first goes: the next one is its heir.
    assert!(s.delete_workspace(MAIN));
    assert_eq!(s.workspace_count(), 1);
    assert_eq!(bar_on(&s, 0), vec![2, 3, 1]);
    assert!(s.is_pinned(3));
    assert!(!s.delete_workspace(0), "the last one stays");
}

#[test]
fn a_closed_pinned_window_leaves_every_workspace_and_comes_back_to_each_place() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.toggle_pin(3);
    s.show_workspace(mail);
    s.reorder(1, None);
    assert_eq!(bar_on(&s, mail), vec![2, 3, 1]);
    assert!(s.remove(3, t).is_some());
    assert!(!s.is_pinned(3));
    for i in [MAIN, mail] {
        assert!(!bar_on(&s, i).contains(&3));
    }
    // It restarts, just after: every place back.
    s.place(7, SCOPE, SCOPE, false, false, t + Duration::from_secs(1));
    assert!(s.is_pinned(7));
    assert_eq!(bar_on(&s, MAIN), vec![1, 7]);
    assert_eq!(bar_on(&s, mail), vec![2, 7, 1]);
}

/// A saved desk: Main shows Terminal's pinned window under its own, Mail its
/// own over it, in each one's order.
fn saved_desk(t: Instant) -> (TabLayout, usize) {
    let (mut s, mail) = two_with_a_window_each(t);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.toggle_pin(3);
    s.place(4, "foot", "foot", false, false, t);
    s.toggle_pin(4);
    s.show_workspace(mail);
    s.reorder(4, Some(2));
    s.show(3);
    s.show_workspace(MAIN);
    s.show(4);
    s.reorder(3, Some(1));
    assert_eq!(bar_on(&s, MAIN), vec![3, 1, 4]);
    assert_eq!(bar_on(&s, mail), vec![4, 2, 3]);
    (s.layout(t), mail)
}

#[test]
fn a_pinned_window_comes_back_to_its_place_on_each_workspace_after_a_restart() {
    let t = Instant::now();
    let (saved, mail) = saved_desk(t);
    assert!(saved.workspaces.iter().all(|w| w.id != PINNED_LAYOUT_ID), "not a workspace");
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
        assert!(s.is_pinned(23) && s.is_pinned(24));
        assert!(!s.is_pinned(21) && !s.is_pinned(22));
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

/// While a pinned window is away (a restart), a saved layout still says so.
#[test]
fn a_layout_saved_while_a_pinned_window_is_away_keeps_it_pinned() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.place(3, SCOPE, SCOPE, false, false, t);
    s.toggle_pin(3);
    s.remove(3, t);
    let saved = s.layout(t);

    let mut s = Stacks::new();
    s.load(&saved, t);
    s.place(7, SCOPE, SCOPE, false, true, t);
    assert!(s.is_pinned(7));
    assert!(bar_on(&s, mail).contains(&7));
}

#[test]
fn a_tab_paired_after_it_opens_goes_back_to_every_workspace() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.place(3, TERMINAL, "sola-terminal/a", false, false, t);
    s.toggle_pin(3);
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
    assert!(s.is_pinned(3));
    assert_eq!(bar_on(&s, mail), vec![3, 2]);
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
}

#[test]
fn a_new_key_reaches_every_row_of_a_pinned_window() {
    let t = Instant::now();
    let (mut s, mail) = two_with_a_window_each(t);
    s.toggle_pin(1);
    assert!(s.set_key(1, "sola-terminal/a", t).is_none());
    assert!(s.is_pinned(1));
    for i in [MAIN, mail] {
        let tabs = s.bar_on(i);
        let row = tabs.iter().find(|t| t.window_id == 1).unwrap();
        assert_eq!(row.key, "sola-terminal/a");
    }
}

#[test]
fn a_workspace_saved_without_a_pinned_window_s_row_gets_it_at_the_end() {
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

/// How a layout looked when Common was a workspace: first in the list, its
/// own rows, and a copy on each of the others.
fn saved_with_common(showing: &str) -> TabLayout {
    let row = |workspace: &str, apps: &[&str], shown| TabLayoutZone {
        zone: FULL,
        apps: apps.iter().map(|a| (*a).to_string()).collect(),
        shown,
        workspace: workspace.into(),
    };
    TabLayout {
        zones: vec![
            row(PINNED_LAYOUT_ID, &[SCOPE, "only-common"], Some(0)),
            row("main", &[TERMINAL, SCOPE], Some(0)),
            row("mail", &[SCOPE, MAIL], Some(1)),
        ],
        workspaces: vec![
            Workspace { id: PINNED_LAYOUT_ID.into(), name: "Common".into() },
            Workspace { id: "main".into(), name: "Main".into() },
            Workspace { id: "mail".into(), name: "Mail".into() },
        ],
        showing: showing.into(),
        ..TabLayout::default()
    }
}

#[test]
fn a_layout_saved_with_common_comes_back_with_its_windows_pinned() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.load(&saved_with_common("mail"), t);
    let ids: Vec<String> = s.workspaces().into_iter().map(|w| w.id).collect();
    assert_eq!(ids, vec!["main".to_string(), "mail".to_string()], "no Common");
    assert_eq!(s.showing_id(), "mail");

    s.place(1, TERMINAL, TERMINAL, false, true, t);
    s.place(2, MAIL, MAIL, false, true, t);
    s.place(3, SCOPE, SCOPE, false, true, t);
    s.place(4, "only-common", "only-common", false, true, t);
    assert!(s.is_pinned(3) && s.is_pinned(4));
    assert!(!s.is_pinned(1) && !s.is_pinned(2));
    assert_eq!(bar_on(&s, 0), vec![1, 3, 4]);
    assert_eq!(bar_on(&s, 1), vec![3, 2, 4]);
    assert_eq!(s.shown(FULL), Some(2), "Mail's choice");

    assert!(s.layout(t).workspaces.iter().all(|w| w.id != PINNED_LAYOUT_ID));
}

#[test]
fn a_layout_saved_showing_common_comes_back_on_the_first_workspace() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.load(&saved_with_common(PINNED_LAYOUT_ID), t);
    assert_eq!(s.showing(), MAIN);
    assert_eq!(s.showing_id(), "main");
}

#[test]
fn a_layout_from_before_common_has_nothing_pinned() {
    let t = Instant::now();
    let old = TabLayout {
        zones: vec![TabLayoutZone {
            zone: FULL,
            apps: vec![TERMINAL.into()],
            shown: Some(0),
            workspace: "ws-1".into(),
        }],
        workspaces: vec![Workspace { id: "ws-1".into(), name: "One".into() }],
        showing: "ws-1".into(),
        ..TabLayout::default()
    };
    let mut s = Stacks::new();
    s.load(&old, t);
    assert_eq!(s.workspaces()[MAIN].id, "ws-1");
    s.place(1, TERMINAL, TERMINAL, false, true, t);
    assert!(!s.is_pinned(1));
    assert_eq!(bar_on(&s, MAIN), vec![1]);
    // Older still: no workspaces at all, the zones are the first one's.
    let older = TabLayout {
        zones: vec![TabLayoutZone {
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
    assert!(!s.is_pinned(1));
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
}
