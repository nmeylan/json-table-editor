use egui::{Context, Ui};

pub const SelectColumnsPanel_id: &str = "Select columns to display";
#[derive(Default)]
pub struct SelectColumnsPanel {
    enabled: bool,
    visible: bool,
    selected_columns: Vec<String>,
}



impl super::Window for SelectColumnsPanel {
    fn name(&self) -> &'static str {
        SelectColumnsPanel_id
    }

    fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new(self.name())
            .open(open)
            .resizable([true, false])
            .default_width(280.0)
            .show(ctx, |ui| {
                use super::View as _;
                self.ui(ui);
            });
    }
}

impl super::View<()> for SelectColumnsPanel {
    fn ui(&mut self, ui: &mut Ui) {
        ui.add_enabled_ui(self.enabled, |ui| {
            ui.set_visible(self.visible);

            egui::Grid::new("select_columns_grid")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .show(ui, |_ui| {

                });
        });
    }
}