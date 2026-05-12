use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub struct Fixture {
    pub name: String,
    pub source: String,
    pub expected_content: String,
    pub expected_metadata: serde_json::Value,
}

pub fn upstream_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/test-pages")
}

pub fn load_fixture(name: &str) -> io::Result<Fixture> {
    let dir = upstream_root().join(name);
    load_fixture_dir(&dir)
}

pub fn load_all() -> io::Result<Vec<Fixture>> {
    let mut fixtures = Vec::new();
    for entry in fs::read_dir(upstream_root())? {
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
