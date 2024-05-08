use std::mem;
use egui::{Context, Ui};
use serde_json::Value;
use crate::flatten::ValueType;
use crate::table::Table;
use crate::View;

pub struct SubTable {
    name: String,
    table: Table,
}

impl SubTable {
    pub fn new(name: String, mut root: Value, parent_value_type: ValueType) -> Self {
        let nodes = if let Some(nodes) = root.as_array_mut() {
            mem::take(nodes)
        } else {
            vec![root]
        };
        Self {
            name: name.clone(),
            table: Table::new(nodes, 10, name, parent_value_type),
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