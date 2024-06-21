use std::cell::RefCell;
use std::mem;
use egui::scroll_area::ScrollBarVisibility;
use egui::{Id, Key, Label, Sense, TextEdit};
use json_flat_parser::{FlatJsonValueOwned, PointerKey, ValueType};
use crate::ArrayResponse;

pub struct ObjectTable {
    pub nodes: FlatJsonValueOwned,

    // Handling interaction

    pub editing_index: RefCell<Option<(usize)>>,
    pub editing_value: RefCell<String>,
}

impl ObjectTable {
    pub fn new(nodes: FlatJsonValueOwned) -> Self {
        Self {
            nodes,
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
                header.col(|ui, _| {Some(ui.label("Pointer"))});
                header.col(|ui, _| {Some(ui.label("Value"))});
            }).body(None, None, |body| {
            let vec = self.nodes.iter()
                .filter(|(pointer, _)| {
                    !matches!(pointer.value_type, ValueType::Array(_)) &&
                        !matches!(pointer.value_type, ValueType::Object(_))
                }).collect::<Vec<&(PointerKey, Option<String>)>>();
            let mut updated_value: Option<(PointerKey, String)> = None;
            body.rows(text_height, vec.len(), |mut row| {
                let row_index = row.index();
                let (pointer, value) = &vec[row_index];
                row.col(|c, _| Some(c.label(&pointer.pointer)));
                row.col(|ui, _| {
                    let mut editing_index = self.editing_index.borrow_mut();
                    if editing_index.is_some() && editing_index.unwrap() == (row_index) {
                        let ref_mut = &mut *self.editing_value.borrow_mut();
                        let textedit_response = ui.add(TextEdit::singleline(ref_mut));
                        if textedit_response.lost_focus() || ui.ctx().input(|input| input.key_pressed(Key::Enter)) {
                            let pointer = pointer.clone();
                            updated_value = Some((pointer, mem::take(ref_mut)))
                        } else {
                            textedit_response.request_focus();
                        }
                        None
                    } else {
                        let rect = ui.available_rect_before_wrap();
                        let cell_zone = ui.interact(rect, Id::new(&pointer.pointer), Sense::click());
                        let response = value.as_ref().map(|v| ui.add(Label::new(v).sense(Sense::click())))
                            .unwrap_or_else(|| ui.label(""));
                        if cell_zone.clicked() || response.clicked() {
                            *self.editing_value.borrow_mut() = value.clone().unwrap_or(String::new());
                            *editing_index = Some(row_index);
                        }
                        Some(response)
                    }
                });
            });
            if let Some((pointer, value)) = updated_value {
                let editing_index = mem::take(&mut *self.editing_index.borrow_mut());
                let row_index = editing_index.unwrap();
                let value = if value.is_empty() {
                    None
                } else {
                    Some(value)
                };
                if let Some(entry) = self.nodes.get_mut(row_index) {
                    entry.1 = value.clone();
                } else {
                    self.nodes.push((pointer.clone(), value.clone()));
                }
                array_response.edited_value = Some((pointer, value));
            }
        });
        array_response
    }
}

impl super::View<ArrayResponse> for ObjectTable {
    fn ui(&mut self, ui: &mut egui::Ui) -> ArrayResponse{
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