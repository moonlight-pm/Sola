use std::time::{Duration, Instant};

use sola_bus::topics::{TabLayout, TabLayoutZone, Workspace, Zone};

use super::*;

/// The default layout's two columns.
const LEFT: Zone = Zone::Grid(1);
const RIGHT: Zone = Zone::Grid(2);


fn none(_: &str) -> bool {
    false
}

/// `zone`'s tabs on the showing workspace, in bar order.
fn ids(s: &Stacks, zone: Zone) -> Vec<u32> {
    s.bar()
        .iter()
        .filter(|t| t.zone == zone)
        .map(|t| t.window_id)
        .collect()
}

/// The showing workspace's bar.
fn bar(s: &Stacks) -> Vec<u32> {
    s.bar().iter().map(|t| t.window_id).collect()
}

#[test]
fn new_windows_join_the_focused_zone_and_show() {
    let t = Instant::now();
    let mut s = Stacks::new();
    let p = s.place(1, "sola-terminal", "sola-terminal", false, false, t);
    assert_eq!(p, Placed { zone: FULL, shown: true, returning: false });
    s.place(2, "sola-browser", "sola-browser", false, false, t);
    assert_eq!(ids(&s, FULL), vec![1, 2]);
    assert_eq!(s.shown(FULL), Some(2));

    s.move_to(2, RIGHT);
    s.move_to(1, LEFT);
    s.focus(2);
    s.place(3, "sola-mail", "sola-mail", false, false, t);
    assert_eq!(ids(&s, RIGHT), vec![2, 3]);
    assert_eq!(s.shown(RIGHT), Some(3));
    assert_eq!(s.shown(LEFT), Some(1));
}

#[test]
fn the_first_tiled_window_takes_full() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "sola-scope", "sola-scope", true, false, t);
    assert_eq!(s.place(2, "term", "term", false, false, t).zone, FULL);
    // A desk whose tiled tabs all left starts over in Full.
    s.move_to(2, LEFT);
    s.remove(2, t);
    assert_eq!(s.place(3, "mail", "mail", false, false, t).zone, FULL);
}

#[test]
fn full_and_the_halves_take_turns() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    s.move_to(2, LEFT);
    // A lone half keeps its half; Full is off screen but keeps its tab.
    assert_eq!(s.on_screen(&none), vec![LEFT]);
    assert_eq!(s.shown(FULL), Some(3));
    s.move_to(1, RIGHT);
    assert_eq!(s.on_screen(&none), vec![LEFT, RIGHT]);
    // Focus never switches; showing a tab does.
    s.focus(3);
    assert_eq!(s.on_screen(&none), vec![LEFT, RIGHT]);
    s.show(3);
    assert_eq!(s.on_screen(&none), vec![FULL]);
    assert_eq!(s.front(&none), Some(3));
    // While Full is up, new windows join it.
    assert_eq!(s.place(4, "app", "app", false, false, t).zone, FULL);
    s.show(1);
    assert_eq!(s.on_screen(&none), vec![LEFT, RIGHT]);
    assert_eq!(s.front(&none), Some(1));
    // With the halves up, new windows join the last focused half.
    assert_eq!(s.place(5, "app", "app", false, false, t).zone, RIGHT);
}

#[test]
fn an_empty_side_gives_the_screen_to_the_other() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "app", "app", false, false, t);
    s.place(2, "app", "app", false, false, t);
    s.move_to(2, LEFT);
    s.show(1);
    assert_eq!(s.on_screen(&none), vec![FULL]);
    s.remove(1, t);
    assert_eq!(s.on_screen(&none), vec![LEFT]);
    assert_eq!(s.front(&none), Some(2));

    s.place(3, "app", "app", false, false, t);
    s.move_to(3, FULL);
    s.show(2);
    s.remove(2, t);
    assert_eq!(s.on_screen(&none), vec![FULL]);
    // A hidden app's tab puts nothing on screen either.
    let hide = |a: &str| a == "app";
    assert_eq!(s.on_screen(&hide), Vec::<Zone>::new());
    assert_eq!(s.front(&hide), None);
}

