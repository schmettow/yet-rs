// SPDX-License-Identifier: GPL-3.0-only

//! A small Markdown subset for experimenter-authored welcome and debrief pages.
//!
//! Supported: headings (`#` to `###`), paragraphs, unordered and ordered
//! lists, block quotes, horizontal rules, and inline `**bold**`, `*italic*`
//! and `` `code` ``. This is deliberately not a full Markdown implementation;
//! it covers the text an experimenter is likely to put in `Intro.md` or
//! `Debrief.md` without another dependency.

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};

/// Render `text` into the current vertical layout.
///
/// `scale` multiplies the base font size, so a fullscreen presenter can use a
/// larger page than a windowed one.
pub fn show_markdown(ui: &mut egui::Ui, text: &str, scale: f32) {
    let base = 26.0 * scale.max(0.5);
    let color = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            ui.add_space(base * 0.6);
            continue;
        }

        if let Some(heading) = trimmed.strip_prefix("### ") {
            heading_label(ui, heading, base * 1.35, strong);
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            heading_label(ui, heading, base * 1.7, strong);
        } else if let Some(heading) = trimmed.strip_prefix("# ") {
            heading_label(ui, heading, base * 2.15, strong);
        } else if trimmed == "---" || trimmed == "***" {
            ui.separator();
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            bullet(ui, "•", item, base, color);
        } else if let Some(item) = ordered_item(trimmed) {
            bullet(ui, item.0, item.1, base, color);
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            ui.horizontal(|ui| {
                ui.add_space(base * 0.5);
                let job = inline_job(quote, base, color, strong, true);
                ui.add(egui::Label::new(job).wrap());
            });
        } else {
            ui.add(egui::Label::new(inline_job(trimmed, base, color, strong, false)).wrap());
        }
    }
}

fn heading_label(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.add_space(size * 0.15);
    ui.add(
        egui::Label::new(inline_job(text, size, color, color, false))
            .wrap()
            .selectable(false),
    );
    ui.add_space(size * 0.15);
}

fn bullet(ui: &mut egui::Ui, marker: &str, text: &str, size: f32, color: egui::Color32) {
    let strong = ui.visuals().strong_text_color();
    ui.horizontal(|ui| {
        ui.add_space(size * 0.4);
        ui.label(egui::RichText::new(marker).size(size).color(color));
        ui.add(egui::Label::new(inline_job(text, size, color, strong, false)).wrap());
    });
}

/// Split `12. text` into the marker and the body.
fn ordered_item(line: &str) -> Option<(&str, &str)> {
    let dot = line.find(". ")?;
    if dot == 0 || !line[..dot].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((&line[..=dot], &line[dot + 2..]))
}

/// Build a wrapped layout job with inline emphasis.
fn inline_job(
    text: &str,
    size: f32,
    color: egui::Color32,
    strong_color: egui::Color32,
    italic: bool,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut rest = text;
    let normal = TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        italics: italic,
        ..Default::default()
    };
    let strong = TextFormat {
        font_id: egui::FontId::proportional(size),
        color: strong_color,
        italics: italic,
        ..Default::default()
    };

    while !rest.is_empty() {
        let Some(index) = rest.find(['*', '`']) else {
            job.append(rest, 0.0, normal.clone());
            break;
        };
        if index > 0 {
            job.append(&rest[..index], 0.0, normal.clone());
            rest = &rest[index..];
        }

        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                job.append(&after[..end], 0.0, strong.clone());
                rest = &after[end + 2..];
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let mut emphasized = normal.clone();
                emphasized.italics = !italic;
                job.append(&after[..end], 0.0, emphasized);
                rest = &after[end + 1..];
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                let code = TextFormat {
                    font_id: egui::FontId::monospace(size * 0.95),
                    color: strong_color,
                    ..Default::default()
                };
                job.append(&after[..end], 0.0, code);
                rest = &after[end + 1..];
                continue;
            }
        }

        // An unmatched marker is literal text; consume one character.
        let next = rest.chars().next().map(char::len_utf8).unwrap_or(1);
        job.append(&rest[..next], 0.0, normal.clone());
        rest = &rest[next..];
    }

    job
}

#[cfg(test)]
mod tests {
    use super::show_markdown;

    #[test]
    fn a_page_with_all_block_kinds_renders() {
        let ctx = eframe::egui::Context::default();
        let text = "# Welcome\n\nThank you.\n\n- first\n- **second**\n\n1. one\n2. two\n\n> note\n\n---\n\n`code` and *emphasis*";

        let mut output = ctx.run_ui(Default::default(), |ui| show_markdown(ui, text, 1.0));

        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
    }

    #[test]
    fn an_empty_page_renders_nothing() {
        let ctx = eframe::egui::Context::default();

        let mut output = ctx.run_ui(Default::default(), |ui| show_markdown(ui, "", 1.0));

        assert!(output.shapes.is_empty());
        output.textures_delta.clear();
    }
}
