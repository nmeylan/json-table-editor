use egui::{Button, Color32, Response, Ui};
use crate::components::icon;

pub fn icon(name: &'static str) -> egui::RichText {
    egui::RichText::new(name)
        .family(egui::FontFamily::Name("fa".into()))
        .size(12.0)
}

pub fn button(ui: &mut Ui, name: &'static str, tooltip: Option<&str>, color: Option<Color32>) -> Response {
    let mut icon = icon::icon(name);
    if color.is_some() {
        icon = icon.color(color.unwrap());
    }
    let button = Button::new(icon);
    let mut response = ui.add(button);
    if let Some(tooltip) = tooltip {
        response = response.on_hover_ui(|ui| { ui.label(tooltip); } );
    }

    response
}