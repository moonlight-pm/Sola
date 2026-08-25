//! Tab catalog — port of `/home/joshua/Workspace/Scratch/tabs.js`.

use crate::strip::{DropDest, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Group(Group),
    Item(Item),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub selected_id: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub id: String,
    pub kind: Kind,
    pub group: Option<String>,
    pub label: String,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub grouped: bool,
    pub start: usize,
    pub len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub leaves: Vec<Leaf>,
    pub spans: Vec<Span>,
    pub selected_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Activate { id: String },
    Toggle { id: String },
    Drop { id: String, dest: DropDest },
}

fn item(id: &str, label: &str) -> Item {
    Item {
        id: id.into(),
        label: label.into(),
    }
}

fn group(id: &str, name: &str, labels: &[&str]) -> Block {
    Block::Group(Group {
        id: id.into(),
        name: name.into(),
        collapsed: false,
        items: labels
            .iter()
            .enumerate()
            .map(|(i, label)| item(&format!("{}{}", id, i + 1), label))
            .collect(),
    })
}

fn singleton(id: &str, label: &str) -> Block {
    Block::Item(item(id, label))
}

pub fn create_store() -> Store {
    Store {
        selected_id: "a1".into(),
        blocks: vec![
            group(
                "a",
                "Group A",
                &[
                    "Home | Patreon",
                    "No Agenda 1885: Adult Day Care",
                    "Add New Series - Sonarr",
                    "TV Series/TV Mini Series, Release date at",
                    "Brideshead Revisited (TV Mini Series 198",
                    "Queue - Radarr",
                    "SABnzbd",
                    "YouTube",
                    "YouTube History",
                ],
            ),
            group(
                "b",
                "Group B",
                &["Item B1", "Item B2", "Item B3", "Item B4", "Item B5"],
            ),
            group(
                "c",
                "Group C",
                &["Item C1", "Item C2", "Item C3", "Item C4", "Item C5"],
            ),
            singleton("u1", "Item U1"),
            singleton("u2", "Item U2"),
            singleton("u3", "Item U3"),
            singleton("u4", "Item U4"),
            singleton("u5", "Item U5"),
        ],
    }
}

pub fn snapshot(store: &Store) -> Snapshot {
    let mut leaves = Vec::new();
    let mut spans = Vec::new();
    for block in &store.blocks {
        match block {
            Block::Group(g) => {
                let start = leaves.len();
                leaves.push(Leaf {
                    id: g.id.clone(),
                    kind: Kind::Header,
                    group: Some(g.id.clone()),
                    label: g.name.clone(),
                    collapsed: g.collapsed,
                });
                if !g.collapsed {
                    for it in &g.items {
                        leaves.push(Leaf {
                            id: it.id.clone(),
                            kind: Kind::Item,
                            group: Some(g.id.clone()),
                            label: it.label.clone(),
                            collapsed: false,
                        });
                    }
                }
                spans.push(Span {
                    grouped: true,
                    start,
                    len: leaves.len() - start,
                });
            }
            Block::Item(it) => {
                leaves.push(Leaf {
                    id: it.id.clone(),
                    kind: Kind::Item,
                    group: None,
                    label: it.label.clone(),
                    collapsed: false,
                });
                spans.push(Span {
                    grouped: false,
                    start: leaves.len() - 1,
                    len: 1,
                });
            }
        }
    }
    Snapshot {
        leaves,
        spans,
        selected_id: store.selected_id.clone(),
    }
}

fn take_item(store: &mut Store, id: &str) -> Option<Item> {
    let mut found: Option<(usize, Option<usize>)> = None;
    for (i, block) in store.blocks.iter().enumerate() {
        match block {
            Block::Group(g) => {
                if let Some(j) = g.items.iter().position(|it| it.id == id) {
                    found = Some((i, Some(j)));
                    break;
                }
            }
            Block::Item(it) if it.id == id => {
                found = Some((i, None));
                break;
            }
            _ => {}
        }
    }
    let (i, maybe_j) = found?;
    if let Some(j) = maybe_j {
        let Block::Group(g) = &mut store.blocks[i] else {
            return None;
        };
        let it = g.items.remove(j);
        if g.items.is_empty() {
            store.blocks.remove(i);
        }
        Some(it)
    } else {
        match store.blocks.remove(i) {
            Block::Item(it) => Some(it),
            _ => None,
        }
    }
}

pub fn group_has_selected(store: &Store, group_id: &str, selected_id: &str) -> bool {
    store.blocks.iter().any(|b| match b {
        Block::Group(g) if g.id == group_id => g.items.iter().any(|it| it.id == selected_id),
        _ => false,
    })
}

