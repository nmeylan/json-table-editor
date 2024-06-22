use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap};
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Sub;
use std::string::ToString;
use std::sync::Arc;
use std::time::{Duration, Instant};
use egui::{Align, Context, CursorIcon, Id, Key, Label, Sense, Style, TextBuffer, TextEdit, Ui, Vec2, Widget, WidgetText};
use egui::scroll_area::ScrollBarVisibility;
use egui::style::Spacing;
use indexmap::IndexSet;
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, JSONParser, ParseResult, PointerKey, ValueType};
use json_flat_parser::serializer::serialize_to_json_with_option;

use crate::{ArrayResponse, concat_string, Window};
use crate::components::icon;
use crate::components::popover::PopupMenu;
use crate::fonts::{FILTER, THUMBTACK};
use crate::parser::search_occurrences;
use crate::subtable_window::SubTable;

#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub depth: u8,
    pub value_type: ValueType,
    pub seen_count: usize,
    pub order: usize,
}

impl Hash for Column {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state)
    }
}

impl Column {
    pub fn new(name: String, value_type: ValueType) -> Self {
        Self {
            name,
            depth: 0,
            value_type,
            seen_count: 0,
            order: 0,
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
        match other.seen_count.cmp(&self.seen_count) {
            Ordering::Equal => other.order.cmp(&self.order),
            cmp => cmp,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScrollToRowMode {
    RowNumber,
    MatchingTerm,
}

impl Default for ScrollToRowMode {
    fn default() -> Self {
        ScrollToRowMode::RowNumber
    }
}

impl ScrollToRowMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RowNumber => "row number",
            Self::MatchingTerm => "matching term",
        }
    }
}

#[derive(Default)]
pub struct ArrayTable {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    column_pinned: Vec<Column>,
    max_depth: u8,
    last_parsed_max_depth: u8,
    parse_result: Option<ParseResult<String>>,
    pub nodes: Vec<JsonArrayEntries<String>>,
    filtered_nodes: Vec<usize>,
    scroll_y: f32,
    columns_filter: HashMap<String, Vec<String>>,
    pub hovered_row_index: Option<usize>,
    columns_offset: Vec<f32>,
    parent_pointer: String,
    parent_value_type: ValueType,
    windows: Vec<SubTable>,
    pub(crate) is_sub_table: bool,
    seed1: usize, // seed for Id
    seed2: usize, // seed for Id
    pub matching_rows: Vec<usize>,
    pub matching_row_selected: usize,
    pub scroll_to_column: String,
    pub scroll_to_row: String,
    pub scroll_to_row_mode: ScrollToRowMode,

    // Handle interaction
    pub next_frame_reset_scroll: bool,
    pub changed_scroll_to_column_value: bool,
    pub changed_matching_row_selected: bool,
    pub changed_scroll_to_row_value: Option<Instant>,

    pub editing_index: RefCell<Option<(usize, usize, bool)>>,
    pub editing_value: RefCell<String>,
}


impl super::View<ArrayResponse> for ArrayTable {
    fn ui(&mut self, ui: &mut egui::Ui) -> ArrayResponse {
        use egui_extras::{Size, StripBuilder};
        self.windows(ui.ctx());
        let mut array_response = ArrayResponse::default();
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
                            if self.changed_scroll_to_column_value {
                                self.changed_scroll_to_column_value = false;
                                let mut index = self.column_selected.iter().position(|c| {
                                    c.name.to_lowercase().eq(&concat_string!("/", &self.scroll_to_column.to_lowercase()))
                                });
                                if index.is_none() {
                                    index = self.column_selected.iter().position(|c| {
                                        c.name.to_lowercase().contains(&self.scroll_to_column.to_lowercase())
                                    });
                                }
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
                            scroll_area.show(ui, |ui| {
                                array_response = self.table_ui(ui, false);
                            });
                        });
                    });
                });
            });
        array_response
    }
}

#[derive(Default)]
struct CacheA {}

type ColumnFilterCache = egui::util::cache::FrameCache<IndexSet<String>, CacheA>;

impl egui::util::cache::ComputerMut<(&Column, &Vec<JsonArrayEntries<String>>, &String), IndexSet<String>> for CacheA {
    fn compute(&mut self, (column, nodes, parent_pointer): (&Column, &Vec<JsonArrayEntries<String>>, &String)) -> IndexSet<String> {
        let mut unique_values = IndexSet::new();
        if matches!(column.value_type, ValueType::String) {
            nodes.iter().enumerate().map(|(i, row)| {
                ArrayTable::get_pointer_for_column(parent_pointer, &&row.entries, i, column).filter(|entry| entry.value.is_some()).map(|entry| entry.value.clone().unwrap())
            }).for_each(|value| {
                if let Some(value) = value {
                    unique_values.insert(value);
                }
            })
        }
        unique_values
    }
}

