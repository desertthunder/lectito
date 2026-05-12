use kuchiki::NodeRef;
use regex::Regex;
use url::Url;

use super::config::{ExtractFlags, ReadabilityOptions};
use super::dom;
use super::patterns::{
    AD_OR_LOADING_WORDS, COMMA, DEFAULT_CLASSES_TO_PRESERVE, DEPRECATED_SIZE_ATTRIBUTE_ELEMS,
    PRESENTATIONAL_ATTRIBUTES, SHARE_ELEMENTS,
};
use super::scoring::{class_weight, link_density};

pub(crate) fn cleanup_article(
    nodes: &[NodeRef], options: &ReadabilityOptions, flags: ExtractFlags, base_url: Option<&Url>,
) {
    for node in nodes {
        clean_styles(node);
        fix_lazy_images(node);
        dom::remove_matching(
            node,
            "script, style, form, fieldset, object, embed, footer, link, aside, iframe",
        );
        remove_share_nodes(node);
        if flags.clean_conditionally {
            clean_conditionally(node, options, flags);
        }
        remove_empty_blocks(node);
        fix_relative_urls(node, base_url);
        if !options.keep_classes {
            clean_classes(node, options);
        }
    }
}

fn clean_styles(node: &NodeRef) {
    if let Some(element) = node.as_element() {
        let tag = element.name.local.to_string();
        let mut attrs = element.attributes.borrow_mut();
        for attr in PRESENTATIONAL_ATTRIBUTES {
            attrs.remove(*attr);
        }
        if DEPRECATED_SIZE_ATTRIBUTE_ELEMS.contains(&tag.as_str()) {
            attrs.remove("width");
            attrs.remove("height");
        }
    }

    for child in node.children() {
        clean_styles(&child);
    }
}

fn fix_lazy_images(root: &NodeRef) {
    // TODO: should these be constants
    let image_url = Regex::new(r"(?i)^\s*\S+\.(jpg|jpeg|png|webp)(\?\S*)?\s*$").expect("valid image url regex");
    let image_srcset = Regex::new(r"(?i)\.(jpg|jpeg|png|webp)\S*\s+\d").expect("valid image srcset regex");

    for node in dom::select_nodes(root, "img, picture, figure") {
        let tag = dom::node_name(&node);
        let attrs_snapshot = dom::attrs(&node);
        if attrs_snapshot.get("src").is_some()
            && !attrs_snapshot
                .get("class")
                .is_some_and(|class| class.to_lowercase().contains("lazy"))
        {
            continue;
        }

        for (name, value) in attrs_snapshot {
            if matches!(name.as_str(), "src" | "srcset" | "alt") {
                continue;
            }

            let copy_to = if image_srcset.is_match(&value) {
                Some("srcset")
            } else if image_url.is_match(&value) {
                Some("src")
            } else {
                None
            };

            if let Some(copy_to) = copy_to {
                if tag == "img" || tag == "picture" {
                    dom::set_attr(&node, copy_to, &value);
                }
            }
        }
    }
}

fn remove_share_nodes(root: &NodeRef) {
    for node in dom::select_nodes(root, "*") {
        if SHARE_ELEMENTS.is_match(&dom::class_id_string(&node)) && dom::inner_text(&node).chars().count() < 500 {
            node.detach();
        }
    }
}

fn clean_conditionally(root: &NodeRef, options: &ReadabilityOptions, flags: ExtractFlags) {
    for node in dom::select_nodes(root, "table, ul, ol, div, section, header") {
        if dom::node_id(&node) == dom::node_id(root) || dom::has_ancestor_tag(&node, "code", 3) {
            continue;
        }

        let text = dom::inner_text(&node);
        let content_length = text.chars().count();
        if content_length == 0 {
            node.detach();
            continue;
        }

        if AD_OR_LOADING_WORDS.is_match(text.trim()) {
            node.detach();
            continue;
        }

        let weight = class_weight(&node, flags);
        let density = link_density(&node) + options.link_density_modifier as f64;
        let p_count = dom::select_nodes(&node, "p").len();
        let img_count = dom::select_nodes(&node, "img").len();
        let li_count = dom::select_nodes(&node, "li").len().saturating_sub(100);
        let input_count = dom::select_nodes(&node, "input").len();
        let embed_count = dom::select_nodes(&node, "object, embed, iframe").len();
        let comma_count = COMMA.find_iter(&text).count();

        let should_remove = weight < 0
            || (comma_count < 10
                && ((img_count > 1 && p_count.saturating_mul(2) < img_count)
                    || li_count > p_count
                    || input_count > p_count / 3
                    || (content_length < 25 && img_count == 0 && density > 0.0)
                    || (weight < 25 && density > 0.2)
                    || (weight >= 25 && density > 0.5)
                    || (embed_count == 1 && content_length < 75)
                    || embed_count > 1));

        if should_remove {
            node.detach();
        }
    }
}

fn remove_empty_blocks(root: &NodeRef) {
    for node in dom::select_nodes(root, "p, div, section, header, h1, h2, h3, h4, h5, h6") {
        if dom::node_id(&node) == dom::node_id(root) {
            continue;
        }
        let has_media = !dom::select_nodes(&node, "img, iframe, video, audio, object, embed").is_empty();
        if !has_media && dom::inner_text(&node).trim().is_empty() {
            node.detach();
        }
    }
}

fn fix_relative_urls(root: &NodeRef, base_url: Option<&Url>) {
    let Some(base_url) = base_url else {
        return;
    };

    for node in dom::select_nodes(root, "a[href], area[href]") {
        if let Some(href) = dom::attr(&node, "href") {
            if let Ok(url) = base_url.join(&href) {
                dom::set_attr(&node, "href", url.as_str());
            }
        }
    }

    for node in dom::select_nodes(root, "img[src], video[src], audio[src], source[src], iframe[src]") {
        if let Some(src) = dom::attr(&node, "src") {
            if let Ok(url) = base_url.join(&src) {
                dom::set_attr(&node, "src", url.as_str());
            }
        }
    }
}

fn clean_classes(node: &NodeRef, options: &ReadabilityOptions) {
    if let Some(class) = dom::attr(node, "class") {
        let preserved: Vec<_> = class
            .split_whitespace()
            .filter(|class| {
                DEFAULT_CLASSES_TO_PRESERVE.contains(class)
                    || options.classes_to_preserve.iter().any(|preserve| preserve == class)
            })
            .collect();
        if preserved.is_empty() {
            dom::remove_attr(node, "class");
        } else {
            dom::set_attr(node, "class", &preserved.join(" "));
        }
    }

    for child in node.children() {
        clean_classes(&child, options);
    }
}
