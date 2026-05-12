use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use lectito_core::{Article, ExtractionDiagnostics, ReadabilityOptions, ReadableOptions, extract};
use lectito_core::{extract_with_diagnostics, is_probably_readable};
use owo_colors::OwoColorize;
use reqwest::blocking::Client;
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const USER_AGENT: &str = "curl/8.7.1 lectito/0.1";

#[derive(Debug, Parser)]
#[command(name = "readability")]
#[command(about = "Extract and inspect readable article content")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Parse(ParseArgs),
    Readable(ReadableArgs),
    Fixture(FixtureArgs),
}

#[derive(Debug, Args)]
struct ParseArgs {
    path: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    url: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[arg(long)]
    pretty: bool,
    #[arg(long)]
    max_elems_to_parse: Option<usize>,
    #[arg(long, default_value_t = 500)]
    char_threshold: usize,
    #[arg(long, default_value_t = 5)]
    nb_top_candidates: usize,
    #[arg(long)]
    content_selector: Option<String>,
    #[arg(long)]
    mobile_viewport_width: Option<usize>,
    #[arg(long, value_enum)]
    diagnostic_format: Option<DiagnosticFormat>,
    #[arg(long)]
    disable_json_ld: bool,
    #[arg(long)]
    keep_classes: bool,
    #[arg(long)]
    classes_to_preserve: Vec<String>,
}

#[derive(Debug, Args)]
struct ReadableArgs {
    path: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    pretty: bool,
    #[arg(long, default_value_t = 140)]
    min_content_length: usize,
    #[arg(long, default_value_t = 20.0)]
    min_score: f32,
}

#[derive(Debug, Args)]
struct FixtureArgs {
    path: PathBuf,
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    diff_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Html,
    Markdown,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DiagnosticFormat {
    Json,
    Pretty,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Parse(args) => {
            let html = read_input(args.path.as_deref(), args.stdin, args.url.as_deref())?;
            let options = ReadabilityOptions {
                max_elems_to_parse: args.max_elems_to_parse,
                nb_top_candidates: args.nb_top_candidates,
                char_threshold: args.char_threshold,
                content_selector: args.content_selector,
                mobile_viewport_width: args.mobile_viewport_width.or(Some(480)),
                classes_to_preserve: args.classes_to_preserve,
                keep_classes: args.keep_classes,
                disable_json_ld: args.disable_json_ld,
                link_density_modifier: 0.0,
            };
            let report = extract_with_diagnostics(&html, args.url.as_deref(), &options)?;
            print_parse_output(report.article.as_ref(), args.format, args.pretty)?;
            if let Some(format) = args.diagnostic_format {
                io::stdout().flush().context("failed to flush parse output")?;
                print_diagnostics(&report.diagnostics, format)?;
            }
        }
        Command::Readable(args) => {
            let html = read_input(args.path.as_deref(), args.stdin, args.url.as_deref())?;
            let options = ReadableOptions { min_content_length: args.min_content_length, min_score: args.min_score };
            let readable = is_probably_readable(&html, &options)?;
            print_readable_output(readable, args.json, args.pretty)?;
        }
        Command::Fixture(args) => run_fixture(&args)?,
    }

    Ok(())
}

