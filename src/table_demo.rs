use std::collections::HashMap;
use egui::TextStyle;
use serde_json::Value;

pub struct TableDemo {
    all_columns: Vec<String>,
    column_selected: Vec<usize>,
    root_node: Value,
    filtered_data: Vec<HashMap<String, String>>,
    should_refresh_data: bool,
}

impl super::View for TableDemo {
    fn ui(&mut self, ui: &mut egui::Ui) {
        // Leave room for the source code link after the table demo:
        let body_text_size = TextStyle::Body.resolve(ui.style()).size;
        use egui_extras::{Size, StripBuilder};
        StripBuilder::new(ui)
            .size(Size::remainder().at_least(100.0)) // for the table
            .size(Size::exact(body_text_size)) // for the source code link
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        self.table_ui(ui);
                    });
                });
            });
    }
}

impl TableDemo {
    pub fn new(all_columns: Vec<String>, root_node: Value) -> Self {
        let mut column_selected = vec![];
        for (i, col) in all_columns.iter().enumerate() {
            match col.as_str() {
                "id" => column_selected.push(i),
                "name" => column_selected.push(i),
                _ => {}
            }
        }
        Self {
            all_columns,
            column_selected,
            root_node,
            filtered_data: Default::default(),
            should_refresh_data: true,
        }
    }
    fn table_ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Column, TableBuilder};
        if self.should_refresh_data {
            self.filtered_data.clear();
            for row in self.root_node.as_array().unwrap() {
                let mut row_data: HashMap<String, String> = HashMap::new();
                let data = row.as_object().unwrap();
                row_data.clear();
                for index in  self.column_selected.iter() {
                    row_data.insert(self.all_columns[*index].to_string(), data.get(&self.all_columns[*index]).map_or_else(|| "".to_string(), |v| v.to_string()));
                }
                self.filtered_data.push(row_data);
            }

            self.should_refresh_data = false;
        }

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::initial(100.0).range(40.0..=300.0))
            .column(Column::initial(100.0).at_least(40.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        table
            .header(20.0, |mut header| {
                for index in  self.column_selected.iter() {
                    header.col(|ui| {
                        ui.strong(&self.all_columns[*index]);
                    });
                }
            })
            .body(|mut body| {
                for row_data in self.filtered_data.iter() {
                    body.row(30.0, |mut row| {
                        for index in  self.column_selected.iter() {
                            row.col(|ui| {
                                ui.label(row_data.get(&self.all_columns[*index]).unwrap());
                            });
                        }
                    });
                }

            });
    }


}
