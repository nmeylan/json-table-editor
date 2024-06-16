use egui::scroll_area::ScrollBarVisibility;
use egui::Sense;
use json_flat_parser::{FlatJsonValueOwned, PointerKey, ValueType};

pub struct ObjectTable {
    pub nodes: FlatJsonValueOwned,
}

impl ObjectTable {
    pub fn new(nodes: FlatJsonValueOwned) -> Self {
        Self {
            nodes
        }
    }

    fn table_ui(&mut self, ui: &mut egui::Ui, pinned: bool) {
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        use crate::components::table::{Column, TableBuilder};
        let parent_height = ui.available_rect_before_wrap().height();
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::LEFT))
            .min_scrolled_height(0.0)
            .max_scroll_height(parent_height)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            ;
        table = table.column(Column::initial(140.0).clip(true).resizable(true));
        table = table.column(Column::initial(340.0).clip(true).resizable(true));
        table
            .header(text_height * 2.0, |mut header| {
                header.col(|ui, _| {Some(ui.label("Pointer"))});
                header.col(|ui, _| {Some(ui.label("Value"))});
            }).body(None, None, |body| {
            let vec = self.nodes.iter()
                .filter(|(pointer, _)| {
                    !matches!(pointer.value_type, ValueType::Array(_)) &&
                        !matches!(pointer.value_type, ValueType::Object(_))
                }).collect::<Vec<&(PointerKey, Option<String>)>>();
            body.rows(text_height, vec.len(), |mut row| {
                let (pointer, value) = &vec[row.index()];
                row.col(|c, _| Some(c.label(&pointer.pointer)));
                row.col(|c, _| { Some(value.as_ref().map(|v| c.label(v)).unwrap_or_else(|| c.label(""))) });
            });
        });
    }
}

impl super::View for ObjectTable {
    fn ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Size, StripBuilder};
        StripBuilder::new(ui)
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    ui.vertical(|ui| {
                        let mut scroll_area = egui::ScrollArea::horizontal();
                        let _scroll_area_output = scroll_area.show(ui, |ui| {
                            self.table_ui(ui, false);
                        });
                    });
                });
            });
    }
}