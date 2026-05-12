use std::collections::HashMap;

use kuchiki::NodeRef;
use kuchiki::traits::TendrilSink;
use scraper::Html;
use url::Url;

use super::config::{Article, ExtractFlags, ReadabilityOptions};
use super::error::Error;
use super::patterns::{MAYBE_CANDIDATE, UNLIKELY_CANDIDATES};
use super::{cleanup, dom, metadata, patterns, scoring, serialize};
use super::{metadata::Metadata, scoring::Candidate};

pub fn extract(html: &str, base_url: Option<&str>, options: &ReadabilityOptions) -> Result<Option<Article>, Error> {
    let base_url = base_url
        .map(|base_url| Url::parse(base_url).map_err(|_| Error::InvalidBaseUrl(base_url.to_string())))
        .transpose()?;

    let document = Html::parse_document(html);
    enforce_element_limit(&document, options.max_elems_to_parse)?;
    let base_url = effective_base_url(&document, base_url.as_ref());

    let metadata = metadata::extract_metadata(&document, html, options);
    let mut best_attempt: Option<ExtractAttempt> = None;

    let attempts = [
        ExtractFlags::all(),
        ExtractFlags { strip_unlikely: false, weight_classes: true, clean_conditionally: true },
        ExtractFlags { strip_unlikely: false, weight_classes: false, clean_conditionally: true },
        ExtractFlags { strip_unlikely: false, weight_classes: false, clean_conditionally: false },
    ];

    for flags in attempts {
        let dom = kuchiki::parse_html().one(html);
        prep_document(&dom, flags);

        let Some(mut attempt) = grab_article(&dom, options, flags, base_url.as_ref(), metadata.title.as_deref())?
        else {
            continue;
        };

        if attempt.text_len >= options.char_threshold {
            attempt.metadata = metadata;
            return Ok(Some(attempt.into_article()));
        }

        if best_attempt
            .as_ref()
            .map(|best| attempt.text_len > best.text_len)
            .unwrap_or(true)
        {
            best_attempt = Some(attempt);
        }
    }

    let Some(mut attempt) = best_attempt.filter(|attempt| attempt.text_len > 0) else {
        return Ok(None);
    };
    attempt.metadata = metadata;
    Ok(Some(attempt.into_article()))
}

struct ExtractAttempt {
    metadata: Metadata,
    content: String,
    text_content: String,
    text_len: usize,
}

impl ExtractAttempt {
    fn into_article(mut self) -> Article {
        if self.metadata.excerpt.as_deref().unwrap_or_default().trim().is_empty() {
            self.metadata.excerpt = metadata::first_paragraph_excerpt(&self.content);
        }

        Article {
            title: self.metadata.title,
            byline: self.metadata.byline,
            dir: self.metadata.dir,
            lang: self.metadata.lang,
            content: self.content,
            text_content: self.text_content,
            length: self.text_len,
            excerpt: self.metadata.excerpt,
            site_name: self.metadata.site_name,
            published_time: self.metadata.published_time,
        }
    }
}

fn prep_document(document: &NodeRef, flags: ExtractFlags) {
    unwrap_noscript_images(document);
    dom::remove_matching(document, "script, style");
    normalize_markup(document);

    if flags.strip_unlikely {
        let nodes = dom::select_nodes(document, "*");
        for node in nodes {
            if !dom::is_kuchiki_visible(&node) || dom::has_unlikely_role(&node) {
                node.detach();
                continue;
            }

            let tag = dom::node_name(&node);
            if tag == "body" || tag == "a" {
                continue;
            }

            let match_string = dom::class_id_string(&node);
            if UNLIKELY_CANDIDATES.is_match(&match_string)
                && !MAYBE_CANDIDATE.is_match(&match_string)
                && !dom::has_ancestor_tag(&node, "table", 3)
                && !dom::has_ancestor_tag(&node, "code", 3)
            {
                node.detach();
            }
        }
    } else {
        let nodes = dom::select_nodes(document, "*");
        for node in nodes {
            if !dom::is_kuchiki_visible(&node) {
                node.detach();
            }
        }
    }
}

fn normalize_markup(document: &NodeRef) {
    for font in dom::select_nodes(document, "font") {
        let _ = dom::retag_node(&font, "span");
    }

    for div in dom::select_nodes(document, "div") {
        if has_single_element_child(&div, "p") && direct_text_is_empty(&div) {
            dom::replace_with_children(&div);
        }
    }

    for div in dom::select_nodes(document, "div") {
        if !has_child_block_element(&div) && !dom::inner_text(&div).is_empty() {
            let _ = dom::retag_node(&div, "p");
        }
    }
}

fn has_single_element_child(node: &NodeRef, tag: &str) -> bool {
    let mut element_children = node.children().filter(|child| child.as_element().is_some());
    let Some(first) = element_children.next() else {
        return false;
    };
    element_children.next().is_none() && dom::node_name(&first) == tag
}

