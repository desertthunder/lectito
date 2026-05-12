use std::{collections::HashMap, io};

use kuchiki::NodeRef;

use super::error::Error;
use super::patterns;

pub(crate) fn inner_text(node: &NodeRef) -> String {
    patterns::normalize_spaces(node.text_contents().trim())
}

pub(crate) fn is_kuchiki_visible(node: &NodeRef) -> bool {
    if attr(node, "hidden").is_some() {
        return false;
    }
    if patterns::has_display_none(attr(node, "style").as_deref()) {
        return false;
    }
    if attr(node, "aria-hidden").as_deref() == Some("true")
        && !attr(node, "class").is_some_and(|class| class.contains("fallback-image"))
    {
        return false;
    }
    true
}

pub(crate) fn has_unlikely_role(node: &NodeRef) -> bool {
    matches!(
        attr(node, "role").as_deref(),
        Some("menu" | "menubar" | "complementary" | "navigation" | "alert" | "alertdialog" | "dialog")
    )
}

pub(crate) fn has_ancestor_tag(node: &NodeRef, tag: &str, max_depth: usize) -> bool {
    for (depth, ancestor) in node
        .ancestors()
        .filter(|node| node.as_element().is_some())
        .skip(1)
        .enumerate()
    {
        if depth > max_depth {
            return false;
        }
        if node_name(&ancestor) == tag {
            return true;
        }
    }
    false
}

pub(crate) fn remove_matching(root: &NodeRef, selector: &str) {
    for node in select_nodes(root, selector) {
        node.detach();
    }
}

pub(crate) fn select_nodes(root: &NodeRef, selector: &str) -> Vec<NodeRef> {
    root.select(selector)
        .map(|nodes| nodes.map(|node| node.as_node().clone()).collect())
        .unwrap_or_default()
}

pub(crate) fn node_name(node: &NodeRef) -> String {
    node.as_element()
        .map(|element| element.name.local.to_string())
        .unwrap_or_default()
}

pub(crate) fn class_id_string(node: &NodeRef) -> String {
    format!(
        "{} {}",
        attr(node, "class").unwrap_or_default(),
        attr(node, "id").unwrap_or_default()
    )
}

pub(crate) fn attrs(node: &NodeRef) -> HashMap<String, String> {
    node.as_element()
        .map(|element| {
            element
                .attributes
                .borrow()
                .map
                .iter()
                .map(|(name, value)| (name.local.to_string(), value.value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn attr(node: &NodeRef, name: &str) -> Option<String> {
    node.as_element()?.attributes.borrow().get(name).map(str::to_string)
}

pub(crate) fn set_attr(node: &NodeRef, name: &str, value: &str) {
    if let Some(element) = node.as_element() {
        element.attributes.borrow_mut().insert(name, value.to_string());
    }
}

pub(crate) fn remove_attr(node: &NodeRef, name: &str) {
    if let Some(element) = node.as_element() {
        element.attributes.borrow_mut().remove(name);
    }
}

pub(crate) fn serialize_node(node: &NodeRef) -> Result<String, Error> {
    let mut bytes = Vec::new();
    node.serialize(&mut bytes)
        .map_err(|_: io::Error| Error::Serialization)?;
    String::from_utf8(bytes).map_err(|_| Error::Serialization)
}

pub(crate) fn node_id(node: &NodeRef) -> usize {
    (&**node) as *const _ as usize
}
