mod tables;

use comrak::{Arena, Options};
use comrak::{escape_commonmark_inline, escape_commonmark_link_destination, format_commonmark, parse_document};
use kuchiki::NodeRef;
use kuchiki::traits::TendrilSink;

use super::{dom, patterns};

pub fn html_to_markdown(html: &str) -> String {
    let document = kuchiki::parse_html().one(format!("<html><body>{html}</body></html>"));
    let body = dom::select_nodes(&document, "body")
        .into_iter()
        .next()
        .unwrap_or(document);
    let mut output = render_children(&body, RenderContext { in_pre: false, list_depth: 0 });
    output = normalize_markdown(&output);
    format_with_comrak(&output)
}

#[derive(Clone, Copy)]
pub(super) struct RenderContext {
    in_pre: bool,
    list_depth: usize,
}

pub(super) fn render_children(node: &NodeRef, ctx: RenderContext) -> String {
    let mut output = String::new();
    for child in node.children() {
        output.push_str(&render_node(&child, ctx));
    }
    output
}

fn render_node(node: &NodeRef, ctx: RenderContext) -> String {
    if let Some(text) = node.as_text() {
        let text = text.borrow();
        if ctx.in_pre {
            return text.to_string();
        }
        return patterns::normalize_spaces(&text);
    }

    let tag = dom::node_name(node);
    match tag.as_str() {
        "h1" => block(format!("# {}", inline_children(node, ctx))),
        "h2" => block(format!("## {}", inline_children(node, ctx))),
        "h3" => block(format!("### {}", inline_children(node, ctx))),
        "h4" => block(format!("#### {}", inline_children(node, ctx))),
        "h5" => block(format!("##### {}", inline_children(node, ctx))),
        "h6" => block(format!("###### {}", inline_children(node, ctx))),
        "p" => block(inline_children(node, ctx)),
        "br" => "  \n".to_string(),
        "strong" | "b" => format!("**{}**", inline_children(node, ctx)),
        "em" | "i" => format!("*{}*", inline_children(node, ctx)),
        "code" if !ctx.in_pre => format!("`{}`", inline_children(node, ctx).replace('`', "\\`")),
        "pre" => {
            let code = render_children(node, RenderContext { in_pre: true, ..ctx });
            format!("\n\n```\n{}\n```\n\n", code.trim_matches('\n'))
        }
        "a" => {
            let label = inline_children(node, ctx);
            if label.is_empty() {
                String::new()
            } else if let Some(href) = dom::attr(node, "href") {
                format!("[{}]({})", label, escape_commonmark_link_destination(&href))
            } else {
                label
            }
        }
        "img" => {
            let src = dom::attr(node, "src").unwrap_or_default();
            if src.is_empty() {
                String::new()
            } else {
                let alt = dom::attr(node, "alt").unwrap_or_default();
                format!(
                    "![{}]({})",
                    escape_commonmark_inline(&alt),
                    escape_commonmark_link_destination(&src)
                )
            }
        }
        "blockquote" => {
            let inner = normalize_markdown(&render_children(node, ctx));
            let quoted = inner
                .lines()
                .map(|line| if line.trim().is_empty() { ">".to_string() } else { format!("> {line}") })
                .collect::<Vec<_>>()
                .join("\n");
            block(quoted)
        }
        "ul" => render_list(node, false, ctx),
        "ol" => render_list(node, true, ctx),
        "li" => block(inline_children(node, ctx)),
        "table" => tables::render_table(node, ctx),
        "div" | "section" | "article" | "main" | "body" => render_children(node, ctx),
        "figure" => block(render_children(node, ctx)),
        "figcaption" => block(inline_children(node, ctx)),
        "hr" => "\n\n---\n\n".to_string(),
        _ => render_children(node, ctx),
    }
}

fn inline_children(node: &NodeRef, ctx: RenderContext) -> String {
    patterns::normalize_spaces(render_children(node, ctx).trim())
}

fn render_list(node: &NodeRef, ordered: bool, ctx: RenderContext) -> String {
    let mut output = String::new();
    let indent = "  ".repeat(ctx.list_depth);
    let mut index = 1;
    for child in node.children().filter(|child| dom::node_name(child) == "li") {
        let marker = if ordered {
            let marker = format!("{index}.");
            index += 1;
            marker
        } else {
            "-".to_string()
        };
        let text = normalize_markdown(&render_children(
            &child,
            RenderContext { list_depth: ctx.list_depth + 1, ..ctx },
        ));
        let mut lines = text.lines();
        if let Some(first) = lines.next() {
            output.push_str(&format!("{indent}{marker} {first}\n"));
        }
        for line in lines {
            output.push_str(&format!("{indent}  {line}\n"));
        }
    }
    output.push('\n');
    output
}

fn block(value: String) -> String {
    let value = value.trim();
    if value.is_empty() { String::new() } else { format!("\n\n{value}\n\n") }
}

pub(super) fn normalize_markdown(value: &str) -> String {
    let mut output = String::new();
    let mut blank_count = 0;
    for line in value.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                output.push('\n');
            }
        } else {
            blank_count = 0;
            output.push_str(line.trim_start());
            output.push('\n');
        }
    }
    output.trim().to_string()
}

fn format_with_comrak(markdown: &str) -> String {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.table = true;
    let root = parse_document(&arena, markdown, &options);
    let mut output = String::new();
    if format_commonmark(root, &options, &mut output).is_err() {
        return markdown.to_string();
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::html_to_markdown;

    #[test]
    fn converts_representative_article_html() {
        let markdown = html_to_markdown(
            r#"<div><h1>Title</h1><p>Hello <strong>bold</strong> <a href="https://example.com">link</a>.</p><ul><li>One</li><li>Two</li></ul><pre><code>let x = 1;</code></pre></div>"#,
        );

        assert!(markdown.contains("# Title"));
        assert!(markdown.contains("Hello **bold** [link](https://example.com)."));
        assert!(markdown.contains("- One"));
        assert!(markdown.contains("    let x = 1;"));
    }

    #[test]
    fn converts_simple_tables_to_pipe_tables() {
        let markdown = html_to_markdown(
            r#"<table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>A</td><td>x|y</td></tr><tr><td>B</td><td><a href="https://example.com">z</a></td></tr></tbody></table>"#,
        );

        assert!(markdown.contains("| Name | Value |"));
        assert!(markdown.contains("| --- | --- |"));
        assert!(markdown.contains(r"| A | x\|y |"));
        assert!(markdown.contains("| B | [z](https://example.com) |"));
    }

    #[test]
    fn unwraps_layout_tables() {
        let markdown = html_to_markdown(
            r#"<table role="presentation"><tr><td><p>Left</p></td><td><p>Right <strong>side</strong></p></td></tr></table>"#,
        );

        assert!(!markdown.contains("| --- |"));
        assert!(markdown.contains("Left"));
        assert!(markdown.contains("Right **side**"));
    }

    #[test]
    fn preserves_spanning_tables_as_html() {
        let markdown =
            html_to_markdown(r#"<table><tr><th colspan="2">Group</th></tr><tr><td>A</td><td>B</td></tr></table>"#);

        assert!(markdown.contains("<table>"));
        assert!(markdown.contains("colspan=\"2\""));
        assert!(markdown.contains("<td>A</td>"));
    }
}