fn direct_text_is_empty(node: &NodeRef) -> bool {
    node.children()
        .filter_map(|child| child.as_text().map(|text| text.borrow().to_string()))
        .all(|text| text.trim().is_empty())
}

fn has_child_block_element(node: &NodeRef) -> bool {
    !dom::select_nodes(
        node,
        "address, article, aside, blockquote, canvas, dd, div, dl, dt, fieldset, figcaption, figure, footer, form, h1, h2, h3, h4, h5, h6, header, hgroup, hr, li, main, nav, noscript, ol, output, p, pre, section, table, tfoot, ul, video",
    )
    .is_empty()
}

fn effective_base_url(document: &Html, base_url: Option<&Url>) -> Option<Url> {
    let base_url = base_url.cloned()?;
    let selector = patterns::selector("base[href]");
    document
        .select(&selector)
        .next()
        .and_then(|base| base.value().attr("href"))
        .and_then(|href| base_url.join(href).ok())
        .or(Some(base_url))
}

fn unwrap_noscript_images(document: &NodeRef) {
    for noscript in dom::select_nodes(document, "noscript") {
        let content = unescape_basic_html(&noscript.text_contents());
        let lower_content = content.to_ascii_lowercase();
        if !lower_content.contains("<img") && !lower_content.contains("<picture") {
            continue;
        }

        let fragment = kuchiki::parse_html().one(format!("<html><body>{content}</body></html>"));
        let Some(body) = dom::select_nodes(&fragment, "body").into_iter().next() else {
            continue;
        };
        let children: Vec<_> = body.children().collect();
        if children.is_empty() {
            continue;
        }

        for child in children {
            noscript.insert_before(child);
        }
        noscript.detach();
    }
}

fn unescape_basic_html(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&")
}

