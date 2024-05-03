use std::collections::HashMap;
use egui::{Align, Sense, Ui};
use serde_json::Value;
use crate::components::table::TableBuilder;
use crate::flatten;
use crate::flatten::{Column, flatten, PointerKey, ValueType};

pub struct Table {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    max_depth: usize,
    nodes: Vec<Value>,
    pub flatten_nodes: Vec<Vec<(PointerKey, Option<String>)>>,
    non_null_columns: Vec<String>,
    pub next_frame_reset_scroll: bool,
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
    pub fn new(nodes: Vec<Value>, depth: u8) -> Self {
        let (flatten_nodes, mut all_columns) = flatten::flatten(&nodes, depth, &vec![]);
        all_columns.sort();
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            flatten_nodes,
            max_depth: depth as usize,
            nodes,
            non_null_columns: vec![],
            // states
            next_frame_reset_scroll: false,
        }
    }

    pub fn update_selected_columns(&mut self, depth: u8) {
        let (flatten_nodes, mut all_columns) = flatten::flatten(&self.nodes, depth, &self.non_null_columns);
        all_columns.sort();
        self.all_columns = all_columns;
        self.flatten_nodes = flatten_nodes;
        let column_selected = Self::selected_columns(&self.all_columns, depth);
        self.column_selected = column_selected;
    }
    pub fn update_max_depth(&mut self, depth: u8) {
        self.max_depth = depth as usize;
        self.update_selected_columns(depth);
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


        self.draw_table(ui, text_height);
    }

    fn draw_table(&mut self, ui: &mut Ui, text_height: f32) {
        use crate::components::table::{Column, TableBuilder};
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::hover())
            .cell_layout(egui::Layout::left_to_right(egui::Align::LEFT))
            .min_scrolled_height(0.0);

        if self.next_frame_reset_scroll {
            table = table.scroll_to_row(0, Some(Align::TOP));
            self.next_frame_reset_scroll = false;
        }
        table = table.columns(Column::initial(100.0).clip(true).resizable(true), self.column_selected.len());
        table
            .header(text_height, |mut header| {
                let mut clicked_column = None;
                for column in self.column_selected.iter() {
                    let (_, response) = header.col(|ui| {
                        let mut chcked = self.non_null_columns.contains(&column.name);
                        if ui.checkbox(&mut chcked, "").clicked() {
                            clicked_column = Some(column.name.clone());
                        }
                        ui.strong(&column.name);
                    });
                }
                if let Some(clicked_column) = clicked_column {
                    self.on_non_null_column_click(clicked_column);
                }
            })
            .body(|mut body| {
                body.rows(text_height, self.flatten_nodes.len(), |mut row| {
                    let node = self.flatten_nodes.get(row.index());
                    if let Some(data) = node.as_ref() {
                        row.cols(|(index)| {
                            // println!("visible {}", self.column_selected[index].name);
                            let column = self.column_selected.get(index).unwrap();
                            let key = &column.name;
                            let data = data.iter().find(|(pointer, _)| pointer.pointer.eq(key));
                            if let Some((pointer, value)) = data {
                                if let Some(value) = value.as_ref() {
                                    if matches!(pointer.value_type, ValueType::Null) {
                                        return None;
                                    } else {
                                        return Some(value);
                                    }
                                } else {
                                    return None;
                                }
                            } else {
                                return None;
                            }
                        });
                    }
                });
            });
    }

    fn on_non_null_column_click(&mut self, column: String) {
        if self.non_null_columns.is_empty() {
            self.non_null_columns.push(column);
        } else {
            if self.non_null_columns.contains(&column) {
                self.non_null_columns.retain(|c| !c.eq(&column));
            } else {
                self.non_null_columns.push(column);
            }
        }
        let (flatten_nodes, _) = flatten::flatten(&self.nodes, self.max_depth as u8, &self.non_null_columns);
        self.flatten_nodes = flatten_nodes;
        self.next_frame_reset_scroll = true;
    }
}