#[test]
fn floats_and_dialogs() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "sola-terminal", "sola-terminal", false, false, t);
    let p = s.place(2, "sola-scope", "sola-scope", true, false, t);
    assert_eq!(p.zone, Zone::Float);
    assert!(s.is_floating(2));
    assert_eq!(s.float_order(), &[2]);

    s.place(3, "sola-terminal", "sola-terminal", false, false, t);
    assert!(s.set_parent(3, 1));
    assert_eq!(ids(&s, FULL), vec![1]);
    assert!(s.is_floating(3));
    assert_eq!(s.children_of(1), vec![3]);
    // Showing a dialog shows its parent.
    assert!(s.show(3));
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn closing_the_shown_tab_shows_the_one_shown_before() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    s.show(1);
    s.show(3);
    // Not 2, the tab beside it: 1 was on screen before.
    s.remove(3, t);
    assert_eq!(s.shown(FULL), Some(1));
    s.remove(1, t);
    assert_eq!(s.shown(FULL), Some(2));
    s.remove(2, t);
    assert_eq!(s.shown(FULL), None);
}

#[test]
fn closing_a_tab_never_shown_falls_back_to_the_next_then_the_previous() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        // Behind: joins without being shown.
        s.place(w, "app", "app", false, w > 1, t);
    }
    assert_eq!(s.shown(FULL), Some(1));
    s.remove(1, t);
    assert_eq!(s.shown(FULL), Some(2));
}

#[test]
fn a_new_window_opens_right_after_the_focused_tab() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    s.show(1);
    s.focus(1);
    s.place(4, "app", "app", false, false, t);
    assert_eq!(ids(&s, FULL), vec![1, 4, 2, 3]);
    assert_eq!(s.shown(FULL), Some(4));
    // The shell focuses it; closing it shows the tab it came from.
    s.focus(4);
    s.remove(4, t);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn tabs_opened_behind_keep_their_order() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    s.show(1);
    s.focus(1);
    s.place(5, "app", "app", false, true, t);
    s.place(6, "app", "app", false, true, t);
    assert_eq!(ids(&s, FULL), vec![1, 5, 6, 2, 3]);
    assert_eq!(s.shown(FULL), Some(1));
    // Focus elsewhere starts over from there.
    s.show(2);
    s.focus(2);
    s.place(7, "app", "app", false, true, t);
    assert_eq!(ids(&s, FULL), vec![1, 5, 6, 2, 7, 3]);
}

#[test]
fn a_new_window_opens_after_the_focused_tab_whatever_its_zone() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "app", "app", false, false, t);
    s.place(9, "scope", "scope", true, false, t);
    s.place(2, "app", "app", false, false, t);
    assert_eq!(bar(&s), vec![1, 9, 2]);
    // Focus on the float: the new tiled window's row goes after it.
    s.focus(9);
    assert_eq!(s.place(4, "app", "app", false, false, t).zone, FULL);
    assert_eq!(bar(&s), vec![1, 9, 4, 2]);
    assert_eq!(ids(&s, FULL), vec![1, 4, 2]);
}

#[test]
fn a_restarting_app_comes_back_to_its_place() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "sola-terminal", "sola-terminal", false, false, t);
    s.place(2, "sola-browser", "sola-browser", false, false, t);
    s.place(3, "sola-mail", "sola-mail", false, false, t);
    s.show(2);
    s.remove(2, t);
    let p = s.place(9, "sola-browser", "sola-browser", false, false, t + Duration::from_secs(3));
    assert_eq!(p, Placed { zone: FULL, shown: true, returning: true });
    assert_eq!(ids(&s, FULL), vec![1, 9, 3]);

    // Past the grace, a new window is just new.
    s.remove(9, t);
    let later = t + GHOST_GRACE + Duration::from_secs(1);
    let p = s.place(10, "sola-browser", "sola-browser", false, false, later);
    assert!(!p.returning);
    assert_eq!(ids(&s, FULL), vec![1, 3, 10]);
}

