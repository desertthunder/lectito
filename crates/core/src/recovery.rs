use once_cell::sync::Lazy;
use regex::Regex;

use kuchiki::NodeRef;

use super::diagnostics::RecoveryDiagnostic;
use super::{dom, patterns};

static MOBILE_MEDIA_BLOCK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)@media[^{]*max-width\s*:\s*(\d+)px[^{]*\{").expect("valid media regex"));

static CSS_RULE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)(?P<selectors>[^{}]+)\{(?P<body>[^{}]*display\s*:\s*[^;}]+[^{}]*)\}")
        .expect("valid css rule regex")
});

static DISPLAY_DECL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)display\s*:\s*(?P<display>[a-z-]+)").expect("valid display regex"));
static SHADOW_TEMPLATE_HTML: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<template\s+[^>]*(?:shadowrootmode|shadowroot)\s*=\s*["']?[^"'\s>]+["']?[^>]*>(?P<body>.*?)</template>"#)
        .expect("valid shadow template regex")
});

pub(crate) fn recover_html_snapshot(html: &str) -> (String, RecoveryDiagnostic) {
    let mut flattened = 0;
    let html = SHADOW_TEMPLATE_HTML
        .replace_all(html, |captures: &regex::Captures<'_>| {
            flattened += 1;
            captures
                .name("body")
                .map(|body| body.as_str())
                .unwrap_or_default()
                .to_string()
        })
        .into_owned();
    (
        html,
        RecoveryDiagnostic { shadow_roots_flattened: flattened, mobile_rules_applied: 0 },
    )
}

pub(crate) fn recover(document: &NodeRef, mobile_viewport_width: Option<usize>) -> RecoveryDiagnostic {
    let mut diagnostic = RecoveryDiagnostic {
        shadow_roots_flattened: flatten_declarative_shadow_dom(document),
        mobile_rules_applied: 0,
    };
    if let Some(width) = mobile_viewport_width {
        diagnostic.mobile_rules_applied = apply_mobile_display_rules(document, width);
    }
    diagnostic
}

fn flatten_declarative_shadow_dom(document: &NodeRef) -> usize {
    let mut flattened = 0;
    for template in dom::select_nodes(document, r#"template[shadowrootmode], template[shadowroot]"#) {
        let children: Vec<_> = template.children().collect();
        if children.is_empty() {
            continue;
        }
        for child in children {
            template.insert_before(child);
        }
        template.detach();
        flattened += 1;
    }
    flattened
}

fn apply_mobile_display_rules(document: &NodeRef, viewport_width: usize) -> usize {
    let mut applied = 0;
    for style in dom::select_nodes(document, "style") {
        let css = style.text_contents();
        for media in MOBILE_MEDIA_BLOCK.captures_iter(&css) {
            let max_width = media
                .get(1)
                .and_then(|width| width.as_str().parse::<usize>().ok())
                .unwrap_or(0);
            if max_width == 0 || viewport_width > max_width {
                continue;
            }
            let Some(body_start) = media.get(0).map(|matched| matched.end()) else {
                continue;
            };
            let body = &css[body_start..];
            for rule in CSS_RULE.captures_iter(body) {
                let display = rule
                    .name("body")
                    .and_then(|body| DISPLAY_DECL.captures(body.as_str()))
                    .and_then(|captures| {
                        captures
                            .name("display")
                            .map(|display| display.as_str().to_ascii_lowercase())
                    });
                let Some(display) = display else {
                    continue;
                };
                if display == "none" {
                    continue;
                }
                let Some(selectors) = rule.name("selectors").map(|selectors| selectors.as_str()) else {
                    continue;
                };
                for selector in selectors
                    .split(',')
                    .map(str::trim)
                    .filter(|selector| safe_selector(selector))
                {
                    for node in dom::select_nodes(document, selector) {
                        if remove_display_none(&node) {
                            applied += 1;
                        }
                    }
                }
            }
        }
    }
    applied
}

fn safe_selector(selector: &str) -> bool {
    !selector.is_empty()
        && selector.len() < 120
        && !selector.contains(':')
        && selector.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '#' | '.' | '-' | '_' | '[' | ']' | '=' | '"' | '\'' | ' ')
        })
}

fn remove_display_none(node: &NodeRef) -> bool {
    let Some(style) = dom::attr(node, "style") else {
        return false;
    };
    if !patterns::has_display_none(Some(&style)) {
        return false;
    }

    let declarations: Vec<_> = style
        .split(';')
        .filter(|declaration| {
            let Some((property, _)) = declaration.split_once(':') else {
                return !declaration.trim().is_empty();
            };
            !property.trim().eq_ignore_ascii_case("display")
        })
        .map(str::trim)
        .filter(|declaration| !declaration.is_empty())
        .collect();
    if declarations.is_empty() {
        dom::remove_attr(node, "style");
    } else {
        dom::set_attr(node, "style", &declarations.join("; "));
    }
    true
}
