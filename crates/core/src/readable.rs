use std::collections::HashSet;

use scraper::{ElementRef, Html};

use super::patterns::{self, MAYBE_CANDIDATE, UNLIKELY_CANDIDATES};
use super::{config::ReadableOptions, error::Error};

pub fn is_probably_readable(html: &str, options: &ReadableOptions) -> Result<bool, Error> {
    let document = Html::parse_document(html);

    let text_selector = patterns::selector("p, pre, article");
    let br_selector = patterns::selector("div > br");
    let list_paragraph_selector = patterns::selector("li p");

    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    let list_paragraphs: HashSet<_> = document
        .select(&list_paragraph_selector)
        .map(|node| node.id())
        .collect();

    for node in document.select(&text_selector) {
        if seen.insert(node.id()) {
            nodes.push(node);
        }
    }

    for br in document.select(&br_selector) {
        if let Some(parent) = br.parent().and_then(ElementRef::wrap) {
            if seen.insert(parent.id()) {
                nodes.push(parent);
            }
        }
    }

    let mut score = 0.0_f32;

    for node in nodes {
        if !is_node_visible(&node) {
            continue;
        }

        let match_string = format!(
            "{} {}",
            node.value().attr("class").unwrap_or_default(),
            node.value().attr("id").unwrap_or_default()
        );
        if UNLIKELY_CANDIDATES.is_match(&match_string) && !MAYBE_CANDIDATE.is_match(&match_string) {
            continue;
        }

        if list_paragraphs.contains(&node.id()) {
            continue;
        }

        let text_length = node.text().collect::<String>().trim().encode_utf16().count();
        if text_length < options.min_content_length {
            continue;
        }

        score += ((text_length - options.min_content_length) as f32).sqrt();
        if score > options.min_score {
            return Ok(true);
        }
    }

    Ok(false)
}

fn is_node_visible(node: &ElementRef<'_>) -> bool {
    let element = node.value();

    if element.has_class("fallback-image", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        return element.attr("hidden").is_none() && !patterns::has_display_none(element.attr("style"));
    }

    !patterns::has_display_none(element.attr("style"))
        && element.attr("hidden").is_none()
        && element.attr("aria-hidden") != Some("true")
}
