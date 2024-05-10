use std::cell::RefCell;
use std::cmp::Ordering;
use std::mem;
use std::time::Instant;
use egui::{Align, Context, Label, Sense, TextBuffer, Ui, Vec2, Widget, WidgetText};
use egui::scroll_area::ScrollBarVisibility;

use crate::{concat_string, Window};
use crate::parser::{JsonArrayEntries, JSONParser};
use crate::parser::parser::{FlatJsonValue, ParseResult, PointerKey, ValueType};
use crate::subtable_window::SubTable;

#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub depth: u8,
    pub value_type: ValueType,
}

impl Column {
    pub fn new(name: String, value_type: ValueType) -> Self {
        Self {
            name,
            depth: 0,
            value_type,
        }
    }
}

impl Eq for Column {}

impl PartialEq<Self> for Column {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl PartialOrd<Self> for Column {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Column {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

pub struct Table {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    column_pinned: Vec<Column>,
    max_depth: usize,
    last_parsed_max_depth: usize,
    parse_result: Option<ParseResult>,
    nodes: Vec<JsonArrayEntries>,
    filtered_nodes: Vec<JsonArrayEntries>,
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
                    ui.horizontal(|ui| {
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
                            let _scroll_area_output = scroll_area.show(ui, |ui| {
                                self.table_ui(ui, false);
                            });
                        });
                    });
                });
            });
    }
}

impl Table {
    pub fn new(parse_result: Option<ParseResult>, nodes: Vec<JsonArrayEntries>, all_columns: Vec<Column>, depth: u8, parent_pointer: String, parent_value_type: ValueType) -> Self {
        let last_parsed_max_depth = parse_result.as_ref().unwrap().parsing_max_depth;
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            max_depth: depth as usize,
            nodes,
            parse_result,
            non_null_columns: vec![],
            // states
            next_frame_reset_scroll: false,
            column_pinned: vec![Column::new("/#".to_string(), ValueType::Number)],
            scroll_y: 0.0,
            hovered_row_index: None,
            columns_offset: vec![],
            parent_pointer,
            parent_value_type,
            windows: vec![],
            scroll_to_column: "".to_string(),
            next_frame_scroll_to_column: false,
            filtered_nodes: vec![],
            last_parsed_max_depth,
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
        if depth <= self.last_parsed_max_depth as u8 {
            let column_selected = Self::selected_columns(&self.all_columns, depth);
            self.column_selected = column_selected;
        } else {
            let previous_parse_result = self.parse_result.clone().unwrap();
            let (new_json_array, new_columns) = JSONParser::change_depth_array(previous_parse_result, mem::take(&mut self.nodes), depth as usize).unwrap();
            self.all_columns = new_columns;
            let column_selected = Self::selected_columns(&self.all_columns, depth);
            self.column_selected = column_selected;
            self.nodes = new_json_array;
            self.last_parsed_max_depth = depth as usize;
        }
    }
    pub fn update_max_depth(&mut self, depth: u8) {
        self.max_depth = depth as usize;
        self.update_selected_columns(depth);
    }

    fn selected_columns(all_columns: &Vec<Column>, depth: u8) -> Vec<Column> {
        let mut column_selected: Vec<Column> = vec![];
        for col in Self::visible_columns(all_columns, depth) {
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
        all_columns.iter().filter(move |column: &&Column| column.depth == depth || (column.depth < depth && !matches!(column.value_type, ValueType::Object)))
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
        let mut click_on_array_row_index: Option<(usize, PointerKey)> = None;
        let table_scroll_output = table
            .header(text_height * 2.0, |mut header| {
                let clicked_column: RefCell<Option<String>> = RefCell::new(None);
                let pinned_column: RefCell<Option<usize>> = RefCell::new(None);
                let i: RefCell<usize> = RefCell::new(0);
                header.cols(true, |index| {
                    let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                    let column = columns.get(index).unwrap();
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
            .body(self.hovered_row_index, |body| {
                let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                let hovered_row_index = body.rows(text_height, self.nodes().len(), |mut row| {
                    let row_index = row.index();
                    let node = self.nodes().get(row_index);
                    if let Some(data) = node.as_ref() {
                        let response = row.cols(false, |index| {

                            let data = self.get_pointer(columns, &data.entries(), index, data.index());

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
                            let data = self.get_pointer(columns, &data.entries(), index, data.index());
                            if let Some((pointer, _value)) = data {
                                let is_array = matches!(pointer.value_type, ValueType::Array);
                                let is_object = matches!(pointer.value_type, ValueType::Object);
                                if is_array || is_object {
                                    click_on_array_row_index = Some((row_index, pointer.clone()));
                                } 
                            }
                        }
                    }
                });
                if self.hovered_row_index != hovered_row_index {
                    self.hovered_row_index = hovered_row_index;
                    request_repaint = true;
                }
            });

        if let Some((row_index, pointer))= click_on_array_row_index {
            let json_array_entries = &self.nodes()[row_index];
            if let Some((key, value)) = json_array_entries.find_node_at(pointer.pointer.as_str()) {
                let mut content = value.clone().unwrap();
                if matches!(pointer.value_type, ValueType::Object){
                    content = concat_string!("[", content, "]");
                }
                self.windows.push(SubTable::new(pointer.pointer, content,
                                                if matches!(pointer.value_type, ValueType::Array) { ValueType::Array } else { ValueType::Object }))
            } else {
                println!("can't find root at {} {}", row_index, pointer.pointer)
            }
        }

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

    fn get_pointer<'a>(&self, columns: &Vec<Column>, data: &&'a FlatJsonValue, index: usize, row_index: usize) -> Option<&'a (PointerKey, Option<String>)> {
        if let Some(column) = columns.get(index) {
            let key = &column.name;
            let key = concat_string!(self.parent_pointer, "/", row_index.to_string(), key);
            return data.iter().find(|(pointer, _)| {
                pointer.pointer.eq(&key)
            });
        }
        None
    }

    fn on_non_null_column_click(&mut self, column: String) {
        if self.non_null_columns.is_empty() {
            self.non_null_columns.push(column);
        } else if self.non_null_columns.contains(&column) {
            self.non_null_columns.retain(|c| !c.eq(&column));
        } else {
            self.non_null_columns.push(column);
        }
        if !self.non_null_columns.is_empty() {
            self.filtered_nodes = JSONParser::filter_non_null_column(&self.nodes, &self.parent_pointer, &self.non_null_columns);
        } else {
            self.filtered_nodes.clear();
        }

        self.next_frame_reset_scroll = true;
    }

    #[inline]
    fn nodes(&self) -> &Vec<JsonArrayEntries> {
        if self.non_null_columns.is_empty() {
            &self.nodes
        } else {
            &self.filtered_nodes
        }
    }
}