pub fn apply_event(store: &mut Store, ev: Event) {
    match ev {
        Event::Activate { id } => store.selected_id = id,
        Event::Toggle { id } => {
            if let Some(Block::Group(g)) = store.blocks.iter_mut().find(|b| match b {
                Block::Group(g) => g.id == id,
                _ => false,
            }) {
                g.collapsed = !g.collapsed;
            }
        }
        Event::Drop { id, dest } => {
            let Some(item) = take_item(store, &id) else {
                return;
            };
            match dest {
                DropDest::Join { section, before } => {
                    let Some(g) = store.blocks.iter_mut().find_map(|b| match b {
                        Block::Group(g) if g.id == section => Some(g),
                        _ => None,
                    }) else {
                        store.blocks.push(Block::Item(item));
                        return;
                    };
                    let at = before
                        .as_ref()
                        .and_then(|b| g.items.iter().position(|it| it.id == *b))
                        .unwrap_or(g.items.len());
                    g.items.insert(at, item);
                }
                DropDest::Loose { before } => {
                    let at = before
                        .as_ref()
                        .and_then(|b| {
                            store.blocks.iter().position(|blk| match blk {
                                Block::Item(it) => it.id == *b,
                                _ => false,
                            })
                        })
                        .unwrap_or(store.blocks.len());
                    store.blocks.insert(at, Block::Item(item));
                }
                DropDest::BeforeGroup { id: gid } => {
                    let at = store
                        .blocks
                        .iter()
                        .position(|b| match b {
                            Block::Group(g) => g.id == gid,
                            _ => false,
                        })
                        .unwrap_or(store.blocks.len());
                    store.blocks.insert(at, Block::Item(item));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(store: &Store) -> Vec<String> {
        snapshot(store).leaves.into_iter().map(|l| l.id).collect()
    }

    #[test]
    fn fixture_order() {
        let s = create_store();
        let id = ids(&s);
        assert_eq!(&id[..3], ["a", "a1", "a2"]);
        assert_eq!(id.last().map(String::as_str), Some("u5"));
    }

    #[test]
    fn join_u1_into_c_before_c3() {
        let mut s = create_store();
        apply_event(
            &mut s,
            Event::Drop {
                id: "u1".into(),
                dest: DropDest::Join {
                    section: "c".into(),
                    before: Some("c3".into()),
                },
            },
        );
        let leaves = ids(&s);
        let u1 = leaves.iter().position(|x| x == "u1").unwrap();
        let c3 = leaves.iter().position(|x| x == "c3").unwrap();
        assert_eq!(u1, c3 - 1);
        assert!(
            !s.blocks
                .iter()
                .any(|b| matches!(b, Block::Item(it) if it.id == "u1"))
        );
    }

    #[test]
    fn leave_c4_as_loose_before_u1() {
        let mut s = create_store();
        apply_event(
            &mut s,
            Event::Drop {
                id: "c4".into(),
                dest: DropDest::Loose {
                    before: Some("u1".into()),
                },
            },
        );
        let leaves = ids(&s);
        let c4 = leaves.iter().position(|x| x == "c4").unwrap();
        let u1 = leaves.iter().position(|x| x == "u1").unwrap();
        let c5 = leaves.iter().position(|x| x == "c5").unwrap();
        assert_eq!(c4, u1 - 1);
        assert_eq!(c5 + 1, c4);
    }

    #[test]
    fn last_member_out_dissolves_group() {
        let mut s = create_store();
        for id in ["c1", "c2", "c3", "c4", "c5"] {
            apply_event(
                &mut s,
                Event::Drop {
                    id: id.into(),
                    dest: DropDest::Loose {
                        before: Some("u1".into()),
                    },
                },
            );
        }
        assert!(
            !s.blocks
                .iter()
                .any(|b| matches!(b, Block::Group(g) if g.id == "c"))
        );
        assert!(!ids(&s).iter().any(|id| id == "c"));
    }

    #[test]
    fn reorder_within_group() {
        let mut s = create_store();
        apply_event(
            &mut s,
            Event::Drop {
                id: "a1".into(),
                dest: DropDest::Join {
                    section: "a".into(),
                    before: Some("a4".into()),
                },
            },
        );
        let Block::Group(a) = &s.blocks[0] else {
            panic!("expected group A");
        };
        let got: Vec<&str> = a.items.iter().map(|it| it.id.as_str()).collect();
        assert_eq!(got, ["a2", "a3", "a1", "a4", "a5"]);
    }
}
