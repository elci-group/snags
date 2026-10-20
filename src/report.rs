//! Terminal and JSON rendering for snag listings and detail views.

use anyhow::Result;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{Attachment, Severity, Snag, Status};

pub fn print_table(snags: &[Snag]) {
    if snags.is_empty() {
        println!("no snags");
        return;
    }

    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("ID").add_attribute(Attribute::Bold),
            Cell::new("SEVERITY").add_attribute(Attribute::Bold),
            Cell::new("STATUS").add_attribute(Attribute::Bold),
            Cell::new("TITLE").add_attribute(Attribute::Bold),
            Cell::new("TAGS").add_attribute(Attribute::Bold),
            Cell::new("REPORTER").add_attribute(Attribute::Bold),
            Cell::new("AGE").add_attribute(Attribute::Bold),
            Cell::new("ATTACHMENTS").add_attribute(Attribute::Bold),
        ]);

    for snag in snags {
        table.add_row(vec![
            Cell::new(format!("#{}", snag.id)),
            severity_cell(snag.severity),
            status_cell(snag.status),
            Cell::new(&snag.title),
            Cell::new(snag.tags.join(", ")),
            Cell::new(&snag.reporter),
            Cell::new(humanize_age(snag.created_at)),
            Cell::new(snag.attachment_count.to_string()),
        ]);
    }

    println!("{table}");
}

pub fn print_report(snags: &[Snag]) {
    if snags.is_empty() {
        println!("no outstanding snags");
        return;
    }
    print_table(snags);
    println!();
    println!("{} outstanding — {}", snags.len(), severity_breakdown(snags));
}

pub fn print_show(snag: &Snag, attachments: &[Attachment], full: bool) {
    println!("#{} [{}] {}", snag.id, snag.severity, snag.title);
    println!("status:   {}", snag.status);
    if let Some(event) = &snag.last_event {
        let note = if event.note.is_empty() {
            String::new()
        } else {
            format!(" — {}", event.note)
        };
        println!(
            "          {} by {} {}{note}",
            event.status,
            event.actor,
            humanize_age(event.at)
        );
    }
    println!("reporter: {}", snag.reporter);
    println!("age:      {}", humanize_age(snag.created_at));
    if !snag.tags.is_empty() {
        println!("tags:     {}", snag.tags.join(", "));
    }
    if !snag.description.is_empty() {
        println!();
        println!("{}", snag.description);
    }
    if !attachments.is_empty() {
        println!();
        println!("attachments:");
        for attachment in attachments {
            println!(
                "  [{}] {} ({} bytes, {})",
                attachment.kind,
                attachment.filename,
                attachment.content.len(),
                humanize_age(attachment.captured_at)
            );
            if full {
                println!("{}", indent(&attachment.content));
                println!();
            }
        }
    }
}

pub fn print_snags_json(snags: &[Snag]) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(snags)?);
    Ok(())
}

#[derive(Serialize)]
struct SnagDetail<'a> {
    #[serde(flatten)]
    snag: &'a Snag,
    attachments: &'a [Attachment],
}

pub fn print_show_json(snag: &Snag, attachments: &[Attachment]) -> Result<()> {
    let detail = SnagDetail { snag, attachments };
    println!("{}", serde_json::to_string_pretty(&detail)?);
    Ok(())
}

fn severity_cell(severity: Severity) -> Cell {
    let color = match severity {
        Severity::Critical => Color::Red,
        Severity::High => Color::DarkYellow,
        Severity::Medium => Color::Yellow,
        Severity::Low => Color::Grey,
    };
    Cell::new(severity.to_string()).fg(color)
}

fn status_cell(status: Status) -> Cell {
    match status {
        Status::Open => Cell::new("open").fg(Color::Red),
        Status::Resolved => Cell::new("resolved").fg(Color::Green),
    }
}

fn severity_breakdown(snags: &[Snag]) -> String {
    let mut counts = [0usize; 4];
    for snag in snags {
        counts[snag.severity as usize] += 1;
    }
    format!(
        "{} critical, {} high, {} medium, {} low",
        counts[3], counts[2], counts[1], counts[0]
    )
}

fn humanize_age(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let secs = now.saturating_sub(at);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
