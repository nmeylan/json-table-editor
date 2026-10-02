use crate::ACTIVE_COLOR;
use crate::array_table::ArrayTable;
use crate::components::icon;
use crate::fonts::FILTER;
use eframe::egui::{Button, Checkbox, Popup, PopupCloseBehavior, Ui};

/// Stateless rendering helpers for table bottom bar.
pub struct TableControlPane;

impl TableControlPane {
    /// Render "X / Y rows" label
    pub fn row_count(ui: &mut Ui, visible: usize, total: usize) {
        if visible == total {
            ui.label(format!("{} rows", visible));
        } else {
            ui.label(format!("{} / {} rows", visible, total));
        }
    }

    /// Filter icon opening a popup to remove one or all active filters.
    pub fn active_filters(ui: &mut Ui, table: &mut ArrayTable) {
        let filtered: Vec<String> = table.row_view().active_filters().cloned().collect();

        if filtered.is_empty() {
            return;
        }

        let icon_response = ui
            .add(Button::new(icon::icon(FILTER).color(ACTIVE_COLOR)).frame(false))
            .on_hover_text(format!("{} filtered column(s)", filtered.len()));

        let mut clear_all = false;
        let mut clear_one: Option<&String> = None;

        Popup::menu(&icon_response)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(220.0);
                if ui.button("Remove all filters").clicked() {
                    clear_all = true;
                    ui.close();
                }
                ui.separator();
                for column in &filtered {
                    ui.horizontal(|ui| {
                        let mut checked = true;
                        if ui.add(Checkbox::without_text(&mut checked)).changed() {
                            clear_one = Some(column);
                        }
                        ui.label(column);
                    });
                }
            });

        if clear_all {
            table.clear_filters();
        } else if let Some(column) = clear_one {
            table.remove_filter(column);
        }
    }
}
