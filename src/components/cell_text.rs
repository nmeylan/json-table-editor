use eframe::emath::Align;
use eframe::epaint;
use std::sync::Arc;
use egui::text::{LayoutJob, TextFormat};
use egui::{FontSelection, Id, Response, RichText, Sense, TextStyle, Ui, WidgetText};

pub struct CellText {
    text: WidgetText,
}

impl CellText {
    pub fn new(text: impl Into<WidgetText>) -> CellText {
        CellText { text: text.into() }
    }

    /// Returns whether the text overflows the cell
    pub fn ui(self, ui: &mut Ui, cell_id: usize) -> (Response, bool) {
        let rect = ui.available_rect_before_wrap();
        let cell_zone = ui.interact(rect, Id::new(cell_id), Sense::click());

        let valign = ui.text_valign();

        let widget_text = self.text;
        let mut layout_job = Arc::unwrap_or_clone(widget_text.into_layout_job(
            ui.style(),
            FontSelection::Default,
            valign,
        ));

        layout_job.break_on_newline = false;
        layout_job.wrap.max_width = f32::INFINITY;
        layout_job.halign = Align::LEFT;
        layout_job.justify = false;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(layout_job));
        let galley_pos = match galley.job.halign {
            Align::LEFT => rect.left_top(),
            Align::Center => rect.center_top(),
            Align::RIGHT => rect.right_top(),
        };

        let overflow = galley.size().x > rect.width();
        ui.painter().add(epaint::TextShape::new(
            galley_pos,
            galley,
            ui.style().visuals.text_color(),
        ));

        (cell_zone, overflow)
    }
}

/// Json pointer with its parent path faded, so the last segment stands out: `/stats/` + `str`.
pub fn pointer_text(ui: &Ui, pointer: &str) -> LayoutJob {
    let split = pointer.rfind('/').map_or(0, |i| i + 1);
    let font_id = TextStyle::Body.resolve(ui.style());
    let mut job = LayoutJob::default();
    job.append(
        &pointer[..split],
        0.0,
        TextFormat::simple(font_id.clone(), ui.visuals().weak_text_color()),
    );
    job.append(
        &pointer[split..],
        0.0,
        TextFormat::simple(font_id, ui.visuals().strong_text_color()),
    );
    job
}
