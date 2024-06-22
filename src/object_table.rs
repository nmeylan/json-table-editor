use std::cell::RefCell;
use std::mem;
use egui::scroll_area::ScrollBarVisibility;
use egui::{Id, Key, Label, Sense, TextEdit};
use json_flat_parser::{FlatJsonValue, PointerKey, ValueType};
use json_flat_parser::serializer::serialize_to_json_with_option;
use crate::ArrayResponse;

pub struct ObjectTable {
    pub nodes: Vec<FlatJsonValue<String>>,
    filtered_nodes: Vec<usize>,
    arrays: Vec<FlatJsonValue<String>>,

    // Handling interaction

    pub editing_index: RefCell<Option<(usize)>>,
    pub editing_value: RefCell<String>,
}

impl ObjectTable {
    pub fn new(nodes: Vec<FlatJsonValue<String>>) -> Self {
        let mut filtered_nodes = Vec::with_capacity(nodes.len());
        let mut arrays = vec![];
        for (index, entry) in nodes.iter().enumerate() {
            if !matches!(entry.pointer.value_type, ValueType::Array(_)) && !matches!(entry.pointer.value_type, ValueType::Object(_)) {
                filtered_nodes.push(index);
            } else if matches!(entry.pointer.value_type, ValueType::Array(_)) {
                arrays.push(entry.clone());
            }
        }
        Self {
            nodes,
            filtered_nodes,
            arrays,
            editing_index: RefCell::new(None),
            editing_value: RefCell::new("".to_string()),
        }
    }

    fn table_ui(&mut self, ui: &mut egui::Ui, pinned: bool) -> ArrayResponse {
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        let mut array_response = ArrayResponse::default();
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
                header.col(|ui, _| { Some(ui.label("Pointer")) });
                header.col(|ui, _| { Some(ui.label("Value")) });
            }).body(None, None, |body| {
            let mut updated_value: Option<(PointerKey, String)> = None;
            body.rows(text_height, self.filtered_nodes.len(), |mut row| {
                let row_index = self.filtered_nodes[row.index()];
                let entry = &self.nodes[row_index];
                row.col(|c, _| Some(c.label(&entry.pointer.pointer)));
                row.col(|ui, _| {
                    let mut editing_index = self.editing_index.borrow_mut();
                    if editing_index.is_some() && editing_index.unwrap() == (row_index) {
                        let ref_mut = &mut *self.editing_value.borrow_mut();
                        let textedit_response = ui.add(TextEdit::singleline(ref_mut));
                        if textedit_response.lost_focus() || ui.ctx().input(|input| input.key_pressed(Key::Enter)) {
                            let pointer = entry.pointer.clone();
                            updated_value = Some((pointer, mem::take(ref_mut)))
                        } else {
                            textedit_response.request_focus();
                        }
                        None
                    } else {
                        let rect = ui.available_rect_before_wrap();
                        let cell_zone = ui.interact(rect, Id::new(&entry.pointer.pointer), Sense::click());
                        let response = entry.value.as_ref().map(|v| ui.add(Label::new(v).sense(Sense::click())))
                            .unwrap_or_else(|| ui.label(""));
                        if cell_zone.clicked() || response.clicked() {
                            *self.editing_value.borrow_mut() = entry.value.clone().unwrap_or(String::new());
                            *editing_index = Some(row_index);
                        }
                        Some(response)
                    }
                });
            });
            if let Some((updated_pointer, value)) = updated_value {
                let editing_index = mem::take(&mut *self.editing_index.borrow_mut());
                let row_index = editing_index.unwrap();
                let value = if value.is_empty() {
                    None
                } else {
                    Some(value)
                };
                let mut value_changed = false;
                if let Some(entry) = self.nodes.get_mut(row_index) {
                    if !entry.value.eq(&value) {
                        entry.value = value.clone();
                        value_changed = true;
                    }
                } else {
                    if !value.is_none() {
                        value_changed = true;
                        self.nodes.insert(self.nodes.len() - 1, FlatJsonValue { pointer: updated_pointer.clone(), value: value.clone() });
                    }
                }
                if !value_changed {
                    return;
                }
                let mut maybe_parent_array = None;
                for array in self.arrays.iter() {
                    if updated_pointer.pointer.starts_with(&array.pointer.pointer) {
                        maybe_parent_array = Some(array);
                        break;
                    }
                }
                if let Some(parent_array) = maybe_parent_array {
                    let mut array_entries = Vec::with_capacity(10);
                    let depth = parent_array.pointer.depth;
                    for node in self.nodes.iter() {
                        if node.pointer.pointer.starts_with(&parent_array.pointer.pointer) {
                            array_entries.push(node.clone());
                        }
                    }
                    let mut parent_pointer = PointerKey {
                        pointer: String::new(),
                        value_type: ValueType::Array(array_entries.len()),
                        depth: 0,
                        index: 0,
                        position: 0,
                    };
                    array_entries.push(FlatJsonValue { pointer: parent_pointer, value: None });
                    let updated_array = serialize_to_json_with_option::<String>(&mut array_entries, depth + 1).to_json();
                    array_response.edited_value = Some(FlatJsonValue { pointer: parent_array.pointer.clone(), value: Some(updated_array) });
                } else {
                    array_response.edited_value = Some(FlatJsonValue::<String> { pointer: updated_pointer, value });
                }
            }
        });
        array_response
    }
}

impl super::View<ArrayResponse> for ObjectTable {
    fn ui(&mut self, ui: &mut egui::Ui) -> ArrayResponse {
        use egui_extras::{Size, StripBuilder};
        let mut array_response = ArrayResponse::default();
        StripBuilder::new(ui)
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    ui.vertical(|ui| {
                        let mut scroll_area = egui::ScrollArea::horizontal();
                        scroll_area.show(ui, |ui| {
                            array_response = self.table_ui(ui, false);
                        });
                    });
                });
            });
        array_response
    }
}