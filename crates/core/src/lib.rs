use std::collections::HashSet;

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use thiserror::Error;
use url::Url;

static UNLIKELY_CANDIDATES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote",
    )
    .expect("valid unlikely-candidates regex")
});

static MAYBE_CANDIDATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)and|article|body|column|content|main|mathjax|shadow").expect("valid ok-maybe regex"));

#[derive(Clone, Debug)]
pub struct ReadabilityOptions {
    pub max_elems_to_parse: Option<usize>,
    pub nb_top_candidates: usize,
    pub char_threshold: usize,
    pub classes_to_preserve: Vec<String>,
    pub keep_classes: bool,
    pub disable_json_ld: bool,
    pub link_density_modifier: f32,
}

impl Default for ReadabilityOptions {
    fn default() -> Self {
        Self {
            max_elems_to_parse: None,
            nb_top_candidates: 5,
            char_threshold: 500,
            classes_to_preserve: Vec::new(),
            keep_classes: false,
            disable_json_ld: false,
            link_density_modifier: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReadableOptions {
    pub min_content_length: usize,
    pub min_score: f32,
}

impl Default for ReadableOptions {
    fn default() -> Self {
        Self { min_content_length: 140, min_score: 20.0 }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Article {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub dir: Option<String>,
    pub lang: Option<String>,
    pub content: String,
    pub text_content: String,
    pub length: usize,
    pub excerpt: Option<String>,
    pub site_name: Option<String>,
    pub published_time: Option<String>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to parse HTML")]
    HtmlParse,
    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(String),
    #[error("document has {actual} elements, exceeding max_elems_to_parse={limit}")]
    MaxElemsExceeded { actual: usize, limit: usize },
    #[error("failed to serialize article HTML")]
    Serialization,
}

pub fn extract(html: &str, base_url: Option<&str>, options: &ReadabilityOptions) -> Result<Option<Article>, Error> {
    if let Some(base_url) = base_url {
        Url::parse(base_url).map_err(|_| Error::InvalidBaseUrl(base_url.to_string()))?;
    }

    let document = Html::parse_document(html);
    enforce_element_limit(&document, options.max_elems_to_parse)?;

    Ok(None)
}

pub fn is_probably_readable(html: &str, options: &ReadableOptions) -> Result<bool, Error> {
    let document = Html::parse_document(html);

    let text_selector = selector("p, pre, article");
    let br_selector = selector("div > br");
    let list_paragraph_selector = selector("li p");

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

fn enforce_element_limit(document: &Html, limit: Option<usize>) -> Result<(), Error> {
    let Some(limit) = limit else {
        return Ok(());
    };

    let selector = selector("*");
    let actual = document.select(&selector).count();
    if actual > limit {
        return Err(Error::MaxElemsExceeded { actual, limit });
    }

    Ok(())
}

fn is_node_visible(node: &ElementRef<'_>) -> bool {
    let element = node.value();

    if element.has_class("fallback-image", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        return element.attr("hidden").is_none() && !has_display_none(element.attr("style"));
    }

    !has_display_none(element.attr("style"))
        && element.attr("hidden").is_none()
        && element.attr("aria-hidden") != Some("true")
}

fn has_display_none(style: Option<&str>) -> bool {
    style
        .unwrap_or_default()
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .any(|(property, value)| {
            property.trim().eq_ignore_ascii_case("display") && value.trim().eq_ignore_ascii_case("none")
        })
}

fn selector(pattern: &str) -> Selector {
    Selector::parse(pattern).expect("internal selector should parse")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct ExpectedMetadata {
        readerable: bool,
    }

    #[test]
    fn readable_default_thresholds_match_upstream_shape() {
        let very_small_doc = r#"<html><p id="main">hello there</p></html>"#;
        let small_doc = format!(r#"<html><p id="main">{}</p></html>"#, "hello there ".repeat(11));
        let large_doc = format!(r#"<html><p id="main">{}</p></html>"#, "hello there ".repeat(12));
        let very_large_doc = format!(r#"<html><p id="main">{}</p></html>"#, "hello there ".repeat(50));

        let options = ReadableOptions::default();

        assert!(!is_probably_readable(very_small_doc, &options).unwrap());
        assert!(!is_probably_readable(&small_doc, &options).unwrap());
        assert!(!is_probably_readable(&large_doc, &options).unwrap());
        assert!(is_probably_readable(&very_large_doc, &options).unwrap());
    }

    #[test]
    fn readable_options_control_content_length_and_score() {
        let small_doc = format!(r#"<html><p id="main">{}</p></html>"#, "hello there ".repeat(11));
        let large_doc = format!(r#"<html><p id="main">{}</p></html>"#, "hello there ".repeat(12));

        assert!(
            is_probably_readable(&small_doc, &ReadableOptions { min_content_length: 120, min_score: 0.0 },).unwrap()
        );
        assert!(
            !is_probably_readable(&large_doc, &ReadableOptions { min_content_length: 200, min_score: 0.0 },).unwrap()
        );
        assert!(
            is_probably_readable(&large_doc, &ReadableOptions { min_content_length: 0, min_score: 11.5 },).unwrap()
        );
    }

    #[test]
    fn readable_skips_hidden_unlikely_and_list_paragraphs() {
        let options = ReadableOptions { min_content_length: 0, min_score: 0.0 };

        assert!(
            !is_probably_readable(r#"<html><p hidden>this paragraph is long enough</p></html>"#, &options,).unwrap()
        );
        assert!(
            !is_probably_readable(
                r#"<html><p style="display: none">this paragraph is long enough</p></html>"#,
                &options,
            )
            .unwrap()
        );
        assert!(
            !is_probably_readable(
                r#"<html><p class="comment">this paragraph is long enough</p></html>"#,
                &options,
            )
            .unwrap()
        );
        assert!(
            !is_probably_readable(
                r#"<html><li><p>this paragraph is long enough</p></li></html>"#,
                &options,
            )
            .unwrap()
        );
    }

    #[test]
    fn extract_exposes_milestone_one_api_stub() {
        let article = extract("<html><body><p>hello</p></body></html>", None, &Default::default()).unwrap();
        assert_eq!(article, None);
    }

    #[test]
    fn readable_matches_upstream_fixture_metadata() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.sandbox/readability/test/test-pages");
        if !root.exists() {
            return;
        }

        let mut checked = 0;
        let mut mismatches = Vec::new();

        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_dir() {
                continue;
            }

            let dir = entry.path();
            let source_path = dir.join("source.html");
            let metadata_path = dir.join("expected-metadata.json");
            if !source_path.exists() || !metadata_path.exists() {
                continue;
            }

            let source = fs::read_to_string(&source_path).unwrap();
            let metadata: ExpectedMetadata =
                serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
            let actual = is_probably_readable(&source, &ReadableOptions::default()).unwrap();
            checked += 1;

            if actual != metadata.readerable {
                mismatches.push(format!(
                    "{}: expected {}, got {}",
                    dir.file_name().unwrap().to_string_lossy(),
                    metadata.readerable,
                    actual
                ));
            }
        }

        assert!(checked > 0);
        assert!(
            mismatches.is_empty(),
            "readable fixture mismatches:\n{}",
            mismatches.join("\n")
        );
    }
}