#[test]
fn saved_layout_restores_in_order_whatever_order_windows_come() {
    let t = Instant::now();
    let layout = TabLayout {
        zones: vec![
            TabLayoutZone {
                zone: LEFT,
                apps: vec!["term".into(), "browser".into(), "mail".into()],
                shown: Some(1),
                workspace: "main".into(),
            },
            TabLayoutZone {
                zone: RIGHT,
                apps: vec!["design".into()],
                shown: Some(0),
                workspace: "main".into(),
            },
            TabLayoutZone {
                zone: Zone::Float,
                apps: vec!["scope".into()],
                shown: None,
                workspace: "main".into(),
            },
        ],
        workspaces: vec![Workspace {
            id: "main".into(),
            name: "Main".into(),
        }],
        showing: "main".into(),
        ..TabLayout::default()
    };
    let mut s = Stacks::new();
    s.load(&layout, t);
    s.place(3, "mail", "mail", false, false, t);
    s.place(2, "browser", "browser", false, false, t);
    s.place(4, "design", "design", false, false, t);
    s.place(1, "term", "term", false, false, t);
    s.place(5, "scope", "scope", false, false, t);
    assert_eq!(ids(&s, LEFT), vec![1, 2, 3]);
    assert_eq!(s.shown(LEFT), Some(2));
    assert_eq!(ids(&s, RIGHT), vec![4]);
    assert_eq!(s.float_order(), &[5]);
    assert_eq!(s.layout(t), layout);
}

#[test]
fn layout_keeps_places_still_waiting() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.load(
        &TabLayout {
            zones: vec![TabLayoutZone {
                zone: LEFT,
                apps: vec!["term".into(), "mail".into()],
                shown: Some(1),
                workspace: String::new(),
            }],
            ..TabLayout::default()
        },
        t,
    );
    s.place(1, "term", "term", false, false, t);
    let l = s.layout(t);
    assert_eq!(l.zones[0].apps, vec!["term".to_string(), "mail".to_string()]);
    assert_eq!(l.zones[0].shown, Some(0));
    // After the grace the unclaimed place is dropped.
    let l = s.layout(t + RESTORE_GRACE + Duration::from_secs(1));
    assert_eq!(l.zones[0].apps, vec!["term".to_string()]);
}

#[test]
fn a_window_paired_late_takes_its_tabs_saved_place() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.load(
        &TabLayout {
            zones: vec![TabLayoutZone {
                zone: RIGHT,
                apps: vec!["sola-terminal/b".into()],
                shown: Some(0),
                workspace: String::new(),
            }],
            ..TabLayout::default()
        },
        t,
    );
    // Placed before its tab was known: by app id, a new window.
    let p = s.place(4, "sola-terminal", "sola-terminal", false, false, t);
    assert_eq!(p.zone, FULL);
    let p = s.set_key(4, "sola-terminal/b", t).expect("moved");
    assert_eq!(p.zone, RIGHT);
    assert_eq!(s.bar()[0].key, "sola-terminal/b");
    // No place waiting: only the key changes.
    assert!(s.set_key(4, "sola-terminal/c", t).is_none());
    assert_eq!(s.zone_of(4), Some(RIGHT));
}

#[test]
fn a_tab_without_a_place_takes_its_apps() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.load(
        &TabLayout {
            zones: vec![TabLayoutZone {
                zone: RIGHT,
                apps: vec!["sola-terminal".into()],
                shown: Some(0),
                workspace: String::new(),
            }],
            ..TabLayout::default()
        },
        t,
    );
    let p = s.place(4, "sola-terminal", "sola-terminal/a", false, false, t);
    assert_eq!(p.zone, RIGHT);
    assert_eq!(s.bar()[0].key, "sola-terminal/a");
}

#[test]
fn returning_windows_do_not_cover_the_zone() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, true, t);
    let p = s.place(2, "browser", "browser", false, true, t);
    assert!(!p.shown);
    assert_eq!(s.shown(FULL), Some(1));
}

