use std::{
    fs, io,
    path::{Path, PathBuf},
};

use regex::Regex;
use scraper::{Html, Selector};

pub struct Fixture {
    pub name: String,
    pub source: String,
    pub expected_content: String,
    pub expected_metadata: serde_json::Value,
}

pub fn upstream_root() -> PathBuf {
    samples_root()
}

pub fn samples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/test-pages")
}

pub fn load_fixture(name: &str) -> io::Result<Fixture> {
    load_fixture_path(samples_root().join(name))
}

pub fn load_fixture_path(path: impl AsRef<Path>) -> io::Result<Fixture> {
    load_fixture_dir(path.as_ref())
}

pub fn load_all() -> io::Result<Vec<Fixture>> {
    let mut fixtures = Vec::new();
    for entry in fs::read_dir(samples_root())? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            fixtures.push(load_fixture_dir(&entry.path())?);
        }
    }
    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fixtures)
}

fn load_fixture_dir(dir: &Path) -> io::Result<Fixture> {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let source = fs::read_to_string(dir.join("source.html"))?;
    let expected_content = fs::read_to_string(dir.join("expected.html"))?;
    let metadata = fs::read_to_string(dir.join("expected-metadata.json"))?;
    let expected_metadata = serde_json::from_str(&metadata).map_err(io::Error::other)?;

    Ok(Fixture { name, source, expected_content, expected_metadata })
}

pub fn normalized_text(html: &str) -> String {
    let document = Html::parse_fragment(html);
    normalize_space(&document.root_element().text().collect::<String>())
}

pub fn tag_sequence(html: &str) -> Vec<String> {
    let document = Html::parse_fragment(html);
    let selector = Selector::parse("*").expect("universal selector should parse");
    document
        .select(&selector)
        .map(|element| element.value().name().to_string())
        .collect()
}

pub fn normalize_space(text: &str) -> String {
    let whitespace = Regex::new(r"\s+").expect("valid whitespace regex");
    whitespace.replace_all(text.trim(), " ").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_upstream_fixtures() {
        let fixtures = load_all().unwrap();
        assert!(fixtures.len() > 100);
        assert!(fixtures.iter().any(|fixture| fixture.name == "wikipedia"));
    }
}
