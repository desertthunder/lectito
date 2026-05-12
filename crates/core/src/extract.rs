use std::collections::HashMap;

use kuchiki::NodeRef;
use kuchiki::traits::TendrilSink;
use scraper::Html;
use url::Url;

use super::config::{Article, ExtractFlags, ReadabilityOptions};
use super::error::Error;
use super::patterns::{MAYBE_CANDIDATE, UNLIKELY_CANDIDATES};
use super::{cleanup, dom, metadata, patterns, scoring};
use super::{metadata::Metadata, scoring::Candidate};

pub fn extract(html: &str, base_url: Option<&str>, options: &ReadabilityOptions) -> Result<Option<Article>, Error> {
    let base_url = base_url
        .map(|base_url| Url::parse(base_url).map_err(|_| Error::InvalidBaseUrl(base_url.to_string())))
        .transpose()?;

    let document = Html::parse_document(html);
    enforce_element_limit(&document, options.max_elems_to_parse)?;

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

        let Some(mut attempt) = grab_article(&dom, options, flags, base_url.as_ref())? else {
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
    dom::remove_matching(document, "script, style");

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

fn grab_article(
    document: &NodeRef, options: &ReadabilityOptions, flags: ExtractFlags, base_url: Option<&Url>,
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

    cleanup::cleanup_article(&included, options, flags, base_url);

    let mut content = String::from(r#"<div id="readability-page-1" class="page">"#);
    let mut text_content = String::new();
    for node in &included {
        content.push_str(&dom::serialize_node(node)?);
        text_content.push_str(&node.text_contents());
        text_content.push('\n');
    }
    content.push_str("</div>");
    text_content = patterns::normalize_spaces(text_content.trim());
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
