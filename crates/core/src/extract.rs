use std::collections::HashMap;

use kuchiki::NodeRef;
use kuchiki::traits::TendrilSink;
use scraper::Html;
use url::Url;

use super::config::{Article, ExtractFlags, ReadabilityOptions};
use super::diagnostics::{
    AttemptDiagnostic, CandidateDiagnostic, CandidateSelection, CleanupDiagnostic, ContentSelectorDiagnostic,
    ExtractionDiagnostics, ExtractionOutcome, ExtractionReport, FlagDiagnostic, NodeDiagnostic,
};
use super::error::Error;
use super::patterns::{MAYBE_CANDIDATE, UNLIKELY_CANDIDATES};
use super::{cleanup, dom, metadata, patterns, scoring, serialize};
use super::{metadata::Metadata, scoring::Candidate};

pub fn extract(html: &str, base_url: Option<&str>, options: &ReadabilityOptions) -> Result<Option<Article>, Error> {
    Ok(extract_with_diagnostics(html, base_url, options)?.article)
}

pub fn extract_with_diagnostics(
    html: &str, base_url: Option<&str>, options: &ReadabilityOptions,
) -> Result<ExtractionReport, Error> {
    let base_url = base_url
        .map(|base_url| Url::parse(base_url).map_err(|_| Error::InvalidBaseUrl(base_url.to_string())))
        .transpose()?;

    let document = Html::parse_document(html);
    enforce_element_limit(&document, options.max_elems_to_parse)?;
    let base_url = effective_base_url(&document, base_url.as_ref());

    let metadata = metadata::extract_metadata(&document, html, options);
    let mut best_attempt: Option<ExtractAttempt> = None;
    let mut diagnostics = ExtractionDiagnostics::default();

    let attempts = [
        ExtractFlags::all(),
        ExtractFlags { strip_unlikely: false, weight_classes: true, clean_conditionally: true },
        ExtractFlags { strip_unlikely: false, weight_classes: false, clean_conditionally: true },
        ExtractFlags { strip_unlikely: false, weight_classes: false, clean_conditionally: false },
    ];

    for (index, flags) in attempts.into_iter().enumerate() {
        let dom = kuchiki::parse_html().one(html);
        prep_document(&dom, flags);

        let Some((mut attempt, attempt_diagnostic)) = grab_article(
            &dom,
            options,
            flags,
            index,
            base_url.as_ref(),
            metadata.title.as_deref(),
        )?
        else {
            diagnostics.attempts.push(AttemptDiagnostic {
                index,
                flags: flags.into(),
                candidate_count: 0,
                candidates: Vec::new(),
                entry_points: Vec::new(),
                selected_root: None,
                cleanup: None,
                text_len: 0,
                accepted: false,
            });
            continue;
        };

        if diagnostics.content_selector.is_none() {
            diagnostics.content_selector = attempt_diagnostic.content_selector.clone();
        }
        diagnostics.attempts.push(attempt_diagnostic.attempt);
        let diagnostic_index = diagnostics.attempts.len() - 1;

        if attempt.text_len >= options.char_threshold {
            attempt.metadata = metadata;
            diagnostics.selected_attempt = Some(diagnostic_index);
            diagnostics.outcome = ExtractionOutcome::Accepted;
            return Ok(ExtractionReport { article: Some(attempt.into_article()), diagnostics });
        }

        if best_attempt
            .as_ref()
            .map(|best| attempt.text_len > best.text_len)
            .unwrap_or(true)
        {
            diagnostics.selected_attempt = Some(diagnostic_index);
            best_attempt = Some(attempt);
        }
    }

    let Some(mut attempt) = best_attempt.filter(|attempt| attempt.text_len > 0) else {
        diagnostics.outcome = ExtractionOutcome::NoContent;
        return Ok(ExtractionReport { article: None, diagnostics });
    };
    attempt.metadata = metadata;
    diagnostics.outcome = ExtractionOutcome::BestAttempt;
    Ok(ExtractionReport { article: Some(attempt.into_article()), diagnostics })
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

// TODO: should this be in this mod?
fn unescape_basic_html(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&")
}

fn grab_article(
    doc: &NodeRef, opts: &ReadabilityOptions, flags: ExtractFlags, index: usize, base_url: Option<&Url>,
    title: Option<&str>,
) -> Result<Option<(ExtractAttempt, GrabDiagnostics)>, Error> {
    if let Some(selector) = opts.content_selector.as_deref() {
        if let Some(root) = dom::select_nodes(doc, selector).into_iter().next() {
            let selector_diagnostic = ContentSelectorDiagnostic {
                selector: selector.to_string(),
                matched: true,
                selected: Some(node_diagnostic(&root)),
            };
            let (attempt, cleanup) = serialize_roots(vec![root], opts, flags, base_url, title)?;
            let attempt_diagnostic = AttemptDiagnostic {
                index,
                flags: flags.into(),
                candidate_count: 0,
                candidates: Vec::new(),
                entry_points: Vec::new(),
                selected_root: selector_diagnostic.selected.clone(),
                cleanup: Some(cleanup),
                text_len: attempt.text_len,
                accepted: attempt.text_len >= opts.char_threshold,
            };
            return Ok(Some((
                attempt,
                GrabDiagnostics { attempt: attempt_diagnostic, content_selector: Some(selector_diagnostic) },
            )));
        }
    }

    let entry_points = entry_point_candidates(doc);
    let mut candidates = scoring::score_candidates(doc, flags);
    if candidates.is_empty() {
        let body = dom::select_nodes(doc, "body").into_iter().next();
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

    let selected_entry_id = selected_entry_point(&entry_points).map(|entry_point| {
        let id = dom::node_id(&entry_point.node);
        if let Some(candidate) = candidates
            .iter_mut()
            .find(|candidate| dom::node_id(&candidate.node) == id)
        {
            candidate.score = candidate.score.max(entry_point.score);
        } else {
            candidates.push(Candidate { node: entry_point.node.clone(), score: entry_point.score });
        }
        id
    });

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));

    let candidate_count = candidates.len();

    candidates.truncate(opts.nb_top_candidates.max(1));

    let candidate_diagnostics: Vec<_> = candidates
        .iter()
        .map(|candidate| CandidateDiagnostic {
            node: node_diagnostic(&candidate.node),
            score: round_score(candidate.score),
            selected_by: if selected_entry_id == Some(dom::node_id(&candidate.node)) {
                CandidateSelection::EntryPointPreselection
            } else {
                CandidateSelection::CandidateScoring
            },
        })
        .collect();

    let top_candidate = candidates[0].node.clone();
    let top_score = candidates[0].score;
    let top_id = dom::node_id(&top_candidate);
    let score_by_id: HashMap<usize, f64> = candidates
        .iter()
        .map(|candidate| (dom::node_id(&candidate.node), candidate.score))
        .collect();

    let parent = top_candidate.parent().unwrap_or_else(|| doc.clone());
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

    let selected_root = included.first().map(node_diagnostic);
    let (attempt, cleanup) = serialize_roots(included, opts, flags, base_url, title)?;
    let content_selector = opts
        .content_selector
        .as_ref()
        .map(|selector| ContentSelectorDiagnostic { selector: selector.clone(), matched: false, selected: None });
    let attempt_diagnostic = AttemptDiagnostic {
        index,
        flags: flags.into(),
        candidate_count,
        candidates: candidate_diagnostics,
        entry_points: entry_points
            .iter()
            .map(|entry_point| entry_point.diagnostic.clone())
            .collect(),
        selected_root,
        cleanup: Some(cleanup),
        text_len: attempt.text_len,
        accepted: attempt.text_len >= opts.char_threshold,
    };

    Ok(Some((
        attempt,
        GrabDiagnostics { attempt: attempt_diagnostic, content_selector },
    )))
}