fn grab_article(
    document: &NodeRef, options: &ReadabilityOptions, flags: ExtractFlags, base_url: Option<&Url>,
    article_title: Option<&str>,
) -> Result<Option<ExtractAttempt>, Error> {
    let mut candidates = scoring::score_candidates(document, flags);
    if candidates.is_empty() {
        let body = dom::select_nodes(document, "body").into_iter().next();
        if let Some(body) = body {
            candidates.push(Candidate { node: body, score: 1.0 });
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    for candidate in &mut candidates {
        candidate.score *= 1.0 - scoring::link_density(&candidate.node);
    }

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.truncate(options.nb_top_candidates.max(1));

    let top_candidate = candidates[0].node.clone();
    let top_score = candidates[0].score;
    let top_id = dom::node_id(&top_candidate);
    let score_by_id: HashMap<usize, f64> = candidates
        .iter()
        .map(|candidate| (dom::node_id(&candidate.node), candidate.score))
        .collect();

    let parent = top_candidate.parent().unwrap_or_else(|| document.clone());
    let sibling_threshold = 10.0_f64.max(top_score * 0.2);
    let top_class = dom::attr(&top_candidate, "class").unwrap_or_default();

    let mut included = Vec::new();
    for sibling in parent.children().filter(|node| node.as_element().is_some()) {
        let mut append = dom::node_id(&sibling) == top_id;

        if !append {
            let mut content_bonus = 0.0;
            if !top_class.is_empty() && dom::attr(&sibling, "class").as_deref() == Some(top_class.as_str()) {
                content_bonus += top_score * 0.2;
            }

            if score_by_id.get(&dom::node_id(&sibling)).copied().unwrap_or(0.0) + content_bonus >= sibling_threshold {
                append = true;
            } else if dom::node_name(&sibling) == "p" {
                let density = scoring::link_density(&sibling);
                let text = dom::inner_text(&sibling);
                let len = text.chars().count();
                if len > 80 && density < 0.25 {
                    append = true;
                } else if len < 80 && len > 0 && density == 0.0 && text.contains(". ") {
                    append = true;
                }
            }
        }

        if append {
            included.push(sibling);
        }
    }

    if included.is_empty() {
        included.push(top_candidate);
    }

    cleanup::cleanup_article(&included, options, flags, base_url, article_title);

    let mut content = String::from(r#"<div id="readability-page-1" class="page">"#);
    for node in &included {
        if dom::node_name(node) == "body" {
            content.push_str(&serialize::serialize_children(node)?);
        } else {
            content.push_str(&serialize::serialize_node(node)?);
        }
    }
    content.push_str("</div>");
    let text_content = serialize::text_content(&included);
    let text_len = text_content.encode_utf16().count();

    Ok(Some(ExtractAttempt {
        metadata: Metadata::default(),
        content,
        text_content,
        text_len,
    }))
}

fn enforce_element_limit(document: &Html, limit: Option<usize>) -> Result<(), Error> {
    let Some(limit) = limit else {
        return Ok(());
    };

    let selector = patterns::selector("*");
    let actual = document.select(&selector).count();
    if actual > limit {
        return Err(Error::MaxElemsExceeded { actual, limit });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::normalize_spaces;

    #[test]
    fn returns_article_for_simple_document() {
        let article = extract(
            "<html><head><title>Example Article</title></head><body><article><p>This is a long enough paragraph, with punctuation, to become a readable article body for the MVP extractor.</p></article></body></html>",
            None,
            &ReadabilityOptions { char_threshold: 0, ..Default::default() },
        )
        .unwrap()
        .unwrap();

        assert_eq!(article.title.as_deref(), Some("Example Article"));
        assert!(article.content.contains("readability-page-1"));
        assert!(article.text_content.contains("long enough paragraph"));
        assert!(article.length > 25);
    }

    #[test]
    fn reports_invalid_base_url_and_element_limit() {
        let invalid_url = extract(
            "<html><body><p>text</p></body></html>",
            Some("not a url"),
            &Default::default(),
        );
        assert!(matches!(invalid_url, Err(Error::InvalidBaseUrl(_))));

        let too_many_elements = extract(
            "<html><body><main><p>text</p></main></body></html>",
            None,
            &ReadabilityOptions { max_elems_to_parse: Some(2), ..Default::default() },
        );
        assert!(matches!(too_many_elements, Err(Error::MaxElemsExceeded { .. })));
    }

    #[test]
    fn honors_base_element_for_relative_urls() {
        let fixture = lectito_fixtures::load_fixture("base-url-base-element").unwrap();
        let article = extract(
            &fixture.source,
            Some("http://fakehost/test/page.html"),
            &ReadabilityOptions { char_threshold: 0, ..Default::default() },
        )
        .unwrap()
        .unwrap();

        assert!(article.content.contains(r#"href="http://fakehost/foo/bar/baz.html""#));
        assert!(article.content.contains(r#"src="http://fakehost/foo/bar/baz.png""#));
        assert!(
            !article
                .content
                .contains(r#"href="http://fakehost/test/foo/bar/baz.html""#)
        );
    }

    #[test]
    fn repairs_noscript_images_and_scores_br_divs() {
        let noscript_article = extract(
            r#"<html><body><article><p>Enough text, with punctuation, to choose the article body for this regression.</p><noscript>&lt;img src="/image.jpg" alt="fallback"&gt;</noscript></article></body></html>"#,
            Some("https://example.com/story"),
            &ReadabilityOptions { char_threshold: 0, ..Default::default() },
        )
        .unwrap()
        .unwrap();
        assert!(
            noscript_article
                .content
                .contains(r#"src="https://example.com/image.jpg""#)
        );

        let br_article = extract(
            "<html><body><div>First long line with enough words to score well.<br><br>Second long line, also with enough words and punctuation to survive extraction.</div></body></html>",
            None,
            &ReadabilityOptions { char_threshold: 0, ..Default::default() },
        )
        .unwrap()
        .unwrap();
        assert!(br_article.text_content.contains("Second long line"));
    }

    #[test]
    fn matches_representative_fixture_metadata() {
        for name in [
            "wikipedia",
            "base-url-base-element",
            "article-author-tag",
            "parsely-metadata",
        ] {
            let fixture = lectito_fixtures::load_fixture(name).unwrap();
            let article = extract(
                &fixture.source,
                Some("http://fakehost/test/page.html"),
                &ReadabilityOptions { char_threshold: 0, ..Default::default() },
            )
            .unwrap()
            .unwrap();

            let expected = fixture.expected_metadata;
            if let Some(title) = expected.get("title").and_then(serde_json::Value::as_str) {
                assert_eq!(article.title.as_deref(), Some(title), "{name} title");
            }
            if let Some(byline) = expected.get("byline").and_then(serde_json::Value::as_str) {
                assert_eq!(article.byline.as_deref(), Some(byline), "{name} byline");
            }
            if let Some(excerpt) = expected.get("excerpt").and_then(serde_json::Value::as_str) {
                assert_eq!(
                    article.excerpt.as_deref().map(normalize_spaces),
                    Some(normalize_spaces(excerpt)),
                    "{name} excerpt"
                );
            }
            assert!(article.length > 0, "{name} should have text");
            assert!(
                article.content.contains("readability-page-1"),
                "{name} should be wrapped"
            );
        }
    }

    #[test]
    fn returns_content_for_representative_fixture_subset() {
        let names = [
            "wikipedia",
            "dropbox-blog",
            "cnet",
            "base-url-base-element",
            "keep-images",
            "replace-brs",
            "article-author-tag",
            "parsely-metadata",
        ];

        for name in names {
            let fixture = lectito_fixtures::load_fixture(name).unwrap();
            let article = extract(
                &fixture.source,
                Some("http://fakehost/test/page.html"),
                &ReadabilityOptions { char_threshold: 0, ..Default::default() },
            )
            .unwrap()
            .unwrap();

            assert!(article.length > 100, "{name} should have meaningful text");
            assert!(
                article.content.contains("readability-page-1"),
                "{name} should be wrapped"
            );
            assert!(
                !fixture.expected_content.trim().is_empty(),
                "{name} fixture should include expected content"
            );
        }
    }
}
