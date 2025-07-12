use eframe::emath::Align;
use eframe::epaint;
use egui::{FontSelection, Id, Response, RichText, Sense, Ui, WidgetText};

pub struct CellText {
    text: WidgetText,
}

impl CellText {
    pub fn new(text: impl Into<WidgetText>) -> CellText {
        CellText { text: text.into() }
    }

    pub fn ui(self, ui: &mut Ui, cell_id: usize) -> Response {
        let rect = ui.available_rect_before_wrap();
        let cell_zone = ui.interact(rect, Id::new(cell_id), Sense::click());

        let valign = ui.text_valign();

        let widget_text = self.text;
        let mut layout_job =
            widget_text.into_layout_job(ui.style(), FontSelection::Default, valign);

        layout_job.break_on_newline = false;
        layout_job.wrap.max_width = f32::INFINITY;
        layout_job.halign = Align::LEFT;
        layout_job.justify = false;
        let galley = ui.fonts(|fonts| fonts.layout_job(layout_job));
        let galley_pos = match galley.job.halign {
            Align::LEFT => rect.left_top(),
            Align::Center => rect.center_top(),
            Align::RIGHT => rect.right_top(),
        };

        ui.painter().add(epaint::TextShape::new(
            galley_pos,
            galley,
            ui.style().visuals.text_color(),
        ));

        cell_zone
    }
}