pub const NON_NULL_FILTER_VALUE: &'static str = "__non_null";

impl ArrayTable {
    pub fn new(parse_result: Option<ParseResult<String>>, nodes: Vec<JsonArrayEntries<String>>, all_columns: Vec<Column>, depth: u8, parent_pointer: String, parent_value_type: ValueType) -> Self {
        let last_parsed_max_depth = parse_result.as_ref().map_or(depth, |p| p.parsing_max_depth);
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            max_depth: depth,
            filtered_nodes: (0..nodes.len()).collect::<Vec<usize>>(),
            nodes,
            parse_result,
            // states
            next_frame_reset_scroll: false,
            column_pinned: vec![Column::new("/#".to_string(), ValueType::Number)],
            scroll_y: 0.0,
            hovered_row_index: None,
            columns_offset: vec![],
            seed1: Id::new(&parent_pointer).value() as usize,
            seed2: Id::new(format!("{}pinned", &parent_pointer)).value() as usize,
            parent_pointer,
            parent_value_type,
            windows: vec![],
            matching_rows: vec![],
            matching_row_selected: 0,
            scroll_to_column: "".to_string(),
            changed_scroll_to_column_value: false,
            last_parsed_max_depth,
            columns_filter: HashMap::new(),
            scroll_to_row_mode: ScrollToRowMode::RowNumber,
            scroll_to_row: "".to_string(),
            changed_scroll_to_row_value: None,
            changed_matching_row_selected: false,
            editing_index: RefCell::new(None),
            editing_value: RefCell::new(String::new()),
            is_sub_table: false,
        }
    }
    pub fn windows(&mut self, ctx: &Context) {
        let mut closed_windows = vec![];
        let mut updated_values = vec![];
        for window in self.windows.iter_mut() {
            let mut opened = true;
            let maybe_response = window.show(ctx, &mut opened);
            if let Some(maybe_inner_response) = maybe_response {
                if let Some(response) = maybe_inner_response {
                    if let Some(entry) = response.edited_value {
                        updated_values.push((entry, window.id(), false));
                    }
                }
            }
            if !opened {
                closed_windows.push(window.name().clone());
            }
        }
        for updated_value in updated_values {
            self.update_value(updated_value.0, updated_value.1, updated_value.2);
        }
        self.windows.retain(|w| !closed_windows.contains(w.name()));
    }

    pub fn update_selected_columns(&mut self, depth: u8) -> Option<usize> {
        if depth <= self.last_parsed_max_depth as u8 {
            let mut column_selected = Self::selected_columns(&self.all_columns, depth);
            column_selected.retain(|c| !self.column_pinned.contains(c));
            self.column_selected = column_selected;
            if self.column_selected.is_empty() {
                self.column_selected.push(Column {
                    name: "".to_string(),
                    depth,
                    value_type: Default::default(),
                    seen_count: 0,
                    order: 0,
                })
            }
            None
        } else {
            let previous_parse_result = self.parse_result.clone().unwrap();
            let (new_json_array, new_columns, new_max_depth) = crate::parser::change_depth_array(previous_parse_result, mem::take(&mut self.nodes), depth as usize).unwrap();
            self.all_columns = new_columns;
            let mut column_selected = Self::selected_columns(&self.all_columns, depth);
            column_selected.retain(|c| !self.column_pinned.contains(c));
            self.column_selected = column_selected;
            self.nodes = new_json_array;
            self.last_parsed_max_depth = depth;
            self.parse_result.as_mut().unwrap().max_json_depth = new_max_depth;
            Some(new_max_depth)
        }
    }
    pub fn update_max_depth(&mut self, depth: u8) -> Option<usize> {
        self.max_depth = depth;
        self.update_selected_columns(depth)
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
        all_columns.iter().filter(move |column: &&Column| column.depth == depth || (column.depth < depth && !matches!(column.value_type, ValueType::Object(_))))
    }

    fn table_ui(&mut self, ui: &mut egui::Ui, pinned: bool) -> ArrayResponse {
        let text_height = Self::row_height(ui.style(), ui.spacing());

        self.draw_table(ui, text_height, 7.0, pinned)
    }

    pub fn row_height(style: &Arc<Style>, spacing: &Spacing) -> f32 {
        let text_height = egui::TextStyle::Body
            .resolve(style)
            .size
            .max(spacing.interact_size.y);
        text_height
    }
    fn draw_table(&mut self, ui: &mut Ui, text_height: f32, text_width: f32, pinned_column_table: bool) -> ArrayResponse {
        use crate::components::table::{Column, TableBuilder};
        let parent_height = ui.available_rect_before_wrap().height();
        let mut array_response = ArrayResponse::default();
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
            table = table.scroll_to_row(0, Some(Align::Center));
            self.next_frame_reset_scroll = false;
        }
        if let Some(changed_scroll_to_row_value) = self.changed_scroll_to_row_value {
            match self.scroll_to_row_mode {
                ScrollToRowMode::RowNumber => {
                    self.changed_scroll_to_row_value = None;
                    table = table.scroll_to_row(self.scroll_to_row.parse::<usize>().unwrap_or_else(|_| {
                        self.scroll_to_row.clear();
                        0
                    }), Some(Align::Center));
                }
                ScrollToRowMode::MatchingTerm => {
                    if changed_scroll_to_row_value.elapsed().as_millis() >= 300 {
                        self.changed_scroll_to_row_value = None;
                        if !self.scroll_to_row.is_empty() {
                            self.matching_rows = search_occurrences(&self.nodes, &self.scroll_to_row.to_lowercase());
                            self.matching_row_selected = 0;
                            if !self.matching_rows.is_empty() {
                                self.changed_matching_row_selected = true;
                            }
                        }
                    }
                }
            }
        }
        if self.changed_matching_row_selected {
            self.changed_matching_row_selected = false;
            table = table.scroll_to_row(self.matching_rows[self.matching_row_selected], Some(Align::Center));
        }
        table = table.vertical_scroll_offset(self.scroll_y);

        let columns_count = if pinned_column_table { self.column_pinned.len() } else { self.column_selected.len() };
        let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
        for i in 0..columns_count {
            if pinned_column_table && i == 0 {
                table = table.column(Column::initial(40.0).clip(true).resizable(true));
                continue;
            }
            table = table.column(Column::initial((columns[i].name.len() + 3).max(10) as f32 * text_width).clip(true).resizable(true));
        }
        let mut request_repaint = false;
        let search_highlight_row = if !self.matching_rows.is_empty() {
            Some(self.matching_rows[self.matching_row_selected])
        } else {
            None
        };
        let table_scroll_output = table
            .header(text_height * 2.0, |mut header| {
                // Mutation after interaction
                let mut clicked_filter_non_null_column: Option<String> = None;
                let mut clicked_filter_column_value: Option<(String, String)> = None;
                let mut pinned_column: Option<usize> = None;
                header.cols(true, |ui, index| {
                    let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                    let column = columns.get(index).unwrap();
                    let name = format!("{}", column.name.clone());
                    let strong = Label::new(WidgetText::RichText(egui::RichText::from(&name)));
                    let label = Label::new(&name);
                    let response = ui.vertical(|ui| {
                        let response = ui.add(strong).on_hover_ui(|ui| { ui.add(label); });

                        if !pinned_column_table || index > 0 {
                            ui.horizontal(|ui| {
                                if column.name.eq("") {
                                    return;
                                }
                                let response = icon::button(ui, THUMBTACK);
                                if response.clicked() {
                                    pinned_column = Some(index);
                                }
                                let column_id = Id::new(&name);
                                PopupMenu::new(column_id.with("filter"))
                                    .show_ui(ui, |ui| icon::button(ui, FILTER),
                                             |ui| {
                                                 let mut checked_filtered_values = self.columns_filter.get(&column.name);
                                                 let mut chcked = if let Some(filters) = checked_filtered_values {
                                                     filters.contains(&NON_NULL_FILTER_VALUE.to_owned())
                                                 } else {
                                                     false
                                                 };
                                                 if ui.checkbox(&mut chcked, "Non null").clicked() {
                                                     clicked_filter_non_null_column = Some(name);
                                                 }

                                                 if matches!(column.value_type, ValueType::String) {
                                                     let values = ui.memory_mut(|mem| {
                                                         let cache = mem.caches.cache::<ColumnFilterCache>();
                                                         let values = cache.get((column, &self.nodes, &self.parent_pointer));
                                                         values
                                                     });
                                                     if values.len() > 0 {
                                                         let mut checked_filtered_values = self.columns_filter.get(&column.name);
                                                         ui.separator();
                                                         values.iter().for_each(|value| {
                                                             let mut chcked = if let Some(filters) = checked_filtered_values {
                                                                 filters.contains(value)
                                                             } else {
                                                                 false
                                                             };
                                                             if ui.checkbox(&mut chcked, value).clicked() {
                                                                 clicked_filter_column_value = Some((column.name.clone(), value.clone()));
                                                             }
                                                         });
                                                     }
                                                 }
                                             });
                            });
                        }

                        response
                    });
                    Some(response.inner)
                });


                if let Some(pinned_column) = pinned_column {
                    if pinned_column_table {
                        let column = self.column_pinned.remove(pinned_column);
                        self.column_selected.push(column);
                        self.column_selected.sort();
                    } else {
                        let column = self.column_selected.remove(pinned_column);
                        self.column_pinned.push(column);
                    }
                }
                if let Some(clicked_column) = clicked_filter_non_null_column {
                    self.on_filter_column_value((clicked_column, NON_NULL_FILTER_VALUE.to_string()));
                }
                if let Some(clicked_column) = clicked_filter_column_value {
                    self.on_filter_column_value(clicked_column.clone());
                }
            })
            .body(self.hovered_row_index, search_highlight_row, |body| {
                // Mutation after interaction
                let mut subtable = None;
                let mut updated_value: Option<(PointerKey, String)> = None;
                let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                let hovered_row_index = body.rows(text_height, self.filtered_nodes.len(), |mut row| {
                    let row_index = self.filtered_nodes[row.index()];
                    let node = self.nodes().get(row_index);

                    if let Some(data) = node.as_ref() {
                        row.cols(false, |ui, col_index| {
                            let cell_id = row_index * columns.len() + col_index + if pinned_column_table { self.seed1 } else { self.seed2 };
                            let data = self.get_pointer(columns, &data.entries(), col_index, data.index());
                            let mut editing_index = self.editing_index.borrow_mut();
                            if editing_index.is_some() && editing_index.unwrap() == (col_index, row_index, pinned_column_table) {
                                let ref_mut = &mut *self.editing_value.borrow_mut();
                                let textedit_response = ui.add(TextEdit::singleline(ref_mut));
                                if textedit_response.lost_focus() || ui.ctx().input(|input| input.key_pressed(Key::Enter)) {
                                    let pointer = PointerKey {
                                        pointer: Self::pointer_key(&self.parent_pointer, row_index, &columns.get(col_index).as_ref().unwrap().name),
                                        value_type: columns[col_index].value_type,
                                        depth: columns[col_index].depth,
                                        index: row_index,
                                        position: 0,
                                    };
                                    updated_value = Some((pointer, mem::take(ref_mut)))
                                } else {
                                    textedit_response.request_focus();
                                }
                            } else if let Some(entry) = data {
                                let is_array = matches!(entry.pointer.value_type, ValueType::Array(_));
                                let is_object = matches!(entry.pointer.value_type, ValueType::Object(_));
                                if pinned_column_table && col_index == 0 {
                                    let label = Label::new(entry.pointer.index.to_string()).sense(Sense::click());
                                    return Some(label.ui(ui));
                                } else if let Some(value) = entry.value.as_ref() {
                                    if !matches!(entry.pointer.value_type, ValueType::Null) {
                                        let mut label = if is_array || is_object {
                                            Label::new(value.replace("\n", "")) // maybe we want cache
                                        } else {
                                            Label::new(value)
                                        };

                                        let rect = ui.available_rect_before_wrap();
                                        let cell_zone = ui.interact(rect, Id::new(cell_id), Sense::click());

                                        label = label.sense(Sense::click());
                                        let response = label.ui(ui);
                                        if cell_zone.clicked() || response.clicked() {
                                            let is_array = matches!(entry.pointer.value_type, ValueType::Array(_));
                                            let is_object = matches!(entry.pointer.value_type, ValueType::Object(_));
                                            if is_array || is_object {
                                                let content = value.clone();
                                                subtable = Some(SubTable::new(entry.pointer.pointer.clone(), content,
                                                                              if matches!(entry.pointer.value_type, ValueType::Array(_)) { ValueType::Array(0) } else { ValueType::Object(true) },
                                                                              row_index, entry.pointer.depth,
                                                ));
                                            } else {
                                                *self.editing_value.borrow_mut() = value.clone();
                                                *editing_index = Some((col_index, row_index, pinned_column_table));
                                            }
                                        }
                                        if cell_zone.hovered() || response.hovered() {
                                            if matches!(entry.pointer.value_type, ValueType::Array(_)) || matches!(entry.pointer.value_type, ValueType::Object(_)) {
                                                ui.ctx().set_cursor_icon(CursorIcon::ZoomIn);
                                            }
                                        }
                                        return Some(response.union(cell_zone));
                                    }
                                } else {
                                    let rect = ui.available_rect_before_wrap();
                                    let cell_zone = ui.interact(rect, Id::new(&entry.pointer.pointer), Sense::click());
                                    if cell_zone.clicked() {
                                        *self.editing_value.borrow_mut() = String::new();
                                        *editing_index = Some((col_index, row_index, pinned_column_table));
                                    }
                                }
                            } else {
                                let rect = ui.available_rect_before_wrap();
                                let cell_zone = ui.interact(rect, Id::new(cell_id), Sense::click());
                                if cell_zone.clicked() {
                                    *self.editing_value.borrow_mut() = String::new();
                                    *editing_index = Some((col_index, row_index, pinned_column_table));
                                }
                                return Some(cell_zone);
                            }
                            None
                        });
                    }
                });
                if let Some(subtable) = subtable {
                    self.windows.push(subtable);
                }
                if let Some((pointer, value)) = updated_value {
                    let editing_index = mem::take(&mut *self.editing_index.borrow_mut());
                    let value = if value.is_empty() {
                        None
                    } else {
                        Some(value)
                    };
                    if self.is_sub_table {
                        array_response.edited_value = Some(FlatJsonValue { pointer: pointer.clone(), value: value.clone() });
                    }
                    let (_, row_index, _) = editing_index.unwrap();
                    self.update_value(FlatJsonValue { pointer, value }, row_index, true);
                }
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
        array_response
    }

    fn update_value(&mut self, updated_entry: FlatJsonValue<String>, row_index: usize, should_update_subtable: bool) {
        if should_update_subtable {
            for subtable in self.windows.iter_mut() {
                if subtable.id() == row_index {
                    subtable.update_nodes(updated_entry.pointer.clone(), updated_entry.value.clone());
                    break;
                }
            }
        }

        if let Some(entry) = self.nodes[row_index].entries.iter_mut().find(|entry| entry.pointer.pointer.eq(&updated_entry.pointer.pointer)) {
            entry.value = updated_entry.value;
        } else {
            self.nodes[row_index].entries.push(FlatJsonValue::<String> { pointer: updated_entry.pointer, value: updated_entry.value });
        }
        if !self.is_sub_table {
            let root_node = self.nodes[row_index].entries.pop().unwrap();
            let value1 = serialize_to_json_with_option::<String>(
                &mut self.nodes[row_index].entries.clone(),
                root_node.pointer.depth + 1);
            self.nodes[row_index].entries.push(FlatJsonValue { pointer: root_node.pointer, value: Some(value1.to_json()) });
        }
    }

    #[inline]
    fn get_pointer<'a>(&self, columns: &Vec<Column>, data: &&'a Vec<FlatJsonValue<String>>, index: usize, row_index: usize) -> Option<&'a FlatJsonValue<String>> {
        if let Some(column) = columns.get(index) {
            return Self::get_pointer_for_column(&self.parent_pointer, data, row_index, column);
        }
        None
    }

    #[inline]
    fn get_pointer_for_column<'a>(parent_pointer: &String, data: &&'a Vec<FlatJsonValue<String>>, row_index: usize, column: &Column) -> Option<&'a FlatJsonValue<String>> {
        let key = &column.name;
        let key = Self::pointer_key(parent_pointer, row_index, key);
        return data.iter().find(|entry| {
            entry.pointer.pointer.eq(&key)
        });
    }

    #[inline]
    fn pointer_key(parent_pointer: &String, row_index: usize, key: &String) -> String {
        concat_string!(parent_pointer, "/", row_index.to_string(), key)
    }


    fn on_filter_column_value(&mut self, (column, value): (String, String)) {
        let maybe_filter = self.columns_filter.get_mut(&column);
        if let Some(filter) = maybe_filter {
            if filter.contains(&value) {
                filter.retain(|v| !v.eq(&value));
                if filter.is_empty() {
                    self.columns_filter.remove(&column);
                }
            } else {
                filter.push(value);
            }
        } else {
            self.columns_filter.insert(column, vec![value]);
        }
        if self.columns_filter.is_empty() {
            self.filtered_nodes = (0..self.nodes.len()).collect::<Vec<usize>>();
        } else {
            self.filtered_nodes = crate::parser::filter_columns(&self.nodes, &self.parent_pointer, &self.columns_filter);
        }
        self.next_frame_reset_scroll = true;
    }

    #[inline]
    fn nodes(&self) -> &Vec<JsonArrayEntries<String>> {
        &self.nodes
    }

    pub fn reset_search(&mut self) {
        self.scroll_to_row.clear();
        self.matching_rows.clear();
        self.changed_scroll_to_row_value = Some(Instant::now().sub(Duration::from_millis(1000)));
        self.matching_row_selected = 0;
    }
}
