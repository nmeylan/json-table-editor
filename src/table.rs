use std::cell::RefCell;
use std::time::Instant;
use egui::{Align, Color32, Context, Label, Sense, Separator, Stroke, TextBuffer, Ui, Vec2, Widget, WidgetText};
use egui::scroll_area::ScrollBarVisibility;
use serde_json::Value;
use crate::components::table::TableBuilder;
use crate::{concat_string, flatten, Window};
use crate::flatten::{Column, value_at};
use crate::parser::parser::{FlatJsonValue, PointerKey, ValueType};
use crate::subtable_window::SubTable;

pub struct Table {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    column_pinned: Vec<Column>,
    max_depth: usize,
    nodes: Vec<FlatJsonValue>,
    scroll_y: f32,
    non_null_columns: Vec<String>,
    pub hovered_row_index: Option<usize>,
    columns_offset: Vec<f32>,
    parent_pointer: String,
    parent_value_type: ValueType,
    windows: Vec<SubTable>,
    pub scroll_to_column: String,

    pub next_frame_reset_scroll: bool,
    pub next_frame_scroll_to_column: bool,
}

impl super::View for Table {
    fn ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Size, StripBuilder};
        self.windows(ui.ctx());
        StripBuilder::new(ui)
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    let parent_size_available = ui.available_rect_before_wrap().height();
                    ui.horizontal(|mut ui| {
                        ui.set_height(parent_size_available);
                        ui.push_id("table-pinned-column", |ui| {
                            ui.vertical(|ui| {
                                self.table_ui(ui, true);
                            })
                        });

                        ui.vertical(|ui| {
                            let mut scroll_to_x = None;
                            if self.next_frame_scroll_to_column {
                                self.next_frame_scroll_to_column = false;
                                let index = self.column_selected.iter().position(|c| {
                                    c.name.to_lowercase().contains(&self.scroll_to_column.to_lowercase())
                                });
                                if let Some(index) = index {
                                    if let Some(offset) = self.columns_offset.get(index) {
                                        scroll_to_x = Some(*offset);
                                    }
                                }
                            }

                            let mut scroll_area = egui::ScrollArea::horizontal();
                            if let Some(offset) = scroll_to_x {
                                scroll_area = scroll_area.scroll_offset(Vec2 { x: offset, y: 0.0 });
                            }
                            let mut scroll_area_output = scroll_area.show(ui, |ui| {
                                self.table_ui(ui, false);
                            });
                        });
                    });
                });
            });
    }
}

