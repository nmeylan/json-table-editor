use egui::{Context, Ui};
use json_flat_parser::{JSONParser, ParseOptions, ValueType};
use crate::array_table::{ArrayTable};
use crate::{View};
use crate::object_table::ObjectTable;

pub struct SubTable {
    name: String,
    array_table: Option<ArrayTable>,
    object_table: Option<ObjectTable>,
}

impl SubTable {
    pub fn new(name: String, content: String, parent_value_type: ValueType) -> Self {
        if matches!(parent_value_type, ValueType::Array(_)) {

            let options = ParseOptions::default().parse_array(false).start_parse_at(name.clone()).prefix(name.clone()).max_depth(10);
            let mut result = JSONParser::parse(content.as_str(), options).unwrap().to_owned();
            let (nodes, columns) = crate::parser::as_array(result).unwrap();
            Self {
                name: name.clone(),
                array_table: Some(ArrayTable::new(None, nodes, columns, 10, name, parent_value_type)),
                object_table: None,
            }
        } else {

            let options = ParseOptions::default().parse_array(false).keep_object_raw_data(false).start_parse_at(name.clone()).prefix(name.clone()).max_depth(10);
            let mut result = JSONParser::parse(content.as_str(), options).unwrap().to_owned();
            Self {
                name: name.clone(),
                array_table: None,
                object_table: Some(ObjectTable::new(result.json)),
            }
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
            if let Some(ref mut array_table) = self.array_table {
                array_table.ui(ui)
            } else {
                self.object_table.as_mut().unwrap().ui(ui)
            }
        });
    }
}