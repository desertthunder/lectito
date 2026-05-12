mod cleanup;
mod config;
mod dom;
mod error;
mod extract;
mod metadata;
mod patterns;
mod readable;
mod scoring;
mod serialize;

pub use config::{Article, ReadabilityOptions, ReadableOptions};
pub use error::Error;
pub use extract::extract;
pub use readable::is_probably_readable;

// TODO: these tests should be localized to relevant mods
#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::patterns::normalize_spaces;

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
    fn extract_returns_article_for_simple_document() {
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
    fn extract_reports_invalid_base_url_and_element_limit() {
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
    fn extract_honors_base_element_for_relative_urls() {
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
    fn extract_repairs_noscript_images_and_scores_br_divs() {
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
    fn readable_matches_upstream_fixture_metadata() {
        let mut checked = 0;
        let mut mismatches = Vec::new();

        for fixture in lectito_fixtures::load_all().unwrap() {
            let metadata: ExpectedMetadata = serde_json::from_value(fixture.expected_metadata).unwrap();
            let actual = is_probably_readable(&fixture.source, &ReadableOptions::default()).unwrap();
            checked += 1;

            if actual != metadata.readerable {
                mismatches.push(format!(
                    "{}: expected {}, got {}",
                    fixture.name, metadata.readerable, actual
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

    #[test]
    fn extract_matches_representative_fixture_metadata() {
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
    fn extract_returns_content_for_representative_fixture_subset() {
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