#[test]
fn a_move_keeps_the_row() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    assert!(s.move_to(1, RIGHT));
    assert_eq!(s.shown(RIGHT), Some(1));
    assert_eq!(ids(&s, FULL), vec![2, 3]);
    assert!(s.move_to(2, LEFT));
    assert!(s.move_to(3, LEFT));
    // A move keeps the row where it was.
    assert_eq!(bar(&s), vec![1, 2, 3]);
    assert_eq!(ids(&s, LEFT), vec![2, 3]);
}

#[test]
fn reordering_a_row_keeps_its_zone_and_what_shows() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=4 {
        s.place(w, "app", "app", false, false, t);
    }
    s.move_to(1, LEFT);
    s.move_to(2, RIGHT);
    s.move_to(3, LEFT);
    s.show(1);
    // 4 (Full) to the top, past rows of the other zones.
    assert!(s.reorder(4, Some(1)));
    assert_eq!(bar(&s), vec![4, 1, 2, 3]);
    assert_eq!(s.zone_of(4), Some(FULL));
    // 3 before 1: Left's order follows the bar; it still shows 1.
    assert!(s.reorder(3, Some(1)));
    assert_eq!(bar(&s), vec![4, 3, 1, 2]);
    assert_eq!(ids(&s, LEFT), vec![3, 1]);
    assert_eq!(s.shown(LEFT), Some(1));
    assert_eq!(s.on_screen(&none), vec![LEFT, RIGHT]);
    // To the end; onto itself or where it is, nothing moves.
    assert!(s.reorder(4, None));
    assert_eq!(bar(&s), vec![3, 1, 2, 4]);
    assert!(!s.reorder(4, None));
    assert!(!s.reorder(1, Some(1)));
    assert!(!s.reorder(3, Some(1)));
}

#[test]
fn the_saved_layout_keeps_the_bar_order_across_zones() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for (w, app) in [(1, "term"), (2, "browser"), (3, "mail")] {
        s.place(w, app, app, false, false, t);
    }
    s.move_to(1, LEFT);
    s.move_to(2, RIGHT);
    s.move_to(3, LEFT);
    // The bar is Left, Right, Left: three runs.
    let layout = s.layout(t);
    let runs: Vec<(Zone, Vec<String>, Option<u32>)> = layout
        .zones
        .iter()
        .map(|z| (z.zone, z.apps.clone(), z.shown))
        .collect();
    assert_eq!(
        runs,
        vec![
            (LEFT, vec!["term".into()], None),
            (RIGHT, vec!["browser".into()], Some(0)),
            (LEFT, vec!["mail".into()], Some(0)),
        ]
    );
    // Back in any order, the rows take their places.
    let mut s = Stacks::new();
    s.load(&layout, t);
    for (w, app) in [(7, "mail"), (5, "term"), (6, "browser")] {
        s.place(w, app, app, false, true, t);
    }
    assert_eq!(bar(&s), vec![5, 6, 7]);
    assert_eq!(s.shown(LEFT), Some(7));
    assert_eq!(s.layout(t), layout);
}

#[test]
fn super_backtick_steps_through_the_bar_whatever_the_zones() {
    let t = Instant::now();
    let mut s = Stacks::new();
    for w in 1..=3 {
        s.place(w, "app", "app", false, false, t);
    }
    s.place(9, "scope", "scope", true, false, t);
    s.move_to(1, LEFT);
    s.move_to(2, RIGHT);
    // The bar: 1 (Left), 2 (Right), 3 (Full), 9 (Float). Wrapping.
    assert_eq!(bar(&s), vec![1, 2, 3, 9]);
    assert_eq!(s.step(Some(1), false), Some(2));
    assert_eq!(s.step(Some(3), false), Some(9));
    assert_eq!(s.step(Some(9), false), Some(1));
    assert_eq!(s.step(Some(1), true), Some(9));
    // Nothing focused: the first (the last, back).
    assert_eq!(s.step(None, false), Some(1));
    assert_eq!(s.step(None, true), Some(9));
    // A dialog steps from its parent.
    s.place(5, "app", "app", false, false, t);
    s.set_parent(5, 2);
    assert_eq!(s.step(Some(5), false), Some(3));
    // Only this workspace's bar.
    s.add_workspace("Mail");
    s.place(7, "mail", "mail", false, false, t);
    assert_eq!(s.step(Some(7), false), None);
    assert_eq!(s.step(Some(1), false), Some(7));
}

