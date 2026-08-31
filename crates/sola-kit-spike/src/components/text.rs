//! Text roles. Components name a role, never a raw size.

use crate::dom::Elem;
use crate::markup;

pub fn heading(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["heading"], None, None, text)
}

pub fn title(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["card-title"], None, None, text)
}

pub fn lede(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["card-body"], None, None, text)
}

pub fn body(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["t-body"], None, None, text)
}

pub fn caption(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["t-caption"], None, None, text)
}

pub fn muted(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["t-body", "t-muted"], None, None, text)
}

pub fn sub(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["t-sub", "t-muted"], None, None, text)
}

pub fn danger(next: &mut u32, text: &str) -> Elem {
    markup::node(next, &["help-danger"], None, None, text)
}

pub fn bind_caption(next: &mut u32, bind: &str) -> Elem {
    let mut el = markup::node(next, &["t-caption"], None, None, "");
    el.data_bind = Some(bind.into());
    el
}
