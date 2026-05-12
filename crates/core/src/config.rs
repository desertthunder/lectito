use serde::Serialize;

#[derive(Clone, Debug)]
pub struct ReadabilityOptions {
    pub max_elems_to_parse: Option<usize>,
    pub nb_top_candidates: usize,
    pub char_threshold: usize,
    pub content_selector: Option<String>,
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
            content_selector: None,
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

#[derive(Clone, Copy)]
pub(crate) struct ExtractFlags {
    pub(crate) strip_unlikely: bool,
    pub(crate) weight_classes: bool,
    pub(crate) clean_conditionally: bool,
}

impl ExtractFlags {
    pub(crate) fn all() -> Self {
        Self { strip_unlikely: true, weight_classes: true, clean_conditionally: true }
    }
}