#[test]
fn hidden_apps_give_way() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.place(2, "mail", "mail", false, false, t);
    let hide_mail = |a: &str| a == "mail";
    assert_eq!(s.visible_shown(FULL, &hide_mail), Some(1));
    assert_eq!(s.on_screen(&none), vec![FULL]);
    s.move_to(1, RIGHT);
    assert_eq!(s.on_screen(&hide_mail), vec![RIGHT]);
}

#[test]
fn cinema_is_a_zone_that_takes_the_whole_screen() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.place(2, "game", "game", false, false, t);
    s.move_to(1, LEFT);
    s.move_to(2, RIGHT);
    assert_eq!(s.on_screen(&none), vec![LEFT, RIGHT]);
    // Cinema covers everything: it alone is on screen.
    assert!(s.move_to(2, Zone::Cinema));
    assert_eq!(s.on_screen(&none), vec![Zone::Cinema]);
    assert_eq!(s.cinema_up(&none), Some(2));
    assert_eq!(s.cinema_return(2), RIGHT);
    // Showing another tab takes Cinema off; nothing toggles.
    s.show(1);
    assert_eq!(s.on_screen(&none), vec![LEFT]);
    assert_eq!(s.cinema_up(&none), None);
    assert_eq!(s.zone_of(2), Some(Zone::Cinema));
}

#[test]
fn a_window_opened_while_cinema_is_up_goes_to_full() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.move_to(1, LEFT);
    s.place(2, "game", "game", false, false, t);
    s.move_to(2, Zone::Cinema);
    s.focus(2);
    assert_eq!(s.place(3, "mail", "mail", false, false, t).zone, FULL);
    assert_eq!(s.on_screen(&none), vec![FULL]);
}

#[test]
fn overlapping_grid_zones_take_turns() {
    use sola_bus::topics::{GridZone, ZoneLayout};
    let t = Instant::now();
    let mut s = Stacks::new();
    // Three columns, and a zone over the right two.
    s.set_layout(ZoneLayout {
        rows: 1,
        columns: 3,
        zones: vec![
            GridZone { id: 1, row: 0, column: 0, rows: 1, columns: 1 },
            GridZone { id: 2, row: 0, column: 1, rows: 1, columns: 1 },
            GridZone { id: 3, row: 0, column: 2, rows: 1, columns: 1 },
            GridZone { id: 4, row: 0, column: 1, rows: 1, columns: 2 },
        ],
    });
    for (w, z) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        s.place(w, "app", &format!("app/{w}"), false, false, t);
        s.move_to(w, Zone::Grid(z));
    }
    s.show(1);
    s.show(4);
    assert_eq!(s.on_screen(&none), vec![Zone::Grid(1), Zone::Grid(4)]);
    // Picking column 2 takes the wide zone off and brings column 3 back.
    s.show(2);
    assert_eq!(s.on_screen(&none), vec![Zone::Grid(1), Zone::Grid(2), Zone::Grid(3)]);
}

#[test]
fn a_gone_zone_s_tabs_move_to_full() {
    use sola_bus::topics::{GridZone, ZoneLayout};
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.move_to(1, RIGHT);
    s.focus(1);
    assert!(s.set_layout(ZoneLayout {
        rows: 1,
        columns: 1,
        zones: vec![GridZone { id: 1, row: 0, column: 0, rows: 1, columns: 1 }],
    }));
    assert_eq!(s.zone_of(1), Some(FULL));
    assert_eq!(s.shown(FULL), Some(1));
    assert!(!s.move_to(1, RIGHT));
    assert_eq!(s.place(2, "mail", "mail", false, false, t).zone, FULL);
}

