use std::collections::HashMap;

use scraper::Html;
use serde_json::Value;
use url::Url;

use super::config::ReadabilityOptions;
use super::patterns::{self, JSON_LD_ARTICLE_TYPES};

const TITLE_SEPARATORS: &[&str] = &[" | ", " - ", " – ", " — ", " \\ ", " / ", " > ", " » "];

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
            for name in property.split_whitespace() {
                let name = name.to_lowercase();
                if meta_property_key(&name) {
                    values.insert(name, decode_html_entities(content));
                }
            }
        }

        if let Some(name) = element.value().attr("name") {
            let normalized = name.to_lowercase().replace(char::is_whitespace, "").replace('.', ":");
            if meta_name_key(&normalized) {
                values.insert(normalized, decode_html_entities(content));
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
    metadata.byline = metadata
        .byline
        .or_else(|| {
            first_value(
                &values,
                &[
                    "dc:creator",
                    "dcterm:creator",
                    "author",
                    "parsely-author",
                    "article:author",
                    "og:article:author",
                ],
            )
            .filter(|value| !is_url(value))
        })
        .or_else(|| byline_from_document(document));
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
    let body_selector = patterns::selector("body");
    let content_dir_selector = patterns::selector(r#"main[dir], [role="main"][dir]"#);
    if let Some(html) = document.select(&html_selector).next() {
        metadata.lang = html.value().attr("lang").map(str::to_string);
        metadata.dir = document
            .select(&body_selector)
            .next()
            .and_then(|body| body.value().attr("dir"))
            .or_else(|| {
                document
                    .select(&content_dir_selector)
                    .next()
                    .and_then(|element| element.value().attr("dir"))
            })
            .or_else(|| html.value().attr("dir"))
            .map(str::to_string);
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
        title: string_field(value, "name").or_else(|| string_field(value, "headline")),
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
        .map(decode_html_entities)
}

fn meta_property_key(name: &str) -> bool {
    let Some((prefix, key)) = name.split_once(':') else {
        return false;
    };
    matches!(prefix, "article" | "dc" | "dcterm" | "og" | "twitter")
        && matches!(
            key,
            "article:author" | "author" | "creator" | "description" | "published_time" | "title" | "site_name"
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
    let title = document
        .select(&title_selector)
        .next()
        .map(|title| title.text().collect::<String>())
        .unwrap_or_default();
    let original = patterns::normalize_spaces(title.trim());
    if original.is_empty() {
        return Some(String::new());
    }

    let mut had_hierarchical_separator = false;
    let mut title = original.clone();

    if let Some((separator, index)) = last_separator(&original) {
        had_hierarchical_separator = matches!(separator, " \\ " | " / " | " > " | " » ");
        title = patterns::normalize_spaces(original[..index].trim());

        if word_count(&title) < 3 {
            title = patterns::normalize_spaces(original[index + separator.len()..].trim());
        }
    } else if original.contains(": ") && !heading_matches(document, &original) {
        title = patterns::normalize_spaces(original[original.rfind(':').unwrap_or(0) + 1..].trim());
        if word_count(&title) < 3 {
            title = patterns::normalize_spaces(original[original.find(':').unwrap_or(0) + 1..].trim());
        } else if word_count(&original[..original.find(':').unwrap_or(0)]) > 5 {
            title = original.clone();
        }
    } else if original.chars().count() > 150 || original.chars().count() < 15 {
        let h1_selector = patterns::selector("h1");
        let h1s: Vec<_> = document.select(&h1_selector).collect();
        if h1s.len() == 1 {
            title = patterns::normalize_spaces(h1s[0].text().collect::<String>().trim());
        }
    }

    let title_word_count = word_count(&title);
    let original_without_separators = TITLE_SEPARATORS
        .iter()
        .fold(original.clone(), |title, separator| title.replace(separator, " "));
    if title_word_count <= 4
        && (!had_hierarchical_separator || title_word_count != word_count(&original_without_separators) - 1)
    {
        title = original;
    }

    Some(title)
}

fn last_separator(title: &str) -> Option<(&'static str, usize)> {
    TITLE_SEPARATORS
        .iter()
        .filter_map(|separator| title.rfind(separator).map(|index| (*separator, index)))
        .max_by_key(|(_, index)| *index)
}

fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

fn heading_matches(document: &Html, title: &str) -> bool {
    let selector = patterns::selector("h1, h2");
    document
        .select(&selector)
        .any(|heading| patterns::normalize_spaces(heading.text().collect::<String>().trim()) == title)
}

fn byline_from_document(document: &Html) -> Option<String> {
    for selector in [
        r#"[itemprop*="author"] [itemprop*="name"], [rel="author"] [itemprop*="name"], a[rel="author"], [class*="author"] a[href*="/author/"], [class*="byline"] a[href*="/author/"]"#,
        r#"[rel="author"], [itemprop*="author"]"#,
        r#".byline, .article-author, .p-author, [class*="byline"], [id*="byline"], [id*="author"], .author, [class*="author"]"#,
    ] {
        if let Some(byline) = byline_from_selector(document, selector) {
            return Some(byline);
        }
    }
    None
}

fn byline_from_selector(document: &Html, selector: &str) -> Option<String> {
    let selector = patterns::selector(selector);
    for element in document.select(&selector) {
        let text = if element
            .value()
            .attr("itemprop")
            .is_some_and(|itemprop| itemprop.contains("author"))
        {
            let name_selector = patterns::selector(r#"[itemprop*="name"]"#);
            element
                .select(&name_selector)
                .next()
                .map(|name| name.text().collect::<String>())
                .unwrap_or_else(|| element.text().collect::<String>())
        } else {
            element.text().collect::<String>()
        };
        let byline = clean_byline(&text);
        if !byline.is_empty() && byline.chars().count() < 100 {
            return Some(byline);
        }
    }
    None
}

fn clean_byline(value: &str) -> String {
    patterns::normalize_spaces(value.trim())
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
        .map(|element| decode_html_entities(&patterns::normalize_spaces(element.text().collect::<String>().trim())))
        .find(|excerpt| {
            let len = excerpt.chars().count();
            (15..=1000).contains(&len)
        })
        .filter(|excerpt| !excerpt.is_empty())
}

fn first_value(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| values.get(*key).cloned())
        .filter(|value| !value.trim().is_empty())
}

fn decode_html_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let after_amp = &rest[start + 1..];
        let Some(end) = after_amp.find(';') else {
            decoded.push_str(&rest[start..]);
            return decoded;
        };

        let entity = &after_amp[..end];
        let replacement = decode_entity(entity);
        if let Some(replacement) = replacement {
            decoded.push_str(&replacement);
        } else {
            decoded.push('&');
            decoded.push_str(entity);
            decoded.push(';');
        }
        rest = &after_amp[end + 1..];
    }

    decoded.push_str(rest);
    decoded
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => Some("&".to_string()),
        "apos" => Some("'".to_string()),
        "gt" => Some(">".to_string()),
        "lt" => Some("<".to_string()),
        "nbsp" => Some(" ".to_string()),
        "quot" => Some("\"".to_string()),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => decode_numeric_entity(&entity[2..], 16),
        _ if entity.starts_with('#') => decode_numeric_entity(&entity[1..], 10),
        _ => None,
    }
}

fn decode_numeric_entity(value: &str, radix: u32) -> Option<String> {
    let codepoint = u32::from_str_radix(value, radix).ok()?;
    let character = char::from_u32(codepoint)
        .filter(|character| *character != '\0')
        .unwrap_or('\u{fffd}');
    Some(character.to_string())
}

fn is_url(value: &str) -> bool {
    Url::parse(value).is_ok()
}
