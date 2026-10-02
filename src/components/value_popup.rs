use crate::components::icon::ButtonWithIcon;
use crate::fonts::COPY;
use eframe::egui::{
    vec2, Align2, Area, Frame, Id, Key, Order, Rect, Response, RichText, ScrollArea, TextStyle,
    TextWrapMode, Ui, Vec2,
};
use std::ops::Range;
use std::sync::Arc;

/// Popup showing a cell value in full, opened when hovering the cell like a tooltip.
/// Unlike egui tooltip it stays open while pointer is over the cell or the popup, so it can be scrolled and its text selected.
#[derive(Clone)]
struct PopupState {
    cell_id: usize,
    value: Arc<FormattedValue>,
    // A popup not shown on previous pass is closed: its cell scrolled out of view or is being edited
    last_pass: u64,
    rect: Rect,
}

struct FormattedValue {
    text: String,
    is_json: bool,
    // Json only: rendered line by line, so only visible lines are laid out
    lines: Vec<Range<usize>>,
    max_line_chars: usize,
}

pub fn value_popup(ui: &Ui, cell: &Response, cell_id: usize, value: &str, is_json: bool) {
    let ctx = ui.ctx();
    let id = Id::new("cell_value_popup");
    let pass = ctx.cumulative_pass_nr();
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let mut state = match ctx.data(|d| d.get_temp::<PopupState>(id)) {
        Some(state) if state.cell_id == cell_id => {
            let hovered = pointer
                .is_some_and(|pos| cell.rect.contains(pos) || state.rect.expand(4.0).contains(pos));
            if state.last_pass + 1 < pass || !hovered || ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.data_mut(|d| d.remove::<PopupState>(id));
                return;
            }
            state
        }
        _ => {
            if !cell.hovered() {
                return;
            }
            let delay = ui.style().interaction.tooltip_delay;
            let still_since = ctx.input(|i| {
                i.pointer
                    .time_since_last_movement()
                    .min(i.time_since_last_scroll())
            });
            if still_since < delay {
                ctx.request_repaint_after_secs(delay - still_since);
                return;
            }
            PopupState {
                cell_id,
                value: Arc::new(FormattedValue::new(value, is_json)),
                last_pass: pass,
                rect: Rect::NOTHING,
            }
        }
    };

    // At least as wide as the column, at most the window to fit the content
    let screen = ctx.content_rect().shrink(8.0);
    // Area content can't outgrow the size remembered from its first (sizing) pass:
    // an area per cell, sized on the window, so each one fits its own content
    let area = Area::new(id.with(cell_id))
        .default_size(screen.size())
        .order(Order::Tooltip)
        .constrain(true)
        .fixed_pos(cell.rect.left_bottom())
        .pivot(Align2::LEFT_TOP)
        .show(ctx, |ui| {
            let frame = Frame::popup(ui.style());
            let margin = frame.total_margin().sum();
            frame.show(ui, |ui| {
                ui.set_min_width(cell.rect.width() - margin.x);
                if ui.add(ButtonWithIcon::new("Copy", COPY)).clicked() {
                    ui.ctx().copy_text(value.to_string());
                }
                ui.separator();
                let max_size = screen.size() - margin - vec2(0.0, ui.min_rect().height());
                state.value.ui(ui, max_size);
            });
        });
    state.rect = area.response.rect;
    state.last_pass = pass;
    ctx.data_mut(|d| d.insert_temp(id, state));
}

impl FormattedValue {
    fn new(value: &str, is_json: bool) -> Self {
        if !is_json {
            return Self {
                text: value.to_string(),
                is_json,
                lines: vec![],
                max_line_chars: 0,
            };
        }
        let text = format_json(value);
        let mut lines = vec![];
        let mut start = 0;
        for (i, _) in text.match_indices('\n') {
            lines.push(start..i);
            start = i + 1;
        }
        lines.push(start..text.len());
        let max_line_chars = lines
            .iter()
            .map(|line| text[line.clone()].chars().count())
            .max()
            .unwrap_or(0);
        Self {
            text,
            is_json,
            lines,
            max_line_chars,
        }
    }

    fn ui(&self, ui: &mut Ui, max_size: Vec2) {
        ui.style_mut().interaction.selectable_labels = true;
        if !self.is_json {
            ui.set_max_width(max_size.x);
            ScrollArea::vertical()
                .max_height(max_size.y)
                .show(ui, |ui| {
                    ui.label(&self.text);
                });
            return;
        }
        let font_id = TextStyle::Monospace.resolve(ui.style());
        let (row_height, char_width) =
            ui.fonts_mut(|f| (f.row_height(&font_id), f.glyph_width(&font_id, ' ')));
        // Content width from the longest line, as only visible lines are laid out
        let width = char_width * self.max_line_chars as f32;
        ScrollArea::both()
            .max_width(max_size.x)
            .max_height(max_size.y)
            .show_rows(ui, row_height, self.lines.len(), |ui, rows| {
                ui.set_min_width(width);
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                for line in &self.lines[rows] {
                    ui.label(RichText::new(&self.text[line.clone()]).font(font_id.clone()));
                }
            });
    }
}

/// Indent raw json in a single pass, without parsing it
fn format_json(raw: &str) -> String {
    fn new_line(out: &mut Vec<u8>, indent: usize) {
        out.push(b'\n');
        out.resize(out.len() + indent * 2, b' ');
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 2);
    let mut indent = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if in_string {
            out.push(b);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                out.push(b);
            }
            b'{' | b'[' => {
                out.push(b);
                let close = if b == b'{' { b'}' } else { b']' };
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                // Empty object or array stays on one line
                if bytes.get(i) == Some(&close) {
                    out.push(close);
                    i += 1;
                } else {
                    indent += 1;
                    new_line(&mut out, indent);
                }
            }
            b'}' | b']' => {
                indent = indent.saturating_sub(1);
                new_line(&mut out, indent);
                out.push(b);
            }
            b',' => {
                out.push(b);
                new_line(&mut out, indent);
            }
            b':' => out.extend_from_slice(b": "),
            b if b.is_ascii_whitespace() => {}
            b => out.push(b),
        }
    }
    String::from_utf8(out).expect("only ascii bytes are added or removed")
}

#[cfg(test)]
mod tests {
    use super::format_json;

    #[test]
    fn format_json_indents_and_keeps_strings() {
        let raw = r#"{"a": [1,2], "b": {}, "c": "x, {\"y\": [z]}", "d": []}"#;
        assert_eq!(
            format_json(raw),
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {},\n  \"c\": \"x, {\\\"y\\\": [z]}\",\n  \"d\": []\n}"
        );
    }
}