impl Table {
    pub fn new(nodes: Vec<FlatJsonValue>, all_columns: Vec<Column>, depth: u8, parent_pointer: String, parent_value_type: ValueType) -> Self {
        let start = Instant::now();
        println!("Flatten structure {}ms", start.elapsed().as_millis());
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            max_depth: depth as usize,
            nodes,
            non_null_columns: vec![],
            // states
            next_frame_reset_scroll: false,
            column_pinned: vec![Column::new("/#".to_string())],
            scroll_y: 0.0,
            hovered_row_index: None,
            columns_offset: vec![],
            parent_pointer,
            parent_value_type,
            windows: vec![],
            scroll_to_column: "".to_string(),
            next_frame_scroll_to_column: false,
        }
    }
    pub fn windows(&mut self, ctx: &Context) {
        let mut closed_windows = vec![];
        for window in self.windows.iter_mut() {
            let mut opened = true;
            window.show(ctx, &mut opened);
            if !opened {
                closed_windows.push(window.name().clone());
            }
        }
        self.windows.retain(|w| !closed_windows.contains(w.name()));
    }

    pub fn update_selected_columns(&mut self, depth: u8) {
        todo!("update_selected_columns not implemented")
        // let (flatten_nodes, mut all_columns) = flatten::flatten(&self.nodes, depth, &self.non_null_columns);
        // all_columns.sort();
        // self.all_columns = all_columns;
        // self.flatten_nodes = flatten_nodes;
        // let column_selected = Self::selected_columns(&self.all_columns, depth);
        // self.column_selected = column_selected;
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

    fn table_ui(&mut self, ui: &mut egui::Ui, pinned: bool) {
        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        self.draw_table(ui, text_height, 7.0, pinned);
    }
    fn draw_table(&mut self, ui: &mut Ui, text_height: f32, text_width: f32, pinned_column_table: bool) {
        use crate::components::table::{Column, TableBuilder};
        let parent_height = ui.available_rect_before_wrap().height();
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::LEFT))
            .min_scrolled_height(0.0)
            .max_scroll_height(parent_height)
            .scroll_bar_visibility(if pinned_column_table { ScrollBarVisibility::AlwaysHidden } else { ScrollBarVisibility::AlwaysVisible })
            ;

        if self.next_frame_reset_scroll {
            table = table.scroll_to_row(0, Some(Align::TOP));
            self.next_frame_reset_scroll = false;
        }
        table = table.vertical_scroll_offset(self.scroll_y);

        let columns_count = if pinned_column_table { self.column_pinned.len() } else { self.column_selected.len() };
        let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
        for i in 0..columns_count {
            if pinned_column_table && i == 0 {
                table = table.column(Column::initial(40.0).clip(true).resizable(true));
                continue;
            }
            table = table.column(Column::initial((columns[i].name.len() + 3) as f32 * text_width).clip(true).resizable(true));
        }
        let mut request_repaint = false;
        let table_scroll_output = table
            .header(text_height * 2.0, |mut header| {
                let clicked_column: RefCell<Option<String>> = RefCell::new(None);
                let mut pinned_column: RefCell<Option<usize>> = RefCell::new(None);
                let mut i: RefCell<usize> = RefCell::new(0);
                header.cols(true, |index| {
                    let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                    let mut column = columns.get(index).unwrap();
                    let name = column.name.clone();
                    let strong = Label::new(WidgetText::RichText(egui::RichText::from(&name)));
                    let label = Label::new(&name);
                    *i.borrow_mut() = index;
                    Some(Box::new(|ui: &mut Ui| {
                        let mut chcked = self.non_null_columns.contains(&column.name);
                        let response = ui.vertical(|ui| {
                            let response = ui.add(strong).on_hover_ui(|ui| { ui.add(label); });

                            if !pinned_column_table || *i.borrow() > 0 {
                                ui.horizontal(|ui| {
                                    let button = egui::Button::new("📌").frame(false);
                                    if ui.add(button).clicked() {
                                        *pinned_column.borrow_mut() = Some(*i.borrow());
                                    }
                                    if ui.checkbox(&mut chcked, "").clicked() {
                                        *clicked_column.borrow_mut() = Some(name);
                                    }
                                });
                            }

                            response
                        });
                        response.inner
                    }))
                });

                let pinned_column = pinned_column.borrow();

                if let Some(pinned_column) = pinned_column.as_ref() {
                    if pinned_column_table {
                        let column = self.column_pinned.remove(*pinned_column);
                        self.column_selected.push(column);
                        self.column_selected.sort();
                    } else {
                        let column = self.column_selected.remove(*pinned_column);
                        self.column_pinned.push(column);
                    }
                }
                let clicked_column = clicked_column.borrow();
                if let Some(clicked_column) = clicked_column.as_ref() {
                    self.on_non_null_column_click(clicked_column.clone());
                }
            })
            .body(self.hovered_row_index, |mut body| {
                let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                let (hovered_row_index) = body.rows(text_height, self.nodes.len(), |mut row| {
                    let row_index = row.index();
                    let node = self.nodes.get(row_index);
                    if let Some(data) = node.as_ref() {
                        let response = row.cols(false, |(index)| {
                            let data = self.get_pointer(columns, data, index);

                            if let Some((pointer, value)) = data {
                                if pinned_column_table && index == 0 {
                                    let label = Label::new(pointer.index.to_string()).sense(Sense::click());
                                    return Some(Box::new(|ui| {
                                        label.ui(ui)
                                    }));
                                }
                                if let Some(value) = value.as_ref() {
                                    if !matches!(pointer.value_type, ValueType::Null) {
                                        let label = Label::new(value).sense(Sense::click());
                                        return Some(Box::new(|ui| {
                                            label.ui(ui)
                                        }));
                                    }
                                }
                            }
                            None
                        });

                        if let Some(index) = response.clicked_col_index {
                            let data = self.get_pointer(columns, data, index);
                            if let Some((pointer, value)) = data {
                                let row_index = pointer.index;
                                let is_array = matches!(pointer.value_type, ValueType::Array);
                                let is_object = matches!(pointer.value_type, ValueType::Object);
                                if is_array || is_object {
                                    todo!("click array")
                                    // if let Some(root) = value_at(&self.nodes[row_index], pointer.pointer.as_str()) {
                                    //     let name = if matches!(self.parent_value_type, ValueType::Array) {
                                    //         format!("{}{}{}", self.parent_pointer, row_index, pointer.pointer)
                                    //     } else {
                                    //         format!("{}{}", self.parent_pointer, pointer.pointer)
                                    //     };
                                    //
                                    //     self.windows.push(SubTable::new(name, root,
                                    //                                     if is_array { ValueType::Array } else { ValueType::Object }))
                                    // } else {
                                    //     println!("can't find root at {} {}", row_index, pointer.pointer)
                                    // }
                                } else {}
                            }
                        }
                    }
                });
                if self.hovered_row_index != hovered_row_index {
                    self.hovered_row_index = hovered_row_index;
                    request_repaint = true;
                }
            });
        if self.scroll_y != table_scroll_output.state.offset.y {
            self.scroll_y = table_scroll_output.state.offset.y;
        }
        if !pinned_column_table {
            self.columns_offset = table_scroll_output.inner;
        }
        if request_repaint {
            ui.ctx().request_repaint();
        }
    }

    fn get_pointer<'a>(&self, columns: &Vec<Column>, data: &&'a FlatJsonValue, index: usize) -> Option<&'a (PointerKey, Option<String>)> {
        if let Some(column) = columns.get(index) {
            let key = &column.name;
            return data.iter().find(|(pointer, _)| {
                let key = concat_string!(self.parent_pointer, "/", pointer.index.to_string(), key);
                pointer.pointer.eq(&key)
            });
        }
        None
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
        todo!("on_non_null_column_click");
        // let (flatten_nodes, _) = flatten::flatten(&self.nodes, self.max_depth as u8, &self.non_null_columns);
        // self.flatten_nodes = flatten_nodes;
        self.next_frame_reset_scroll = true;
    }
}
