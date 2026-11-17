//! XML helpers (matched by local name: the same shapes appear as p:sp, dsp:sp, …)

use super::*;

pub(super) fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

pub(super) fn children<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

pub(super) fn path<'a, 'i>(node: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    names.iter().try_fold(node, |n, name| child(n, name))
}

pub(super) fn num(node: Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().parse().ok().filter(|v: &f32| v.is_finite())
}

pub(super) fn flag(node: Node, name: &str) -> Option<bool> {
    node.attribute(name).map(|v| v == "1" || v == "true" || v == "on")
}

/// A length in points, within what a slide can hold (PowerPoint's limit is 56 in)
pub(super) fn emu(node: Node, name: &str) -> Option<f32> {
    num(node, name).map(|v| (v / EMU).clamp(-100_000.0, 100_000.0))
}
