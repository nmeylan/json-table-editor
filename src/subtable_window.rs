use eframe::egui::{Context, Ui};
use egui::Order;
use json_flat_parser::{FlatJsonValue, ParseOptions, ParseResult, PointerKey, ValueType};
use json_flat_parser::lexer::Lexer;
use json_flat_parser::parser::Parser;
use crate::array_table::{ArrayTable};
use crate::{ArrayResponse, View};
use crate::object_table::ObjectTable;

pub struct SubTable<'array> {
    name: String,
    array_table: Option<ArrayTable<'array>>,
    object_table: Option<ObjectTable>,
    row_index: usize,
}

impl <'array>SubTable<'array> {
    pub fn new(parent_pointer: PointerKey, content: String, parent_value_type: ValueType,
               index_in_json_entries_array: usize, depth: u8) -> Self {
        let name = parent_pointer.pointer.clone();
        if matches!(parent_value_type, ValueType::Array(_)) {

            let options = ParseOptions::default().parse_array(false).start_parse_at(name.clone()).prefix(name.clone()).start_depth(depth + 1).max_depth(10);
            let result = Self::parse(&content, &options, false);
            let (nodes, columns) = crate::parser::as_array(result).unwrap();
            let mut array_table = ArrayTable::new(None, nodes, columns, 10, parent_pointer);
            array_table.is_sub_table = true;
            Self {
                name,
                array_table: Some(array_table),
                object_table: None,
                row_index: index_in_json_entries_array,
            }
        } else {
            let options = ParseOptions::default().parse_array(true).keep_object_raw_data(false).start_parse_at(name.clone()).start_depth(depth + 1).prefix(name.clone()).max_depth(10);
            let result = Self::parse(&content, &options, true);
            Self {
                name: name.clone(),
                array_table: None,
                object_table: Some(ObjectTable::new(result.json)),
                row_index: index_in_json_entries_array,
            }
        }
    }

    fn parse(content: &String, options: &ParseOptions, state_seen_start_parse_at: bool) -> ParseResult<String> {
        let mut lexer = Lexer::new(content.as_str().as_bytes());
        let mut parser = Parser::new(&mut lexer);
        parser.state_seen_start_parse_at = state_seen_start_parse_at;
        let result = parser.parse(options, options.start_depth).unwrap().to_owned();
        result
    }
    pub(crate) fn name(&self) -> &String {
        &self.name
    }

    #[inline]
    pub fn id(&self) -> usize {
        self.row_index
    }

    pub fn update_nodes(&mut self, pointer: PointerKey, value: Option<String>) {
        if let Some(ref mut array_table) = self.array_table {
            if let Some(entry) = array_table.nodes[self.row_index].entries.iter_mut()
                .find(|entry| entry.pointer.pointer.eq(&pointer.pointer)) {
                entry.value = value;
            } else {
                array_table.nodes[self.row_index].entries.push(FlatJsonValue::<String>{ pointer, value});
            }
        } else {
            let table = self.object_table.as_mut().unwrap();
            if let Some(entry) = table.nodes.iter_mut().find(|entry| entry.pointer.pointer.eq(&pointer.pointer)) {
                entry.value = value;
            } else {
                table.nodes.push(FlatJsonValue::<String>{ pointer, value});
            }
        }
    }

    pub(crate) fn show(&mut self, ctx: &Context, open: &mut bool) -> Option<Option<ArrayResponse>> {
        egui::Window::new(self.name())
            .open(open)
            .resize(|r| {
                let nodes =  if let Some(ref array_table) = self.array_table {
                    array_table.nodes.len()
                } else if let Some(ref object_table) = self.object_table {
                    object_table.nodes.len()
                } else {
                    1
                };
                r.default_height(40.0 + nodes as f32 * ArrayTable::row_height(&ctx.style(), &ctx.style().spacing)).default_width( 480.0)
            })
            .order(Order::Middle)
            .resizable([true, true])
            .show(ctx, |ui| {
                let id = self.name().to_string();
                ui.push_id(id, |ui| {
                    self.ui(ui)
                }).inner
            }).map(|i| i.inner)
    }
}

impl <'array>super::View<ArrayResponse> for SubTable<'array> {
    fn ui(&mut self, ui: &mut Ui) -> ArrayResponse {
        ui.vertical(|ui| {
            if let Some(ref mut array_table) = self.array_table {
                array_table.ui(ui)
            } else {
                self.object_table.as_mut().unwrap().ui(ui)
            }
        }).inner
    }
}