use egui::{Button, Color32, Response, Ui};
use crate::components::icon;

pub fn icon(name: &'static str) -> egui::RichText {
    egui::RichText::new(name)
        .family(egui::FontFamily::Name("fa".into()))
        .size(12.0)
}

pub fn button(ui: &mut Ui, name: &'static str, tooltip: &str) -> Response {
    let button = Button::new(icon::icon(name));
    
    ui.add(button).on_hover_ui(|ui| { ui.label(tooltip); } )
}
pub fn button_with_color(ui: &mut Ui, name: &'static str, tooltip: &str, color: Option<Color32>) -> Response {
    let mut icon = icon::icon(name);
    if color.is_some() {
        icon = icon.color(color.unwrap());
    }
    let button = Button::new(icon);

    ui.add(button).on_hover_ui(|ui| { ui.label(tooltip); } )
}