fn layout_of(zones: Vec<(&str, Zone, Vec<&str>, Option<u32>)>, workspaces: &[(&str, &str)], showing: &str) -> TabLayout {
    TabLayout {
        zones: zones
            .into_iter()
            .map(|(ws, zone, apps, shown)| TabLayoutZone {
                zone,
                apps: apps.into_iter().map(String::from).collect(),
                shown,
                workspace: ws.into(),
            })
            .collect(),
        workspaces: workspaces
            .iter()
            .map(|(id, name)| Workspace {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
        showing: showing.into(),
        ..TabLayout::default()
    }
}

#[test]
fn readers_answer_for_the_showing_workspace() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    let second = s.add_workspace("Mail");
    assert_eq!(s.showing(), second);
    // A new window joins the showing workspace.
    s.place(2, "mail", "mail", false, false, t);
    assert_eq!(ids(&s, FULL), vec![2]);
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
    assert_eq!(s.zone_of(1), Some(FULL));
    assert!(!s.on_showing(1));
    assert!(s.on_showing(2));
    assert!(s.show_workspace(0));
    assert_eq!(ids(&s, FULL), vec![1]);
    assert_eq!(s.bar_on(second)[0].window_id, 2);
}

#[test]
fn showing_a_window_shows_its_workspace() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.add_workspace("Mail");
    s.place(2, "mail", "mail", false, false, t);
    assert!(s.show(1));
    assert_eq!(s.showing(), 0);
    // A dialog shows its parent's workspace.
    s.place(3, "mail", "mail", false, false, t);
    s.set_parent(3, 2);
    assert_eq!(s.workspace_of(3), s.workspace_of(2));
    assert!(s.show(3));
    assert_eq!(s.showing(), 1);
}

#[test]
fn sending_a_window_keeps_its_zone_and_shows_it_there() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.place(2, "browser", "browser", false, false, t);
    s.move_to(2, RIGHT);
    s.place(3, "scope", "scope", true, false, t);
    let mail = s.add_workspace("Mail");
    s.show_workspace(0);
    assert!(s.send(2, mail));
    assert!(s.send(3, mail));
    assert!(!s.send(3, mail), "already there");
    assert_eq!(ids(&s, RIGHT), Vec::<u32>::new());
    assert_eq!(s.float_order(), &[] as &[u32]);
    assert_eq!(s.bar_on(mail)[0].window_id, 2);
    assert!(s.show_workspace(mail));
    assert_eq!(s.shown(RIGHT), Some(2));
    assert_eq!(s.float_order(), &[3]);
    // A tab sent to Full brings Full up there.
    s.show_workspace(0);
    assert!(s.send(1, mail));
    s.show_workspace(mail);
    assert_eq!(s.on_screen(&none), vec![FULL]);
}

#[test]
fn deleting_a_workspace_moves_its_windows_back_one() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    let mail = s.add_workspace("Mail");
    s.place(2, "mail", "mail", false, false, t);
    assert!(s.delete_workspace(mail));
    assert_eq!(s.workspace_count(), 1);
    assert_eq!(s.showing(), 0);
    assert_eq!(ids(&s, FULL), vec![1, 2]);
    assert_eq!(s.shown(FULL), Some(2));
    assert!(!s.delete_workspace(0), "the last one stays");
}

#[test]
fn rename_and_reorder_keep_the_showing_one() {
    let mut s = Stacks::new();
    let mail = s.add_workspace("Mail");
    assert!(s.rename_workspace(mail, "  Post "));
    assert!(!s.rename_workspace(mail, "   "));
    assert!(s.move_workspace(mail, true));
    assert_eq!(s.workspaces()[0].name, "Post");
    assert_eq!(s.showing(), 0);
    assert_eq!(s.showing_id(), s.workspaces()[0].id);
    assert!(!s.move_workspace(0, true));
    assert_eq!(s.step_workspace(false), 1);
    assert_eq!(s.step_workspace(true), 1);
}

