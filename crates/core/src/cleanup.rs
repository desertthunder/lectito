use kuchiki::NodeRef;
use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use super::config::{ExtractFlags, ReadabilityOptions};
use super::dom;
use super::patterns::{
    AD_OR_LOADING_WORDS, COMMA, DEFAULT_CLASSES_TO_PRESERVE, DEPRECATED_SIZE_ATTRIBUTE_ELEMS,
    PRESENTATIONAL_ATTRIBUTES, SHARE_ELEMENTS,
};
use super::scoring::{class_weight, link_density};

static LAZY_IMAGE_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\s*\S+\.(jpg|jpeg|png|webp)(\?\S*)?\s*$").expect("valid image url regex"));

static LAZY_IMAGE_SRCSET: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\.(jpg|jpeg|png|webp)\S*\s+\d").expect("valid image srcset regex"));

pub(crate) fn cleanup_article(
    nodes: &[NodeRef], options: &ReadabilityOptions, flags: ExtractFlags, base_url: Option<&Url>,
    article_title: Option<&str>,
) {
    for node in nodes {
        clean_styles(node);
        fix_lazy_images(node);
        dom::remove_matching(node, "script, style, form, fieldset, footer, link, aside");
        clean_embeds(node);
        remove_share_nodes(node);
        clean_headers(node, article_title, flags);
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

fn clean_embeds(root: &NodeRef) {
    for node in dom::select_nodes(root, "object, embed, iframe") {
        let keep = dom::attrs(&node).values().any(|value| allowed_video(value));
        if !keep {
            node.detach();
        }
    }
}

fn allowed_video(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "youtube.com",
        "youtube-nocookie.com",
        "player.vimeo.com",
        "dailymotion.com",
        "player.twitch.tv",
        "archive.org",
        "upload.wikimedia.org",
    ]
    .iter()
    .any(|needle| value.contains(needle))
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

            let copy_to = if LAZY_IMAGE_SRCSET.is_match(&value) {
                Some("srcset")
            } else if LAZY_IMAGE_URL.is_match(&value) {
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

fn clean_headers(root: &NodeRef, article_title: Option<&str>, flags: ExtractFlags) {
    for node in dom::select_nodes(root, "h1, h2") {
        let low_weight = class_weight(&node, flags) < 0;
        let duplicates_title = article_title
            .map(|title| text_similarity(title, &dom::inner_text(&node)) > 0.75)
            .unwrap_or(false);
        if low_weight || duplicates_title {
            node.detach();
        }
    }
}

fn text_similarity(a: &str, b: &str) -> f32 {
    let tokens_a: Vec<_> = a
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect();
    let tokens_b: Vec<_> = b
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect();
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let total_len = tokens_b.join(" ").len().max(1) as f32;
    let unique_b_len = tokens_b
        .iter()
        .filter(|token| !tokens_a.contains(token))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
        .len() as f32;
    1.0 - unique_b_len / total_len
}

fn clean_conditionally(root: &NodeRef, options: &ReadabilityOptions, flags: ExtractFlags) {
    for node in dom::select_nodes(root, "table, ul, ol, div, section, header") {
        if dom::node_id(&node) == dom::node_id(root) || dom::has_ancestor_tag(&node, "code", 3) {
            continue;
        }
        if dom::node_name(&node) == "table" && is_data_table(&node) {
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

fn is_data_table(node: &NodeRef) -> bool {
    if dom::attr(node, "role").as_deref() == Some("presentation") {
        return false;
    }

    if dom::attr(node, "datatable").as_deref() == Some("0") {
        return false;
    }

    if dom::attr(node, "summary").is_some()
        || !dom::select_nodes(node, "caption, col, colgroup, tfoot, thead, th").is_empty()
    {
        return true;
    }

    let rows = dom::select_nodes(node, "tr").len();
    let columns = dom::select_nodes(node, "tr")
        .into_iter()
        .map(|row| dom::select_nodes(&row, "td, th").len())
        .max()
        .unwrap_or(0);
    rows >= 2 && columns >= 2 && rows.saturating_mul(columns) >= 10
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
