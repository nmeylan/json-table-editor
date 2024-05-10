use std::mem;
use egui::{Context, Ui};
use crate::parser::{JsonArrayEntries, JSONParser, ParseOptions};
use crate::parser::parser::ValueType;
use crate::table::{Column, Table};
use crate::{concat_string, View};

pub struct SubTable {
    name: String,
    table: Table,
}

impl SubTable {
    pub fn new(name: String, content: String, parent_value_type: ValueType) -> Self {
        let mut parser = JSONParser::new(content.as_str());
        let options = ParseOptions::default().parse_array(false).start_parse_at(name.clone()).prefix(name.clone()).max_depth(10);
        let result = parser.parse(options.clone()).unwrap();
        let (nodes, columns) = JSONParser::as_array(result).unwrap();
        Self {
            name: name.clone(),
            table: Table::new(nodes, columns, 10, name, parent_value_type),
        }
    }
    pub(crate) fn name(&self) -> &String {
        &self.name
    }

    pub(crate) fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new(self.name())
            .open(open)
            .resizable([true, true])
            .show(ctx, |ui| {
                let id = self.name().to_string();
                ui.push_id(id, |ui| {
                    self.ui(ui);
                });
            });
    }
}

impl super::View for SubTable {
    fn ui(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            self.table.ui(ui)
        });
    }
}