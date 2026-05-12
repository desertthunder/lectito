use once_cell::sync::Lazy;
use regex::Regex;

pub(crate) static UNLIKELY_CANDIDATES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)-ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote",
    )
    .expect("valid unlikely-candidates regex")
});

pub(crate) static MAYBE_CANDIDATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)and|article|body|column|content|main|mathjax|shadow").expect("valid ok-maybe regex"));

pub(crate) static POSITIVE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story")
        .expect("valid positive regex")
});

pub(crate) static NEGATIVE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget")
        .expect("valid negative regex")
});

pub(crate) static NORMALIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").expect("valid whitespace regex"));

pub(crate) static COMMA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\u{002C}|\u{060C}|\u{FE50}|\u{FE10}|\u{FE11}|\u{2E41}|\u{2E34}|\u{2E32}|\u{FF0C}")
        .expect("valid comma regex")
});

pub(crate) static SHARE_ELEMENTS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(\b|_)(share|sharedaddy)(\b|_)").expect("valid share regex"));

pub(crate) static AD_OR_LOADING_WORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?iu)^(ad(vertising|vertisement)?|pub(licité)?|werb(ung)?|广告|Реклама|Anuncio|(loading|正在加载|Загрузка|chargement|cargando)(…|\.\.\.)?)$").expect("valid ad/loading regex")
});

pub(crate) const TAGS_TO_SCORE: &[&str] = &["section", "h2", "h3", "h4", "h5", "h6", "p", "td", "pre"];

pub(crate) const DEPRECATED_SIZE_ATTRIBUTE_ELEMS: &[&str] = &["table", "th", "td", "hr", "pre"];

pub(crate) const DEFAULT_CLASSES_TO_PRESERVE: &[&str] = &["page"];

pub(crate) const PRESENTATIONAL_ATTRIBUTES: &[&str] = &[
    "align",
    "background",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "frame",
    "hspace",
    "rules",
    "style",
    "valign",
    "vspace",
];

pub(crate) fn normalize_spaces(text: &str) -> String {
    NORMALIZE.replace_all(text, " ").into_owned()
}

pub(crate) fn has_display_none(style: Option<&str>) -> bool {
    style
        .unwrap_or_default()
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .any(|(property, value)| {
            property.trim().eq_ignore_ascii_case("display") && value.trim().eq_ignore_ascii_case("none")
        })
}

pub(crate) fn selector(pattern: &str) -> scraper::Selector {
    scraper::Selector::parse(pattern).expect("internal selector should parse")
}