fn print_diagnostics(diagnostics: &ExtractionDiagnostics, format: DiagnosticFormat) -> anyhow::Result<()> {
    match format {
        DiagnosticFormat::Json => {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(diagnostics).context("failed to serialize diagnostics")?
            );
        }
        DiagnosticFormat::Pretty => {
            eprintln!("{}", "readability diagnostics".bold().blue());
            eprintln!("{} {:?}", "outcome:".bold(), diagnostics.outcome);
            if let Some(selector) = &diagnostics.content_selector {
                let status =
                    if selector.matched { "matched".green().to_string() } else { "not matched".yellow().to_string() };
                eprintln!("{} {} ({status})", "content selector:".bold(), selector.selector);
            }
            for attempt in &diagnostics.attempts {
                let marker = if Some(attempt.index) == diagnostics.selected_attempt {
                    "*".green().to_string()
                } else {
                    " ".to_string()
                };
                eprintln!(
                    "{marker} {} {} {} {} {}",
                    "attempt".bold(),
                    attempt.index,
                    "text_len=".dimmed(),
                    attempt.text_len,
                    if attempt.accepted {
                        "accepted".green().to_string()
                    } else {
                        "below threshold".yellow().to_string()
                    }
                );
                if let Some(root) = &attempt.selected_root {
                    eprintln!(
                        "  {} {} (text {}, links {:.3})",
                        "root:".bold(),
                        root.selector,
                        root.text_len,
                        root.link_density
                    );
                }
                if attempt.recovery.shadow_roots_flattened > 0 || attempt.recovery.mobile_rules_applied > 0 {
                    eprintln!(
                        "  {} shadow_roots={}, mobile_rules={}",
                        "recovery:".bold(),
                        attempt.recovery.shadow_roots_flattened,
                        attempt.recovery.mobile_rules_applied
                    );
                }
                if !attempt.entry_points.is_empty() {
                    eprintln!("  {}", "entry points:".bold());
                    for candidate in attempt.entry_points.iter().take(3) {
                        eprintln!(
                            "    {:>8.3} {} text={}",
                            candidate.score, candidate.node.selector, candidate.node.text_len
                        );
                    }
                }
                if !attempt.candidates.is_empty() {
                    eprintln!("  {}", "top candidates:".bold());
                    for candidate in attempt.candidates.iter().take(5) {
                        eprintln!(
                            "    {:>8.3} {} text={} links={:.3}",
                            candidate.score,
                            candidate.node.selector,
                            candidate.node.text_len,
                            candidate.node.link_density
                        );
                    }
                }
                if let Some(cleanup) = &attempt.cleanup {
                    eprintln!(
                        "  {} text {} -> {}, elements {} -> {} (removed {})",
                        "cleanup:".bold(),
                        cleanup.text_len_before,
                        cleanup.text_len_after,
                        cleanup.element_count_before,
                        cleanup.element_count_after,
                        cleanup.removed_elements
                    );
                }
            }
        }
    }

    Ok(())
}

fn read_input(path: Option<&Path>, read_stdin: bool, url: Option<&str>) -> anyhow::Result<String> {
    if read_stdin && path.is_some() {
        anyhow::bail!("cannot combine --stdin with a file path");
    }

    if read_stdin {
        let mut html = String::new();
        io::stdin().read_to_string(&mut html).context("failed to read stdin")?;
        return Ok(html);
    }

    if let Some(path) = path {
        return std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()));
    }

    if let Some(url) = url {
        return fetch_url(url);
    }

    anyhow::bail!("pass either --stdin, a file path, or --url without a file path")
}

fn fetch_url(url: &str) -> anyhow::Result<String> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .with_context(|| format!("failed to build HTTP client for {url}"))?;

    client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.text())
        .with_context(|| format!("HTTP request failed for {url}"))
}

fn print_parse_output(article: Option<&Article>, format: OutputFormat, pretty: bool) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            if pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&article).context("failed to serialize JSON")?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&article).context("failed to serialize JSON")?
                );
            }
        }
        OutputFormat::Html => {
            if let Some(article) = article {
                println!("{}", article.content);
            }
        }
        OutputFormat::Markdown => {
            if let Some(article) = article {
                println!("{}", article.markdown);
            }
        }
        OutputFormat::Text => {
            if let Some(article) = article {
                println!("{}", article.text_content);
            }
        }
    }

    Ok(())
}

fn print_readable_output(readable: bool, json: bool, pretty: bool) -> anyhow::Result<()> {
    if json {
        let value = serde_json::json!({ "readable": readable });
        if pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).context("failed to serialize JSON")?
            );
        } else {
            println!("{}", serde_json::to_string(&value).context("failed to serialize JSON")?);
        }
    } else {
        println!("{readable}");
    }

    Ok(())
}

fn run_fixture(args: &FixtureArgs) -> anyhow::Result<()> {
    let fixture = load_fixture_arg(&args.path)?;
    let expected_readable = fixture
        .expected_metadata
        .get("readerable")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let actual_readable = is_probably_readable(&fixture.source, &ReadableOptions::default())?;
    let article = extract(&fixture.source, args.url.as_deref(), &ReadabilityOptions::default())?;
    let metadata_mismatches = article
        .as_ref()
        .map(|article| metadata_mismatches(&fixture.expected_metadata, article))
        .unwrap_or_else(|| vec!["article: expected extracted article, got none".to_string()]);
    let content_report = article
        .as_ref()
        .map(|article| content_report(&fixture.expected_content, &article.content));

    println!(
        "readable: {}",
        if actual_readable == expected_readable { "pass" } else { "mismatch" }
    );
    println!(
        "metadata: {}",
        if metadata_mismatches.is_empty() { "pass" } else { "mismatch" }
    );
    for mismatch in &metadata_mismatches {
        println!("  - {mismatch}");
    }

    if let Some(report) = &content_report {
        println!(
            "content text: {}",
            if report.text_matches { "pass" } else { "mismatch" }
        );
        println!("content tags: {}", if report.tags_match { "pass" } else { "mismatch" });
        println!("expected text chars: {}", report.expected_text_chars);
        println!("actual text chars: {}", report.actual_text_chars);
        println!("expected tags: {}", report.expected_tags);
        println!("actual tags: {}", report.actual_tags);
    } else {
        println!("content text: mismatch");
        println!("content tags: mismatch");
    }

    if let (Some(diff_dir), Some(article), Some(report)) = (&args.diff_dir, &article, &content_report) {
        write_fixture_diff(diff_dir, &fixture.name, &fixture.expected_content, article, report)?;
        println!("diff: {}", diff_dir.display());
    }

    Ok(())
}

