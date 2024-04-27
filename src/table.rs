use std::iter::Filter;
use std::slice::Iter;
use serde_json::Value;

pub struct Table {
    all_columns: Vec<String>,
    column_selected: Vec<String>,
    root_node: Value,
}

impl super::View for Table {
    fn ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Size, StripBuilder};
        StripBuilder::new(ui)
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        self.table_ui(ui);
                    });
                });
            });
    }
}

impl Table {
    pub fn new(all_columns: Vec<String>, root_node: Value, depth: u8) -> Self {
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            root_node,
        }
    }

    pub fn update_selected_columns(&mut self, depth: u8) {
        let column_selected = Self::selected_columns(&self.all_columns, depth);
        self.column_selected = column_selected;
    }

    fn selected_columns(all_columns: &Vec<String>, depth: u8) -> Vec<String> {
        let mut column_selected = vec![];
        for col in Self::visible_columns(&all_columns, depth as usize) {
            match col.as_str() {
                // "id" => column_selected.push(i),
                // "name" => column_selected.push(i),
                // _ => {}
                _ => column_selected.push(col.clone())
            }
        }
        column_selected
    }

    pub fn all_columns(&self) -> &Vec<String> {
        &self.all_columns
    }

    pub fn visible_columns(all_columns: &Vec<String>, depth: usize) -> impl Iterator<Item = &String> {
        all_columns.iter().filter(move |column: &&String| column.matches(".").count() <= depth)
    }

    fn table_ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Column, TableBuilder};
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::LEFT))
            .min_scrolled_height(0.0);

        table = table.columns(Column::auto(), self.column_selected.len());
        table
            .header(text_height, |mut header| {
                for column in  self.column_selected.iter() {
                    header.col(|ui| {
                        ui.strong(column);
                    });
                }
            })
            .body(|mut body| {
                let array = self.root_node.as_array().unwrap();
                body.rows(text_height, array.len(), |mut row| {
                    let data = array[row.index()].as_object().unwrap();
                    for key in  self.column_selected.iter() {
                        row.col(|ui| {
                            // ui.label( "column");
                            let key = key.rfind(".").map_or_else(|| key.as_str(), |i| &key[(i+1)..]);
                            data.get(key).map(|v| ui.label(format!("{}", v)));
                        });
                    }
                });

            });
    }


}
