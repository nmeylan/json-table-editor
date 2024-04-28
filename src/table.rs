use egui::Ui;
use serde_json::Value;
use crate::Column;
use crate::components::table::TableBuilder;

pub struct Table {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    max_depth: usize,
    nodes: Vec<Value>,
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
    pub fn new(all_columns: Vec<Column>, nodes: Vec<Value>, depth: u8) -> Self {
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            max_depth: depth as usize,
            nodes,
        }
    }

    pub fn update_selected_columns(&mut self, depth: u8) {
        let column_selected = Self::selected_columns(&self.all_columns, depth);
        self.column_selected = column_selected;
    }
    pub fn update_max_depth(&mut self, depth: u8) {
        self.max_depth = depth as usize;
    }

    fn selected_columns(all_columns: &Vec<Column>, depth: u8) -> Vec<Column> {
        let mut column_selected: Vec<Column> = vec![];
        for col in Self::visible_columns(&all_columns, depth) {
            match col.name.as_str() {
                // "id" => column_selected.push(i),
                // "name" => column_selected.push(i),
                // _ => {}
                _ => column_selected.push(col.clone())
            }
        }
        column_selected
    }

    pub fn all_columns(&self) -> &Vec<Column> {
        &self.all_columns
    }

    pub fn visible_columns(all_columns: &Vec<Column>, depth: u8) -> impl Iterator<Item=&Column> {
        all_columns.iter().filter(move |column: &&Column| column.depth <= depth)
    }

    fn table_ui(&mut self, ui: &mut egui::Ui) {
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        Self::draw_table(ui, text_height, &self.column_selected, &self.nodes, self.max_depth);
    }

    fn draw_table(ui: &mut Ui, text_height: f32, columns: &Vec<Column>, nodes: &Vec<Value>, max_depth: usize) {
        use crate::components::table::{Column, TableBuilder};
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::LEFT))
            .min_scrolled_height(0.0);

        table = table.columns(Column::auto(), columns.len());
        table
            .header(text_height, |mut header| {
                for column in columns.iter() {
                    header.col(|ui| {
                        ui.strong(&column.name);
                    });
                }
            })
            .body(|mut body| {
                body.rows(text_height, nodes.len(), |mut row| {
                    let data = nodes[row.index()].as_object().unwrap();
                    let depth = 1;
                    for column in columns.iter() {
                        let key = &column.name;
                        if column.depth == 1 {
                            if let Some(column_data) = data.get(key) {
                                if column_data.is_array() {
                                    row.col(|ui| { ui.label(format!("{}", column_data)); });
                                } else if column_data.is_object() {
                                    if depth == max_depth {
                                        row.col(|ui| { ui.label(format!("{}", column_data)); });
                                    }
                                } else {
                                    row.col(|ui| { ui.label(format!("{}", column_data)); });
                                }
                            } else {
                                row.empty_col();
                            }
                        } else if column.depth == 2 {
                            println!("depth == 2, {} - {}", column.name, key);
                            let parent = key.find(".").map_or_else(|| key.as_str(), |i| &key[0..i]);
                            println!("{}", parent);
                            if let Some(data) = data.get(parent) {
                                let key = column.name.replace(&format!("{}.", parent), "");
                                println!("{}", key);
                                if let Some(column_data) = data.get(key) {
                                    if column_data.is_array() {
                                        row.col(|ui| { ui.label(format!("{}", column_data)); });
                                    } else if column_data.is_object() {
                                        if depth == max_depth {
                                            row.col(|ui| { ui.label(format!("{}", column_data)); });
                                        }
                                    } else {
                                        row.col(|ui| { ui.label(format!("{}", column_data)); });
                                    }
                                } else {
                                    row.empty_col();
                                }
                            }
                            row.empty_col();
                        } else {
                            row.empty_col();
                        }
                    }
                });
            });
    }
}