struct GrabDiagnostics {
    attempt: AttemptDiagnostic,
    content_selector: Option<ContentSelectorDiagnostic>,
}

fn serialize_roots(
    roots: Vec<NodeRef>, opts: &ReadabilityOptions, flags: ExtractFlags, base_url: Option<&Url>, title: Option<&str>,
) -> Result<(ExtractAttempt, CleanupDiagnostic), Error> {
    let text_len_before = serialize::text_content(&roots).encode_utf16().count();
    let element_count_before = roots.iter().map(element_count).sum();
    let root_selectors = roots.iter().map(node_selector).collect();

    cleanup::cleanup_article(&roots, opts, flags, base_url, title);

    let mut content = String::from(r#"<div id="readability-page-1" class="page">"#);
    for node in &roots {
        if dom::node_name(node) == "body" {
            content.push_str(&serialize::serialize_children(node)?);
        } else {
            content.push_str(&serialize::serialize_node(node)?);
        }
    }
    content.push_str("</div>");
    let text_content = serialize::text_content(&roots);
    let text_len = text_content.encode_utf16().count();
    let element_count_after = roots.iter().map(element_count).sum();

    let attempt = ExtractAttempt { metadata: Metadata::default(), content, text_content, text_len };
    let cleanup = CleanupDiagnostic {
        roots: root_selectors,
        text_len_before,
        text_len_after: text_len,
        element_count_before,
        element_count_after,
        removed_elements: element_count_before.saturating_sub(element_count_after),
    };

    Ok((attempt, cleanup))
}

impl From<ExtractFlags> for FlagDiagnostic {
    fn from(flags: ExtractFlags) -> Self {
        Self {
            strip_unlikely: flags.strip_unlikely,
            weight_classes: flags.weight_classes,
            clean_conditionally: flags.clean_conditionally,
        }
    }
}

struct EntryPointCandidate {
    node: NodeRef,
    score: f64,
    diagnostic: CandidateDiagnostic,
}

fn entry_point_candidates(document: &NodeRef) -> Vec<EntryPointCandidate> {
    // TODO: could this be a constant?
    let selectors = [
        "article",
        "main",
        r#"[role="main"]"#,
        "#content",
        "#main",
        "#article",
        ".content",
        ".main",
        ".article",
        ".post",
        ".entry-content",
    ];
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for selector in selectors {
        for node in dom::select_nodes(document, selector) {
            if !seen.insert(dom::node_id(&node)) {
                continue;
            }
            let text_len = dom::inner_text(&node).chars().count();
            if text_len < 80 {
                continue;
            }
            let link_density = scoring::link_density(&node);
            if link_density > 0.65 {
                continue;
            }
            let score = (text_len as f64 / 25.0) * (1.0 - link_density).max(0.0)
                + scoring::class_weight(&node, ExtractFlags::all()) as f64;
            let diagnostic = CandidateDiagnostic {
                node: node_diagnostic(&node),
                score: round_score(score),
                selected_by: CandidateSelection::EntryPointPreselection,
            };
            candidates.push(EntryPointCandidate { node, score, diagnostic });
        }
    }

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.truncate(8);
    candidates
}

fn selected_entry_point(entry_points: &[EntryPointCandidate]) -> Option<Candidate> {
    let best = entry_points.first()?;
    if best.diagnostic.node.text_len < 140 || best.score < 8.0 {
        return None;
    }
    Some(Candidate { node: best.node.clone(), score: best.score + 25.0 })
}

fn node_diagnostic(node: &NodeRef) -> NodeDiagnostic {
    let class = dom::attr(node, "class").unwrap_or_default();
    NodeDiagnostic {
        selector: node_selector(node),
        tag: dom::node_name(node),
        id: dom::attr(node, "id"),
        classes: class.split_whitespace().map(str::to_string).collect(),
        text_len: dom::inner_text(node).encode_utf16().count(),
        link_density: round_score(scoring::link_density(node)),
    }
}

fn node_selector(node: &NodeRef) -> String {
    let tag = dom::node_name(node);
    if tag.is_empty() {
        return "<node>".to_string();
    }
    if let Some(id) = dom::attr(node, "id") {
        if !id.trim().is_empty() {
            return format!("{tag}#{id}");
        }
    }
    if let Some(class) = dom::attr(node, "class") {
        let mut classes = class.split_whitespace().take(3).collect::<Vec<_>>();
        if !classes.is_empty() {
            classes.insert(0, tag.as_str());
            return classes.join(".");
        }
    }
    tag
}

fn element_count(node: &NodeRef) -> usize {
    node.descendants().filter(|node| node.as_element().is_some()).count()
}

fn round_score(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
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
    fn content_selector_override_forces_root_and_reports_diagnostics() {
        let report = extract_with_diagnostics(
            r#"
            <html><body>
                <main>
                    <p>Navigation teaser text that should lose when the explicit selector points elsewhere.</p>
                </main>
                <section id="forced">
                    <p>This forced article body has enough punctuation, enough words, and enough detail to become the returned content.</p>
                </section>
            </body></html>
            "#,
            None,
            &ReadabilityOptions {
                char_threshold: 0,
                content_selector: Some("#forced".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let article = report.article.unwrap();
        assert!(article.text_content.contains("forced article body"));
        assert!(!article.text_content.contains("Navigation teaser"));

        let selector = report.diagnostics.content_selector.unwrap();
        assert_eq!(selector.selector, "#forced");
        assert!(selector.matched);
        assert_eq!(selector.selected.unwrap().selector, "section#forced");
        assert!(report.diagnostics.attempts[0].cleanup.is_some());
    }

    #[test]
    fn unmatched_content_selector_falls_back_to_scoring_and_reports_candidates() {
        let report = extract_with_diagnostics(
            r#"
            <html><body>
                <article class="story">
                    <p>This article body is long enough, punctuated enough, and clean enough to be selected by normal scoring.</p>
                </article>
            </body></html>
            "#,
            None,
            &ReadabilityOptions {
                char_threshold: 0,
                content_selector: Some(".missing".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(
            report
                .article
                .unwrap()
                .text_content
                .contains("selected by normal scoring")
        );
        let selector = report.diagnostics.content_selector.unwrap();
        assert!(!selector.matched);
        assert!(report.diagnostics.attempts[0].candidate_count > 0);
        assert!(!report.diagnostics.attempts[0].entry_points.is_empty());
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