#[test]
fn the_saved_layout_brings_every_workspace_back() {
    let t = Instant::now();
    let layout = layout_of(
        vec![
            ("main", LEFT, vec!["term"], Some(0)),
            ("ws-mail", LEFT, vec!["mail", "calendar"], Some(1)),
            ("ws-mail", Zone::Float, vec!["scope"], None),
        ],
        &[("main", "Main"), ("ws-mail", "Mail")],
        "ws-mail",
    );
    let mut s = Stacks::new();
    s.load(&layout, t);
    assert_eq!(s.showing(), 1);
    s.place(1, "calendar", "calendar", false, true, t);
    s.place(2, "term", "term", false, true, t);
    s.place(3, "mail", "mail", false, true, t);
    s.place(4, "scope", "scope", false, true, t);
    assert_eq!(ids(&s, LEFT), vec![3, 1]);
    assert_eq!(s.shown(LEFT), Some(1));
    assert_eq!(s.float_order(), &[4]);
    assert_eq!(s.workspace_of(2), Some("main"));
    assert_eq!(s.layout(t), layout);
}

#[test]
fn the_saved_layout_remembers_which_side_is_up() {
    let t = Instant::now();
    let mut layout = layout_of(
        vec![
            ("main", FULL, vec!["browser"], Some(0)),
            ("main", LEFT, vec!["term"], Some(0)),
            ("ws-mail", FULL, vec!["mail"], Some(0)),
            ("ws-mail", RIGHT, vec!["calendar"], Some(0)),
        ],
        &[("main", "Main"), ("ws-mail", "Mail")],
        "main",
    );
    layout.full = vec!["ws-mail".into()];
    let mut s = Stacks::new();
    s.load(&layout, t);
    for (w, app) in [(1, "browser"), (2, "term"), (3, "mail"), (4, "calendar")] {
        s.place(w, app, app, false, true, t);
    }
    assert_eq!(s.on_screen(&none), vec![LEFT]);
    s.show_workspace(1);
    assert_eq!(s.on_screen(&none), vec![FULL]);
    s.show_workspace(0);
    // An older file's `full` comes back as the workspace's pick order.
    layout.picks = vec![sola_bus::topics::WorkspacePicks {
        workspace: "ws-mail".into(),
        zones: vec![FULL],
    }];
    assert_eq!(s.layout(t), layout);
    // And a file with picks reads them back.
    let mut again = Stacks::new();
    again.load(&layout, t);
    assert_eq!(again.layout(t).picks, layout.picks);
}

#[test]
fn a_layout_from_before_workspaces_is_the_first_ones() {
    let t = Instant::now();
    let old = layout_of(vec![("", RIGHT, vec!["term"], Some(0))], &[], "");
    let mut s = Stacks::new();
    s.load(&old, t);
    s.place(1, "term", "term", false, true, t);
    assert_eq!(s.zone_of(1), Some(RIGHT));
    assert_eq!(s.workspace_of(1), Some(FIRST_WORKSPACE.0));
    let saved = s.layout(t);
    assert_eq!(saved.showing, FIRST_WORKSPACE.0);
    assert_eq!(saved.zones[0].workspace, FIRST_WORKSPACE.0);
}

#[test]
fn a_restarting_app_comes_back_to_its_workspace() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "term", "term", false, false, t);
    s.add_workspace("Mail");
    s.place(2, "mail", "mail", false, false, t);
    s.show_workspace(0);
    s.remove(2, t);
    let p = s.place(7, "mail", "mail", false, false, t + Duration::from_secs(2));
    assert!(p.returning);
    assert_eq!(s.workspace_of(7), Some(s.workspaces()[1].id.as_str()));
    assert_eq!(s.showing(), 0);
}

#[test]
fn the_landing_slot_is_where_the_window_lands() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.place(1, "sola-terminal", "sola-terminal", false, false, t);
    s.place(2, "sola-browser", "sola-browser", false, false, t);
    s.place(3, "sola-mail", "sola-mail", false, false, t);
    s.focus(1);
    // After the focused tab, before the ones after it.
    let (zone, at, opener) = s.landing_slot(false);
    assert_eq!((zone, at, opener), (FULL, 1, Some(1)));
    s.place(4, "sola-calendar", "sola-calendar", false, false, t);
    assert_eq!(ids(&s, FULL), vec![1, 4, 2, 3]);
    // A floating app lands in Float, its row after the focused tab too.
    s.focus(4);
    assert_eq!(s.landing_slot(true), (Zone::Float, 2, Some(4)));
}
