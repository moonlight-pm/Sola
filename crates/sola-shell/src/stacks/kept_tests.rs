//! Kept apps: a row of their own while they have no window, which their
//! next window takes, in place (`design/sidebar-keep/`, way B).

use std::time::{Duration, Instant};

use super::*;

const STEAM: &str = "steam";
const PAINT: &str = "sola-paint";
const MAIL: &str = "sola-mail";
const SETTINGS: &str = "sola-settings";

/// The showing workspace's bar: a window's row as its id, a kept app's as
/// `+app`, a group as `app[ids]`.
fn rows(s: &Stacks) -> Vec<String> {
    rows_on(s, s.showing())
}

fn rows_on(s: &Stacks, d: usize) -> Vec<String> {
    s.desks[d]
        .bar
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Loose(t) => Some(t.window_id.to_string()),
            Block::Kept { app, .. } => Some(format!("+{app}")),
            Block::Group { tabs, .. } if tabs.is_empty() => None,
            Block::Group { app, tabs, .. } => {
                let ids: Vec<String> = tabs.iter().map(|t| t.window_id.to_string()).collect();
                Some(format!("{app}[{}]", ids.join(",")))
            }
        })
        .collect()
}

fn open(s: &mut Stacks, window_id: u32, app: &str, t: Instant) {
    s.place(window_id, app, app, false, false, t);
    s.focus(window_id);
}

fn kept(apps: &[&str]) -> Vec<String> {
    apps.iter().map(|a| a.to_string()).collect()
}

#[test]
fn a_kept_app_with_no_window_has_a_row_its_window_takes_and_leaves_in_place() {
    let t = Instant::now();
    let mut s = Stacks::new();
    open(&mut s, 1, MAIL, t);
    assert!(s.set_kept(&kept(&[STEAM])));
    assert_eq!(rows(&s), ["1", "+steam"], "at the end of the loose rows");
    // Mail's next window lands after Mail, before Steam's row.
    open(&mut s, 2, SETTINGS, t);
    assert_eq!(rows(&s), ["1", "2", "+steam"]);
    s.focus(1);
    open(&mut s, 3, STEAM, t);
    assert_eq!(rows(&s), ["1", "2", "3"], "Steam's window takes its row");
    s.remove(3, t);
    assert_eq!(rows(&s), ["1", "2", "+steam"], "and leaves it where it was");
    // Moved: it stays where it was put.
    assert!(s.move_block(&BlockId::Kept(STEAM.into()), Some(&BlockId::Window(1))));
    assert_eq!(rows(&s), ["+steam", "1", "2"]);
    // Past the time a restart has to come back to the place it left.
    open(&mut s, 4, STEAM, t + Duration::from_secs(60));
    assert_eq!(rows(&s), ["4", "1", "2"]);
}

#[test]
fn kept_apps_closed_in_turn_keep_their_order() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.set_kept(&kept(&[STEAM, MAIL]));
    open(&mut s, 1, STEAM, t);
    open(&mut s, 2, MAIL, t);
    open(&mut s, 3, SETTINGS, t);
    s.reorder(3, Some(1));
    assert_eq!(rows(&s), ["3", "1", "2"]);
    s.remove(1, t);
    s.remove(2, t);
    assert_eq!(rows(&s), ["3", "+steam", "+sola-mail"]);
    s.remove(3, t);
    assert_eq!(rows(&s), ["+steam", "+sola-mail"]);
}

#[test]
fn an_app_with_many_windows_keeps_one_row_after_its_last() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.set_kept(&kept(&[PAINT]));
    open(&mut s, 1, MAIL, t);
    s.focus(1);
    open(&mut s, 2, PAINT, t);
    open(&mut s, 3, PAINT, t);
    assert_eq!(rows(&s), ["1", "sola-paint[2,3]"]);
    s.remove(2, t);
    assert_eq!(rows(&s), ["1", "3"], "one left: a loose row");
    s.remove(3, t);
    assert_eq!(rows(&s), ["1", "+sola-paint"]);
}

#[test]
fn letting_go_takes_the_row_and_a_window_back_at_its_place_takes_it_too() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.set_kept(&kept(&[STEAM]));
    open(&mut s, 1, MAIL, t);
    s.focus(1);
    open(&mut s, 2, STEAM, t);
    s.remove(2, t);
    assert_eq!(rows(&s), ["1", "+steam"]);
    // Steam restarting: its window comes back to the place it left, and
    // the row goes.
    open(&mut s, 3, STEAM, t + Duration::from_secs(1));
    assert_eq!(rows(&s), ["1", "3"]);
    s.remove(3, t + Duration::from_secs(2));
    assert!(s.set_kept(&[]));
    assert_eq!(rows(&s), ["1"]);
}

#[test]
fn a_kept_window_sent_away_leaves_its_row_where_it_closes() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.add_workspace("Games");
    s.show_workspace(0);
    s.set_kept(&kept(&[STEAM]));
    open(&mut s, 1, STEAM, t);
    assert!(s.send(1, 1));
    assert_eq!(rows_on(&s, 0), Vec::<String>::new(), "no row while it has a window");
    s.remove(1, t);
    assert_eq!(rows_on(&s, 1), ["+steam"]);
    assert!(!s.has_kept_row(STEAM), "not on the showing workspace");
    // Started from here later, its window lands here and the row there goes.
    open(&mut s, 2, STEAM, t + Duration::from_secs(60));
    assert_eq!(rows_on(&s, 0), ["2"]);
    assert_eq!(rows_on(&s, 1), Vec::<String>::new());
}

#[test]
fn the_saved_layout_keeps_the_rows_and_windows_come_back_around_them() {
    let t = Instant::now();
    let mut s = Stacks::new();
    s.set_kept(&kept(&[STEAM]));
    open(&mut s, 1, MAIL, t);
    s.focus(1);
    open(&mut s, 2, STEAM, t);
    open(&mut s, 3, SETTINGS, t);
    s.remove(2, t);
    assert_eq!(rows(&s), ["1", "+steam", "3"]);
    let layout = s.layout(t + Duration::from_secs(60));
    assert_eq!(layout.kept.len(), 1);

    // A new shell: the layout, then the windows coming back in any order,
    // and the kept apps said after.
    let mut s = Stacks::new();
    s.load(&layout, t);
    open(&mut s, 13, SETTINGS, t);
    open(&mut s, 11, MAIL, t);
    assert_eq!(rows(&s), ["11", "+steam", "13"]);
    s.set_kept(&kept(&[STEAM]));
    assert_eq!(rows(&s), ["11", "+steam", "13"]);
}
