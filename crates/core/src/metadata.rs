use std::collections::HashMap;

use scraper::Html;
use serde_json::Value;
use url::Url;

use super::config::ReadabilityOptions;
use super::patterns::{self, JSON_LD_ARTICLE_TYPES};

#[derive(Clone, Debug, Default)]
pub(crate) struct Metadata {
    pub(crate) title: Option<String>,
    pub(crate) byline: Option<String>,
    pub(crate) excerpt: Option<String>,
    pub(crate) site_name: Option<String>,
    pub(crate) published_time: Option<String>,
    pub(crate) lang: Option<String>,
    pub(crate) dir: Option<String>,
}

pub(crate) fn extract_metadata(document: &Html, html: &str, options: &ReadabilityOptions) -> Metadata {
    let mut metadata = if options.disable_json_ld { Metadata::default() } else { extract_json_ld(html) };
    let mut values = HashMap::<String, String>::new();
    let meta_selector = patterns::selector("meta");

    for element in document.select(&meta_selector) {
        let content = element
            .value()
            .attr("content")
            .map(str::trim)
            .filter(|content| !content.is_empty());
        let Some(content) = content else {
            continue;
        };

        if let Some(property) = element.value().attr("property") {
            let name = property.to_lowercase().replace(char::is_whitespace, "");
            if meta_property_key(&name) {
                values.insert(name, content.to_string());
            }
        }

        if let Some(name) = element.value().attr("name") {
            let normalized = name.to_lowercase().replace(char::is_whitespace, "").replace('.', ":");
            if meta_name_key(&normalized) {
                values.insert(normalized, content.to_string());
            }
        }
    }

    metadata.title = metadata
        .title
        .or_else(|| {
            first_value(
                &values,
                &[
                    "dc:title",
                    "dcterm:title",
                    "og:title",
                    "weibo:article:title",
                    "weibo:webpage:title",
                    "title",
                    "twitter:title",
                    "parsely-title",
                ],
            )
        })
        .or_else(|| article_title(document));
    metadata.byline = metadata.byline.or_else(|| {
        first_value(
            &values,
            &[
                "dc:creator",
                "dcterm:creator",
                "author",
                "parsely-author",
                "article:author",
            ],
        )
        .filter(|value| !is_url(value))
    });
    metadata.excerpt = metadata.excerpt.or_else(|| {
        first_value(
            &values,
            &[
                "dc:description",
                "dcterm:description",
                "og:description",
                "weibo:article:description",
                "weibo:webpage:description",
                "description",
                "twitter:description",
            ],
        )
    });
    metadata.site_name = metadata.site_name.or_else(|| first_value(&values, &["og:site_name"]));
    metadata.published_time = metadata
        .published_time
        .or_else(|| first_value(&values, &["article:published_time", "parsely-pub-date"]));

    let html_selector = patterns::selector("html");
    if let Some(html) = document.select(&html_selector).next() {
        metadata.lang = html.value().attr("lang").map(str::to_string);
        metadata.dir = html.value().attr("dir").map(str::to_string);
    }

    metadata
}

fn extract_json_ld(html: &str) -> Metadata {
    let document = Html::parse_document(html);
    let script_selector = patterns::selector(r#"script[type="application/ld+json"]"#);

    for script in document.select(&script_selector) {
        let content = script.text().collect::<String>();
        let content = content.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>");
        let Ok(value) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if let Some(article) = find_json_ld_article(&value) {
            return metadata_from_json_ld(article);
        }
    }

    Metadata::default()
}

fn find_json_ld_article(value: &Value) -> Option<&Value> {
    match value {
        Value::Array(items) => items.iter().find_map(find_json_ld_article),
        Value::Object(map) => {
            if let Some(graph) = map.get("@graph").and_then(Value::as_array) {
                if let Some(article) = graph.iter().find_map(find_json_ld_article) {
                    return Some(article);
                }
            }

            if map.get("@type").is_some_and(json_ld_type_is_article) { Some(value) } else { None }
        }
        _ => None,
    }
}

fn json_ld_type_is_article(value: &Value) -> bool {
    match value {
        Value::String(kind) => JSON_LD_ARTICLE_TYPES.is_match(kind.trim_start_matches("https://schema.org/")),
        Value::Array(kinds) => kinds.iter().any(json_ld_type_is_article),
        _ => false,
    }
}

fn metadata_from_json_ld(value: &Value) -> Metadata {
    Metadata {
        title: string_field(value, "headline").or_else(|| string_field(value, "name")),
        byline: byline_from_json_ld(value.get("author")),
        excerpt: string_field(value, "description"),
        site_name: value
            .get("publisher")
            .and_then(|publisher| string_field(publisher, "name")),
        published_time: string_field(value, "datePublished"),
        lang: None,
        dir: None,
    }
}

fn byline_from_json_ld(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(author) => Some(author.trim().to_string()),
        Value::Object(_) => string_field(value?, "name"),
        Value::Array(authors) => {
            let names: Vec<_> = authors
                .iter()
                .filter_map(|author| string_field(author, "name"))
                .collect();
            (!names.is_empty()).then(|| names.join(", "))
        }
        _ => None,
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn meta_property_key(name: &str) -> bool {
    let Some((prefix, key)) = name.split_once(':') else {
        return false;
    };
    matches!(prefix, "article" | "dc" | "dcterm" | "og" | "twitter")
        && matches!(
            key,
            "author" | "creator" | "description" | "published_time" | "title" | "site_name"
        )
}

fn meta_name_key(name: &str) -> bool {
    let name = name
        .strip_prefix("dc:")
        .or_else(|| name.strip_prefix("dcterm:"))
        .or_else(|| name.strip_prefix("og:"))
        .or_else(|| name.strip_prefix("twitter:"))
        .or_else(|| name.strip_prefix("parsely-"))
        .or_else(|| name.strip_prefix("weibo:article:"))
        .or_else(|| name.strip_prefix("weibo:webpage:"))
        .unwrap_or(name);
    matches!(
        name,
        "author" | "creator" | "pub-date" | "description" | "title" | "site_name"
    )
}

fn article_title(document: &Html) -> Option<String> {
    let title_selector = patterns::selector("title");
    let title = document.select(&title_selector).next()?.text().collect::<String>();
    let original = patterns::normalize_spaces(title.trim());
    if original.is_empty() {
        return None;
    }

    for separator in [" | ", " - ", " — ", " – ", " :: ", " / "] {
        if let Some((first, _)) = original.split_once(separator) {
            let first = patterns::normalize_spaces(first.trim());
            if first.split_whitespace().count() > 4 {
                return Some(first);
            }
        }
    }

    Some(original)
}

pub(crate) fn first_paragraph_excerpt(content: &str) -> Option<String> {
    let document = Html::parse_fragment(content);
    first_excerpt_for_selector(&document, "p").or_else(|| first_excerpt_for_selector(&document, "div"))
}

fn first_excerpt_for_selector(document: &Html, selector_pattern: &str) -> Option<String> {
    let selector = patterns::selector(selector_pattern);
    document
        .select(&selector)
        .filter(|element| element.value().attr("id") != Some("readability-page-1"))
        .map(|element| patterns::normalize_spaces(element.text().collect::<String>().trim()))
        .find(|excerpt| {
            let len = excerpt.chars().count();
            (30..=1000).contains(&len)
        })
        .filter(|excerpt| !excerpt.is_empty())
}

fn first_value(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| values.get(*key).cloned())
        .filter(|value| !value.trim().is_empty())
}

fn is_url(value: &str) -> bool {
    Url::parse(value).is_ok()
}
