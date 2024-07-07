use eframe::egui::Context;
use eframe::egui::{Ui};

#[derive(Default)]
pub struct AboutPanel {
    enabled: bool,
    visible: bool,
}



impl super::Window for AboutPanel {
    fn name(&self) -> &'static str {
        "About"
    }

    fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new(self.name())
            .collapsible(false)
            .open(open)
            .resizable([true, true])
            .default_width(280.0)
            .show(ctx, |ui| {
                use super::View as _;
                self.ui(ui);
            });
    }
}

impl super::View<()> for AboutPanel {
    fn ui(&mut self, ui: &mut Ui) {
        ui.heading("About");
        ui.label("Licence: Apache-2.0 license");
        ui.hyperlink_to("View project on Github", "https://github.com/nmeylan/json-table-editor");
        ui.separator();
        ui.heading("Credits");
        ui.hyperlink_to("egui project and its community", "https://github.com/emilk/egui");
        ui.hyperlink_to("Maintainers of dependencies used by this project", "https://github.com/nmeylan/json-table-editor/blob/master/Cargo.lock");
    }
}