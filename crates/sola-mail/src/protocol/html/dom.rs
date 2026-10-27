//! A small HTML5 DOM for letters: html5ever's tree builder into `Rc`
//! nodes (the shape of html5ever's own `rcdom`, which is not published for
//! 0.31). Only what the letter walker reads is kept: elements with their
//! attributes, text, and the tree. Comments, doctypes and processing
//! instructions become inert nodes.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use html5ever::interface::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, LocalName, ParseOpts, QualName, parse_document};

pub type Handle = Rc<Node>;

pub enum Data {
    Document,
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        /// A `<template>`'s contents (never rendered).
        template: Option<Handle>,
    },
    Text(RefCell<String>),
    /// Comment, doctype, processing instruction.
    Other,
}

pub struct Node {
    pub data: Data,
    parent: Cell<Option<Weak<Node>>>,
    pub children: RefCell<Vec<Handle>>,
}

impl Node {
    fn new(data: Data) -> Handle {
        Rc::new(Node {
            data,
            parent: Cell::new(None),
            children: RefCell::new(Vec::new()),
        })
    }

    /// The element's local name (`p`, `td`), lowercase.
    pub fn tag(&self) -> Option<&LocalName> {
        match &self.data {
            Data::Element { name, .. } => Some(&name.local),
            _ => None,
        }
    }

    /// An attribute's value.
    pub fn attr(&self, key: &str) -> Option<String> {
        match &self.data {
            Data::Element { attrs, .. } => attrs
                .borrow()
                .iter()
                .find(|a| a.name.local.as_ref().eq_ignore_ascii_case(key))
                .map(|a| a.value.to_string()),
            _ => None,
        }
    }

    fn parent(&self) -> Option<Handle> {
        let weak = self.parent.take();
        let strong = weak.as_ref().and_then(Weak::upgrade);
        self.parent.set(weak);
        strong
    }
}

/// Parse a whole document (mail bodies are documents or fragments that
/// html5ever wraps in one).
pub fn parse(html: &str) -> Handle {
    parse_document(Sink::default(), ParseOpts::default())
        .from_utf8()
        .one(html.as_bytes())
}

struct Sink {
    document: Handle,
}

impl Default for Sink {
    fn default() -> Self {
        Self {
            document: Node::new(Data::Document),
        }
    }
}

fn append(parent: &Handle, child: Handle) {
    child.parent.set(Some(Rc::downgrade(parent)));
    parent.children.borrow_mut().push(child);
}

/// Append text, merging into a preceding text node.
fn append_text(parent: &Handle, text: &str) {
    if let Some(last) = parent.children.borrow().last()
        && let Data::Text(t) = &last.data
    {
        t.borrow_mut().push_str(text);
        return;
    }
    append(parent, Node::new(Data::Text(RefCell::new(text.to_string()))));
}

fn detach(target: &Handle) {
    if let Some(parent) = target.parent() {
        parent
            .children
            .borrow_mut()
            .retain(|c| !Rc::ptr_eq(c, target));
    }
    target.parent.set(None);
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Handle;
    type ElemName<'a> = &'a QualName;

    fn finish(self) -> Handle {
        self.document
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        self.document.clone()
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        match &target.data {
            Data::Element { name, .. } => name,
            _ => panic!("not an element"),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let template = flags.template.then(|| Node::new(Data::Document));
        Node::new(Data::Element {
            name,
            attrs: RefCell::new(attrs),
            template,
        })
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        Node::new(Data::Other)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        Node::new(Data::Other)
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        match child {
            NodeOrText::AppendNode(node) => append(parent, node),
            NodeOrText::AppendText(text) => append_text(parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if element.parent().is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &Handle) -> Handle {
        match &target.data {
            Data::Element {
                template: Some(contents),
                ..
            } => contents.clone(),
            _ => panic!("not a template element"),
        }
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        Rc::ptr_eq(x, y)
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let Some(parent) = sibling.parent() else {
            return;
        };
        let at = parent
            .children
            .borrow()
            .iter()
            .position(|c| Rc::ptr_eq(c, sibling))
            .unwrap_or(0);
        let node = match child {
            NodeOrText::AppendNode(node) => {
                detach(&node);
                node
            }
            NodeOrText::AppendText(text) => {
                // Merge into the text node just before the sibling.
                if at > 0
                    && let Data::Text(t) = &parent.children.borrow()[at - 1].data
                {
                    t.borrow_mut().push_str(&text);
                    return;
                }
                Node::new(Data::Text(RefCell::new(text.to_string())))
            }
        };
        node.parent.set(Some(Rc::downgrade(&parent)));
        parent.children.borrow_mut().insert(at, node);
    }

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if let Data::Element { attrs: have, .. } = &target.data {
            let mut have = have.borrow_mut();
            for attr in attrs {
                if !have.iter().any(|a| a.name == attr.name) {
                    have.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        detach(target);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let children = std::mem::take(&mut *node.children.borrow_mut());
        for child in children {
            append(new_parent, child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(node: &Handle, out: &mut String) {
        match &node.data {
            Data::Text(t) => out.push_str(&t.borrow()),
            _ => {
                for c in node.children.borrow().iter() {
                    text_of(c, out);
                }
            }
        }
    }

    #[test]
    fn parses_a_fragment_into_a_document() {
        let doc = parse("<p>Hi <b>there</b></p><table><tr><td>a</td></tr></table>");
        let mut out = String::new();
        text_of(&doc, &mut out);
        assert_eq!(out, "Hi therea");
    }

    #[test]
    fn misnested_markup_still_reads_in_order() {
        // Foster-parented text and an unclosed <b> exercise the
        // before-sibling and reparent paths.
        let doc = parse("<table>x<tr><td>cell</td></tr></table><p><b>one<p>two</b>");
        let mut out = String::new();
        text_of(&doc, &mut out);
        assert_eq!(out, "xcellonetwo");
    }
}