fn load_fixture_arg(path: &Path) -> anyhow::Result<lectito_fixtures::Fixture> {
    if path.exists() {
        return lectito_fixtures::load_fixture_path(path)
            .with_context(|| format!("failed to load fixture {}", path.display()));
    }

    let name = path
        .to_str()
        .context("fixture name must be valid UTF-8 when it is not a path")?;
    lectito_fixtures::load_fixture(name).with_context(|| format!("failed to load sample fixture {name}"))
}

#[derive(Debug)]
struct ContentReport {
    text_matches: bool,
    tags_match: bool,
    expected_text: String,
    actual_text: String,
    expected_text_chars: usize,
    actual_text_chars: usize,
    expected_tags: usize,
    actual_tags: usize,
    expected_tag_sequence: Vec<String>,
    actual_tag_sequence: Vec<String>,
}

fn content_report(expected_html: &str, actual_html: &str) -> ContentReport {
    let expected_text = lectito_fixtures::normalized_text(expected_html);
    let actual_text = lectito_fixtures::normalized_text(actual_html);
    let expected_tag_sequence = lectito_fixtures::tag_sequence(expected_html);
    let actual_tag_sequence = lectito_fixtures::tag_sequence(actual_html);

    ContentReport {
        text_matches: expected_text == actual_text,
        tags_match: expected_tag_sequence == actual_tag_sequence,
        expected_text_chars: expected_text.chars().count(),
        actual_text_chars: actual_text.chars().count(),
        expected_tags: expected_tag_sequence.len(),
        actual_tags: actual_tag_sequence.len(),
        expected_text,
        actual_text,
        expected_tag_sequence,
        actual_tag_sequence,
    }
}

fn metadata_mismatches(expected: &serde_json::Value, article: &Article) -> Vec<String> {
    let checks = [
        ("title", article.title.as_deref()),
        ("byline", article.byline.as_deref()),
        ("dir", article.dir.as_deref()),
        ("excerpt", article.excerpt.as_deref()),
        ("siteName", article.site_name.as_deref()),
        ("publishedTime", article.published_time.as_deref()),
        ("image", article.image.as_deref()),
        ("domain", article.domain.as_deref()),
        ("favicon", article.favicon.as_deref()),
    ];

    checks
        .into_iter()
        .filter_map(|(field, actual)| {
            let expected = expected.get(field);
            if expected.is_none() {
                return None;
            }
            let expected = expected.and_then(serde_json::Value::as_str);
            let matches = match (expected, actual) {
                (Some(expected), Some(actual)) if field == "excerpt" => {
                    lectito_fixtures::normalize_space(expected) == lectito_fixtures::normalize_space(actual)
                }
                (Some(expected), Some(actual)) => expected == actual,
                (None, None) => true,
                _ => false,
            };
            (!matches).then(|| format!("{field}: expected {:?}, got {:?}", expected, actual))
        })
        .collect()
}

fn write_fixture_diff(
    diff_dir: &Path, fixture_name: &str, expected_content: &str, article: &Article, report: &ContentReport,
) -> anyhow::Result<()> {
    let dir = diff_dir.join(sanitize_path_segment(fixture_name));
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("expected.html"), expected_content)?;
    std::fs::write(dir.join("actual.html"), &article.content)?;
    std::fs::write(dir.join("expected.txt"), &report.expected_text)?;
    std::fs::write(dir.join("actual.txt"), &report.actual_text)?;
    std::fs::write(dir.join("expected-tags.txt"), report.expected_tag_sequence.join("\n"))?;
    std::fs::write(dir.join("actual-tags.txt"), report.actual_tag_sequence.join("\n"))?;
    Ok(())
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '_' })
        .collect()
}
