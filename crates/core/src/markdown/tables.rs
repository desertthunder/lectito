use kuchiki::NodeRef;

use super::{RenderContext, math, normalize_markdown, render_children};
use crate::{dom, patterns, serialize};

pub(super) fn render_table(node: &NodeRef, ctx: RenderContext) -> String {
    let rows = table_rows(node);
    if rows.is_empty() {
        return String::new();
    }

    if has_spanning_cell(node) && !math::has_math(node) {
        return raw_html_table(node);
    }

    if is_layout_table(node, &rows) {
        return unwrap_table(node, ctx);
    }

    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.len() < 2 || width < 2 || !rows.iter().all(|row| row.len() == width) {
        return unwrap_table(node, ctx);
    }

    let mut output = String::new();
    output.push_str("\n\n");
    output.push_str(&pipe_row(&rows[0]));
    output.push('\n');
    output.push_str(&separator_row(width));
    output.push('\n');
    for row in rows.iter().skip(1) {
        output.push_str(&pipe_row(row));
        output.push('\n');
    }
    output.push('\n');
    output
}

fn table_rows(node: &NodeRef) -> Vec<Vec<String>> {
    direct_rows(node)
        .into_iter()
        .map(|row| {
            row.children()
                .filter(|child| matches!(dom::node_name(child).as_str(), "td" | "th"))
                .map(|cell| table_cell_text(&cell))
                .collect::<Vec<_>>()
        })
        .filter(|row| !row.is_empty())
        .collect()
}

fn direct_rows(node: &NodeRef) -> Vec<NodeRef> {
    let mut rows = Vec::new();
    collect_direct_rows(node, &mut rows);
    rows
}

fn collect_direct_rows(node: &NodeRef, rows: &mut Vec<NodeRef>) {
    for child in node.children() {
        match dom::node_name(&child).as_str() {
            "tr" => rows.push(child),
            "thead" | "tbody" | "tfoot" => collect_direct_rows(&child, rows),
            _ => {}
        }
    }
}

fn table_cell_text(cell: &NodeRef) -> String {
    let content = render_children(cell, RenderContext { in_pre: false, list_depth: 0 });
    let content = normalize_markdown(&content);
    escape_table_cell(&patterns::normalize_spaces(content.trim()))
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn pipe_row(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

fn separator_row(width: usize) -> String {
    let cells = vec!["---"; width];
    pipe_row(&cells.into_iter().map(str::to_string).collect::<Vec<_>>())
}

fn has_spanning_cell(node: &NodeRef) -> bool {
    dom::select_nodes(node, "td, th")
        .into_iter()
        .any(|cell| span_value(&cell, "rowspan") > 1 || span_value(&cell, "colspan") > 1)
}

fn span_value(node: &NodeRef, attr: &str) -> usize {
    dom::attr(node, attr)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1)
}

fn is_layout_table(node: &NodeRef, rows: &[Vec<String>]) -> bool {
    if dom::attr(node, "role").as_deref() == Some("presentation")
        || dom::attr(node, "datatable").as_deref() == Some("0")
    {
        return true;
    }

    let class_id = dom::class_id_string(node).to_ascii_lowercase();
    if ["layout", "presentation", "wrapper", "container"]
        .iter()
        .any(|token| class_id.contains(token))
    {
        return true;
    }

    if dom::select_nodes(node, "table")
        .into_iter()
        .any(|table| dom::node_id(&table) != dom::node_id(node))
    {
        return true;
    }

    let has_header_signal = dom::attr(node, "summary").is_some()
        || !dom::select_nodes(node, "caption, col, colgroup, tfoot, thead, th").is_empty();
    if has_header_signal {
        return false;
    }

    let row_count = rows.len();
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if row_count < 2 || column_count < 2 {
        return true;
    }

    (math::has_math(node) && !has_data_density(rows))
        || (has_layout_cell_content(node) && row_count.saturating_mul(column_count) < 10)
}

fn has_data_density(rows: &[Vec<String>]) -> bool {
    let row_count = rows.len();
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    row_count >= 2 && column_count >= 2 && row_count.saturating_mul(column_count) >= 10
}

fn has_layout_cell_content(node: &NodeRef) -> bool {
    dom::select_nodes(node, "td, th").into_iter().any(|cell| {
        !dom::select_nodes(
            &cell,
            "p, div, section, article, header, footer, ul, ol, table, form, figure",
        )
        .is_empty()
    })
}

fn unwrap_table(node: &NodeRef, ctx: RenderContext) -> String {
    let mut output = String::new();
    for row in direct_rows(node) {
        for cell in row
            .children()
            .filter(|child| matches!(dom::node_name(child).as_str(), "td" | "th"))
        {
            output.push_str(&render_children(&cell, ctx));
            output.push('\n');
        }
    }
    format!("\n\n{}\n\n", normalize_markdown(&output))
}

fn raw_html_table(node: &NodeRef) -> String {
    let html = serialize::serialize_node(node).unwrap_or_else(|_| node.to_string());
    format!("\n\n{}\n\n", html.trim())
}
