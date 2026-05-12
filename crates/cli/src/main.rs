use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use lectito_core::{Article, ReadabilityOptions, ReadableOptions, extract, is_probably_readable};
use reqwest::blocking::Client;
use std::{
    fs,
    io::{self, Read},
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
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Html,
    Text,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Parse(args) => {
            let html = read_input(args.path.as_deref(), args.stdin, args.url.as_deref())?;
            let options = ReadabilityOptions {
                max_elems_to_parse: args.max_elems_to_parse,
                nb_top_candidates: args.nb_top_candidates,
                char_threshold: args.char_threshold,
                classes_to_preserve: args.classes_to_preserve,
                keep_classes: args.keep_classes,
                disable_json_ld: args.disable_json_ld,
                link_density_modifier: 0.0,
            };
            let article = extract(&html, args.url.as_deref(), &options)?;
            print_parse_output(article.as_ref(), args.format, args.pretty)?;
        }
        Command::Readable(args) => {
            let html = read_input(args.path.as_deref(), args.stdin, args.url.as_deref())?;
            let options = ReadableOptions { min_content_length: args.min_content_length, min_score: args.min_score };
            let readable = is_probably_readable(&html, &options)?;
            print_readable_output(readable, args.json, args.pretty)?;
        }
        Command::Fixture(args) => run_fixture(&args.path)?,
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
        return fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()));
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

fn run_fixture(path: &Path) -> anyhow::Result<()> {
    let source_path = path.join("source.html");
    let expected_path = path.join("expected.html");
    let metadata_path = path.join("expected-metadata.json");

    let source =
        fs::read_to_string(&source_path).with_context(|| format!("failed to read {}", source_path.display()))?;
    let metadata =
        fs::read_to_string(&metadata_path).with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let expected_content =
        fs::read_to_string(&expected_path).with_context(|| format!("failed to read {}", expected_path.display()))?;

    let metadata: serde_json::Value =
        serde_json::from_str(&metadata).context("failed to parse expected-metadata.json")?;
    let expected_readable = metadata
        .get("readerable")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let actual_readable = is_probably_readable(&source, &ReadableOptions::default())?;
    let article = extract(&source, None, &ReadabilityOptions::default())?;

    println!(
        "readable: {}",
        if actual_readable == expected_readable { "pass" } else { "mismatch" }
    );
    println!("content: {}", if article.is_some() { "pass" } else { "mismatch" });
    println!("expected content bytes: {}", expected_content.len());

    Ok(())
}
