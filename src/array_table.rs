use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap};
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Sub;
use std::string::ToString;
use std::sync::Arc;
use std::time::{Duration, Instant};
use egui::{Align, Context, CursorIcon, Id, Key, Label, Sense, Style, TextEdit, Ui, Vec2, Widget, WidgetText};
use egui::scroll_area::ScrollBarVisibility;
use egui::style::Spacing;
use egui::util::cache;
use indexmap::IndexSet;
use json_flat_parser::{FlatJsonValue, JsonArrayEntries, JSONParser, ParseOptions, ParseResult, PointerKey, ValueType};
use json_flat_parser::serializer::serialize_to_json_with_option;


use crate::{ACTIVE_COLOR, ArrayResponse, concat_string};
use crate::components::icon;
use crate::components::popover::PopupMenu;
use crate::components::table::{CellLocation, TableBody, TableRow};
use crate::fonts::{FILTER, THUMBTACK};
use crate::parser::{search_occurrences};
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
#[derive(Default)]
pub enum ScrollToRowMode {
    #[default]
    RowNumber,
    MatchingTerm,
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
    pub max_depth: u8,
    last_parsed_max_depth: u8,
    parse_result: Option<ParseResult<String>>,
    pub nodes: Vec<JsonArrayEntries<String>>,
    filtered_nodes: Vec<usize>,
    scroll_y: f32,
    pub columns_filter: HashMap<String, Vec<String>>,
    pub hovered_row_index: Option<usize>,
    columns_offset: Vec<f32>,
    pub parent_pointer: String,
    windows: Vec<SubTable>,
    pub(crate) is_sub_table: bool,
    cache: RefCell<crate::components::cache::CacheStorage>,
    seed1: usize, // seed for Id
    seed2: usize, // seed for Id
    pub matching_rows: Vec<usize>,
    pub matching_row_selected: usize,
    pub matching_columns: Vec<usize>,
    pub matching_column_selected: usize,
    pub scroll_to_column: String,
    pub scroll_to_row: String,
    pub scroll_to_row_mode: ScrollToRowMode,
    pub focused_cell: Option<CellLocation>,

    // Handle interaction
    pub next_frame_reset_scroll: bool,
    pub changed_scroll_to_column_value: bool,
    pub changed_matching_column_selected: bool,
    pub changed_matching_row_selected: bool,

    #[cfg(not(target_arch = "wasm32"))]
    pub changed_scroll_to_row_value: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    pub changed_scroll_to_row_value: Option<crate::compatibility::InstantWrapper>,

    pub editing_index: RefCell<Option<(usize, usize, bool)>>,
    pub editing_value: RefCell<String>,
}


impl super::View<ArrayResponse> for ArrayTable {
    fn ui(&mut self, ui: &mut egui::Ui) -> ArrayResponse {
        use egui_extras::{Size, StripBuilder};
        let mut array_response = ArrayResponse::default();
        self.windows(ui.ctx(), &mut array_response);
        StripBuilder::new(ui)
            .size(Size::remainder())
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    let parent_height_available = ui.available_rect_before_wrap().height();
                    let parent_width_available = ui.available_rect_before_wrap().width();
                    ui.horizontal(|ui| {
                        ui.set_height(parent_height_available);
                        ui.push_id("table-pinned-column", |ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width(parent_width_available / 2.0);
                                let scroll_area = egui::ScrollArea::horizontal();
                                scroll_area.show(ui, |ui| {
                                    array_response = array_response.union(self.table_ui(ui, true));
                                });
                            })
                        });

                        ui.vertical(|ui| {
                            let mut scroll_to_x = None;
                            if self.changed_scroll_to_column_value {
                                self.changed_scroll_to_column_value = false;
                                self.changed_matching_column_selected = true;
                                self.matching_columns.clear();
                                self.matching_column_selected = 0;
                                if !self.scroll_to_column.is_empty() {
                                    for (index, column) in self.column_selected.iter().enumerate() {
                                        if column.name.to_lowercase().eq(&concat_string!("/", &self.scroll_to_column.to_lowercase()))
                                            || column.name.to_lowercase().contains(&self.scroll_to_column.to_lowercase()) {
                                            self.matching_columns.push(index);
                                        }
                                    }
                                }
                            }

                            if self.changed_matching_column_selected {
                                self.changed_matching_column_selected = false;
                                if !self.matching_columns.is_empty() {
                                    if let Some(offset) = self.columns_offset.get(self.matching_columns[self.matching_column_selected]) {
                                        scroll_to_x = Some(*offset);
                                    }
                                }
                            }

                            let mut scroll_area = egui::ScrollArea::horizontal();
                            if let Some(offset) = scroll_to_x {
                                scroll_area = scroll_area.scroll_offset(Vec2 { x: offset, y: 0.0 });
                            }
                            scroll_area.show(ui, |ui| {
                                array_response = array_response.union(self.table_ui(ui, false));
                            });
                        });
                    });
                });
            });
        self.cache.borrow_mut().update();

        let mut copied_value = None;
        if self.editing_index.borrow().is_none() {
            ui.input_mut(|i| {
                for event in i.events.iter().filter(|e| match e {
                    egui::Event::Copy => array_response.hover_data.hovered_cell.is_some(),
                    egui::Event::Paste(_) => array_response.hover_data.hovered_cell.is_some(),
                    _ => false,
                }) {
                    let cell_location = array_response.hover_data.hovered_cell.unwrap();
                    let row_index = self.filtered_nodes[cell_location.row_index];
                    let index = self.get_pointer_index_from_cache(cell_location.is_pinned_column_table, &&self.nodes[row_index], cell_location.column_index);

                    match event {
                        egui::Event::Paste(v) => {
                            let columns = self.columns(cell_location.is_pinned_column_table);
                            let pointer = Self::pointer_key(&self.parent_pointer, row_index, &columns.get(cell_location.column_index).as_ref().unwrap().name);
                            let flat_json_value = FlatJsonValue::<String> {
                                pointer: PointerKey {
                                    pointer,
                                    value_type: columns[cell_location.column_index].value_type,
                                    depth: columns[cell_location.column_index].depth,
                                    index: row_index,
                                    position: 0,
                                },
                                value: Some(v.clone()),
                            };
                            self.update_value(flat_json_value, row_index, !self.is_sub_table);
                        }
                        egui::Event::Copy => {
                            if let Some(index) = index {
                                if let Some(value) = &self.nodes[row_index].entries()[index].value {
                                    copied_value = Some(value.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });
            if let Some(value) = copied_value {
                ui.ctx().copy_text(value.clone());
            }
        }
        array_response
    }
}

#[derive(Default)]
struct CacheFilterOptions {}

#[derive(Default)]
struct CacheGetPointer {}

#[derive(Copy, Clone)]
struct CachePointerKey {
    pinned_column_table: bool,
    index: usize,
    row_index: usize,
}

impl Hash for CachePointerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pinned_column_table.hash(state);
        self.index.hash(state);
        self.row_index.hash(state);
    }
}

impl crate::components::cache::ComputerMut<(&Column, &String), &Vec<JsonArrayEntries<String>>, IndexSet<String>> for CacheFilterOptions {
    fn compute(&mut self, (column, parent_pointer): (&Column, &String), nodes: &Vec<JsonArrayEntries<String>>) -> IndexSet<String> {
        let mut unique_values = IndexSet::new();
        if ArrayTable::is_filterable(column) {
            nodes.iter().enumerate().map(|(i, row)| {
                ArrayTable::get_pointer_for_column(parent_pointer, &&row.entries, i, column).filter(|entry| entry.value.is_some()).map(|entry| entry.value.clone().unwrap())
            }).for_each(|value| {
                if let Some(value) = value {
                    unique_values.insert(value);
                }
            })
        }
        if matches!(column.value_type, ValueType::Number) {
            unique_values.sort_by(|a, b| {
                let num_a = a.parse::<f64>();
                let num_b = b.parse::<f64>();

                // Compare parsed numbers; handle parse errors by pushing them to the end
                match (num_a, num_b) {
                    (Ok(a), Ok(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less, // Numbers are less than errors
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater, // Errors are greater than numbers
                    (Err(_), Err(_)) => std::cmp::Ordering::Equal, // Treat errors equally
                }
            });
        } else {
            unique_values.sort_by(|a, b| a.cmp(b));
        }
        unique_values
    }
}


impl crate::components::cache::ComputerMut<CachePointerKey, &ArrayTable, Option<usize>> for CacheGetPointer {
    fn compute(&mut self, cache_pointer_key: CachePointerKey, table: &ArrayTable) -> Option<usize> {
        let columns = if cache_pointer_key.pinned_column_table { &table.column_pinned } else { &table.column_selected };
        ArrayTable::get_pointer_index(&table.parent_pointer, columns, &table.nodes()[cache_pointer_key.row_index].entries(), cache_pointer_key.index, cache_pointer_key.row_index)
    }
}

pub const NON_NULL_FILTER_VALUE: &str = "__non_null";

impl ArrayTable {
    pub fn new(parse_result: Option<ParseResult<String>>, nodes: Vec<JsonArrayEntries<String>>, all_columns: Vec<Column>, depth: u8, parent_pointer: String) -> Self {
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
            windows: vec![],
            matching_rows: vec![],
            matching_row_selected: 0,
            matching_columns: vec![],
            matching_column_selected: 0,
            scroll_to_column: "".to_string(),
            changed_scroll_to_column_value: false,
            last_parsed_max_depth,
            columns_filter: HashMap::new(),
            scroll_to_row_mode: ScrollToRowMode::RowNumber,
            scroll_to_row: "".to_string(),
            changed_scroll_to_row_value: None,
            changed_matching_row_selected: false,
            changed_matching_column_selected: false,
            editing_index: RefCell::new(None),
            editing_value: RefCell::new(String::new()),
            is_sub_table: false,
            focused_cell: None,
            cache: Default::default(),
        }
    }
    pub fn windows(&mut self, ctx: &Context, array_response: &mut ArrayResponse) {
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
            if self.update_value(updated_value.0.clone(), updated_value.1, updated_value.2) {
                array_response.edited_value = Some(updated_value.0.clone())
            }
        }
        self.windows.retain(|w| !closed_windows.contains(w.name()));
    }

    pub fn update_selected_columns(&mut self, depth: u8) -> Option<usize> {
        self.cache.borrow_mut().update();
        if depth <= self.last_parsed_max_depth {
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
            self.parse_result.as_mut().unwrap().parsing_max_depth = depth;
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
        egui::TextStyle::Body
            .resolve(style)
            .size
            .max(spacing.interact_size.y)
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
            .set_is_pinned_column_table(pinned_column_table)
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
                    }), Some(Align::TOP));
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
        let columns = self.columns(pinned_column_table);
        if columns_count <= 3 {
            for i in 0..columns_count {
                if pinned_column_table && i == 0 {
                    table = table.column(Column::initial(40.0).clip(true).resizable(true));
                } else {
                    table = table.column(Column::auto().clip(true).resizable(true));
                }
            }
        } else {
            for i in 0..columns_count {
                if pinned_column_table && i == 0 {
                    table = table.column(Column::initial(40.0).clip(true).resizable(true));
                    continue;
                }
                // table = table.column(Column::initial(10.0).clip(true).resizable(true));
                table = table.column(Column::initial((columns[i].name.len() + 3).max(10) as f32 * text_width).clip(true).resizable(true));
            }
        }

        let mut request_repaint = false;
        let search_highlight_row = if !self.matching_rows.is_empty() {
            Some(self.matching_rows[self.matching_row_selected])
        } else {
            None
        };
        let table_scroll_output = table
            .header(text_height * 2.0, |mut header| {
                self.header(pinned_column_table, header);
            })
            .body(self.hovered_row_index, search_highlight_row, self.focused_cell, |body| {
                self.body(text_height, pinned_column_table, &mut array_response, request_repaint, body);
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

    fn header(&mut self, pinned_column_table: bool, mut header: TableRow) {
        // Mutation after interaction
        let mut clicked_filter_non_null_column: Option<String> = None;
        let mut clicked_filter_column_value: Option<(String, String)> = None;
        let mut pinned_column: Option<usize> = None;
        header.cols(true, |ui, index| {
            let columns = self.columns(pinned_column_table);
            let column = columns.get(index).unwrap();
            let name = column.name.clone().to_string();
            let strong = Label::new(WidgetText::RichText(egui::RichText::from(&name)));
            let label = Label::new(&name);
            let response = ui.vertical(|ui| {
                let response = ui.add(strong).on_hover_ui(|ui| { ui.add(label); });

                if !pinned_column_table || index > 0 {
                    ui.horizontal(|ui| {
                        if column.name.eq("") {
                            return;
                        }
                        let response = icon::button(ui, THUMBTACK, Some(if pinned_column_table { "Unpin column" } else { "Pin column to left" }), None);
                        if response.clicked() {
                            pinned_column = Some(index);
                        }
                        let column_id = Id::new(&name);
                        let checked_filtered_values = self.columns_filter.get(&column.name);
                        PopupMenu::new(column_id.with("filter"))
                            .show_ui(ui, |ui| icon::button(ui, FILTER, None, if checked_filtered_values.is_some() { Some(ACTIVE_COLOR) } else { None }),
                                     |ui| {
                                         let mut chcked = if let Some(filters) = checked_filtered_values {
                                             filters.contains(&NON_NULL_FILTER_VALUE.to_owned())
                                         } else {
                                             false
                                         };
                                         if ui.checkbox(&mut chcked, "Non null").clicked() {
                                             clicked_filter_non_null_column = Some(name);
                                         }

                                         if Self::is_filterable(column) {
                                             let mut cache_ref_mut = self.cache.borrow_mut();
                                             let cache = cache_ref_mut.cache::<crate::components::cache::FrameCache<IndexSet<String>, CacheFilterOptions>>();

                                             let values = cache.get((column, &self.parent_pointer), &self.nodes);
                                             if !values.is_empty() {
                                                 let checked_filtered_values = self.columns_filter.get(&column.name);
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
            self.cache.borrow_mut().evict();
        }
        if let Some(clicked_column) = clicked_filter_non_null_column {
            self.on_filter_column_value((clicked_column, NON_NULL_FILTER_VALUE.to_string()));
        }
        if let Some(clicked_column) = clicked_filter_column_value {
            self.on_filter_column_value(clicked_column.clone());
        }
    }


    fn body<'arraytable>(&'arraytable mut self, text_height: f32, pinned_column_table: bool, mut array_response: &mut ArrayResponse, mut request_repaint: bool, body: TableBody) {
        // Mutation after interaction
        let mut subtable = None;
        let mut focused_cell = None;
        let mut focused_changed = false;
        let mut updated_value: Option<(PointerKey, String)> = None;
        let mut filter_by_value: Option<(String, String)> = None;
        let columns = self.columns(pinned_column_table);
        let hover_data = body.rows(text_height, self.filtered_nodes.len(), |mut row| {
            let table_row_index = row.index();
            let row_index = self.filtered_nodes[table_row_index];
            let node = self.nodes().get(row_index);

            if let Some(row_data) = node.as_ref() {
                row.cols(false, |ui, col_index| {
                    let cell_id = row_index * columns.len() + col_index + if pinned_column_table { self.seed1 } else { self.seed2 };
                    let index = self.get_pointer_index_from_cache(pinned_column_table, row_data, col_index);
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
                    } else if let Some(index) = index {
                        let entry = &row_data.entries()[index];
                        let is_array = matches!(entry.pointer.value_type, ValueType::Array(_));
                        let is_object = matches!(entry.pointer.value_type, ValueType::Object(_));
                        if pinned_column_table && col_index == 0 {
                            let label = Label::new(entry.pointer.index.to_string()).sense(Sense::click());
                            return Some(label.ui(ui));
                        } else if let Some(value) = entry.value.as_ref() {
                            if !matches!(entry.pointer.value_type, ValueType::Null) {
                                let mut label = if is_array || is_object {
                                    Label::new(value.replace('\n', "")) // maybe we want cache
                                } else {
                                    Label::new(value)
                                };

                                let rect = ui.available_rect_before_wrap();
                                let cell_zone = ui.interact(rect, Id::new(cell_id), Sense::click());

                                label = label.sense(Sense::click());
                                let mut response = cell_zone.union(label.ui(ui));

                                let is_array = matches!(entry.pointer.value_type, ValueType::Array(_));
                                let is_object = matches!(entry.pointer.value_type, ValueType::Object(_));

                                if response.double_clicked() {
                                    *self.editing_value.borrow_mut() = value.clone();
                                    *editing_index = Some((col_index, row_index, pinned_column_table));
                                }
                                if response.secondary_clicked() {
                                    focused_cell = Some(CellLocation { column_index: col_index, row_index: table_row_index, is_pinned_column_table: pinned_column_table });
                                    focused_changed = true;
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Edit").clicked() {
                                        *self.editing_value.borrow_mut() = value.clone();
                                        *editing_index = Some((col_index, row_index, pinned_column_table));
                                        ui.close_menu();
                                    }
                                    if ui.button("Copy").clicked() {
                                        ui.ctx().copy_text(value.clone());
                                        ui.close_menu();
                                    }
                                    if Self::is_filterable(&columns[col_index]) {
                                        if ui.button("Filter by this value").clicked() {
                                            filter_by_value = Some((columns[col_index].name.clone(), value.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                    if is_array || is_object {
                                        ui.separator();
                                        if ui.button(format!("Open {} in sub table", if is_array { "array" } else { "object" })).clicked() {
                                            ui.close_menu();
                                            let content = value.clone();
                                            subtable = Self::open_subtable(row_index, entry, content);
                                        }
                                    }
                                    if !self.is_sub_table {
                                        ui.separator();
                                        if ui.button("Open row in sub table".to_string()).clicked() {
                                            ui.close_menu();
                                            let root_node = row_data.entries.last().unwrap();
                                            subtable = Some(SubTable::new(root_node.pointer.pointer.clone(),
                                                                          root_node.value.as_ref().unwrap().clone(),
                                                                          ValueType::Object(true),
                                                                          row_index, root_node.pointer.depth,
                                            ));
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("Copy pointer").clicked() {
                                        ui.ctx().copy_text(entry.pointer.pointer.clone());
                                        ui.close_menu();
                                    }
                                });

                                if let Some(focused_cell_location) = self.focused_cell {
                                    if focused_cell_location.is_pinned_column_table == pinned_column_table && focused_cell_location.row_index == table_row_index && focused_cell_location.column_index == col_index && !response.context_menu_opened() {
                                        focused_cell = None;
                                        focused_changed = true;
                                    }
                                }
                                if response.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::Cell);
                                }

                                if value.len() > 100 {
                                    response = response.on_hover_ui(|ui| {
                                        ui.label(value);
                                    });
                                };
                                return Some(response);
                            }
                        }
                    }
                    let rect = ui.available_rect_before_wrap();
                    let response = ui.interact(rect, Id::new(cell_id), Sense::click());
                    if response.double_clicked() {
                        *self.editing_value.borrow_mut() = String::new();
                        *editing_index = Some((col_index, row_index, pinned_column_table));
                    }

                    if response.secondary_clicked() {
                        focused_cell = Some(CellLocation { column_index: col_index, row_index: table_row_index, is_pinned_column_table: pinned_column_table });
                        focused_changed = true;
                    }
                    response.context_menu(|ui| {
                        if ui.button("Edit").clicked() {
                            *self.editing_value.borrow_mut() = String::new();
                            *editing_index = Some((col_index, row_index, pinned_column_table));
                            ui.close_menu();
                        }
                        if !self.is_sub_table {
                            ui.separator();
                            if ui.button("Open row in sub table".to_string()).clicked() {
                                ui.close_menu();
                                let root_node = row_data.entries.last().unwrap();
                                subtable = Some(SubTable::new(root_node.pointer.pointer.clone(), root_node.value.as_ref().unwrap().clone(),
                                                              ValueType::Object(true),
                                                              row_index, root_node.pointer.depth,
                                ));
                            }
                        }
                    });

                    if let Some(focused_cell_location) = self.focused_cell {
                        if focused_cell_location.is_pinned_column_table == pinned_column_table && focused_cell_location.row_index == table_row_index && focused_cell_location.column_index == col_index && !response.context_menu_opened() {
                            focused_cell = None;
                            focused_changed = true;
                        }
                    }
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Cell);
                    }
                    Some(response)
                });
            }
        });
        if focused_changed {
            self.focused_cell = focused_cell;
        }
        if let Some(subtable) = subtable {
            self.windows.push(subtable);
        }
        if let Some((column_name, filter_value)) = filter_by_value {
            self.on_filter_column_value((column_name, filter_value));
        }
        if let Some((pointer, value)) = updated_value {
            let editing_index = mem::take(&mut *self.editing_index.borrow_mut());
            let value = if value.is_empty() {
                None
            } else {
                Some(value)
            };
            let (_, row_index, _) = editing_index.unwrap();
            if self.is_sub_table {
                let updated_pointer = pointer.clone();
                let value_changed = self.update_value(FlatJsonValue { pointer: updated_pointer.clone(), value: value.clone() }, row_index, false);

                if value_changed {
                    let mut entries = self.nodes.iter().flat_map(|row| row.entries.clone()).collect::<Vec<FlatJsonValue<String>>>();
                    let mut parent_pointer = PointerKey {
                        pointer: String::new(),
                        value_type: ValueType::Array(self.nodes.len()),
                        depth: 0,
                        index: 0,
                        position: 0,
                    };
                    entries.push(FlatJsonValue { pointer: parent_pointer.clone(), value: None });
                    let updated_array = serialize_to_json_with_option::<String>(&mut entries, updated_pointer.depth - 1).to_json();
                    parent_pointer.pointer = self.parent_pointer.clone();
                    array_response.edited_value = Some(FlatJsonValue { pointer: parent_pointer, value: Some(updated_array) });
                }
            } else {
                let value_changed = self.update_value(FlatJsonValue { pointer: pointer.clone(), value: value.clone() }, row_index, true);
                if value_changed {
                    array_response.edited_value = Some(FlatJsonValue { pointer, value });
                }
            }
        }
        if self.hovered_row_index != hover_data.hovered_row {
            self.hovered_row_index = hover_data.hovered_row;
            request_repaint = true;
        }
        array_response.hover_data = hover_data;
    }

    #[inline]
    fn columns(&self, pinned_column_table: bool) -> &Vec<Column> {
        if pinned_column_table { &self.column_pinned } else { &self.column_selected }
    }

    fn get_pointer_index_from_cache(&self, pinned_column_table: bool, row_data: &&JsonArrayEntries<String>, col_index: usize) -> Option<usize> {
        let index = {
            let mut cache_ref_mut = self.cache.borrow_mut();
            let cache = cache_ref_mut.cache::<crate::components::cache::FrameCache<Option<usize>, CacheGetPointer>>();
            let key = CachePointerKey {
                pinned_column_table,
                index: col_index,
                row_index: row_data.index(),
            };
            cache.get(key, &self)
        };
        index
    }

    #[inline]
    fn is_filterable(column: &Column) -> bool {
        !(matches!(column.value_type, ValueType::Object(_)) || matches!(column.value_type, ValueType::Array(_)) || matches!(column.value_type, ValueType::Null))
    }

    fn open_subtable(row_index: usize, entry: &FlatJsonValue<String>, content: String) -> Option<SubTable> {
        Some(SubTable::new(entry.pointer.pointer.clone(), content,
                           entry.pointer.value_type,
                           row_index, entry.pointer.depth,
        ))
    }

    fn update_value(&mut self, updated_entry: FlatJsonValue<String>, row_index: usize, should_update_subtable: bool) -> bool {
        let mut value_changed = false;
        if should_update_subtable {
            for subtable in self.windows.iter_mut() {
                if subtable.id() == row_index {
                    subtable.update_nodes(updated_entry.pointer.clone(), updated_entry.value.clone());
                    break;
                }
            }
        }

        if let Some(entry) = self.nodes[row_index].entries.iter_mut()
            .find(|entry| entry.pointer.pointer.eq(&updated_entry.pointer.pointer)) {
            if !entry.value.eq(&updated_entry.value) {
                value_changed = true;
                entry.value = updated_entry.value;
            }
        } else if updated_entry.value.is_some() {
            value_changed = true;
            let entries = &mut self.nodes[row_index].entries;
            entries.insert(entries.len() - 1, FlatJsonValue::<String> { pointer: updated_entry.pointer, value: updated_entry.value });
        }
        // After update we serialized root element then parse it again so nested serialized object are updated as well
        if value_changed && !self.is_sub_table {
            let root_node = self.nodes[row_index].entries.pop().unwrap();
            let value1 = serialize_to_json_with_option::<String>(
                &mut self.nodes[row_index].entries.clone(),
                root_node.pointer.depth + 1);
            let new_root_node_serialized_json = value1.to_json();
            let result = JSONParser::parse(new_root_node_serialized_json.as_str(),
                                           ParseOptions::default()
                                               .prefix(root_node.pointer.pointer.clone())
                                               .start_depth(root_node.pointer.depth + 1).parse_array(false)
                                               .max_depth(self.last_parsed_max_depth)).unwrap().to_owned();
            let line_number_entry = mem::take(&mut self.nodes[row_index].entries[0]);
            self.nodes[row_index].entries.clear();
            self.nodes[row_index].entries.push(line_number_entry);
            self.nodes[row_index].entries.extend(result.json);
            self.nodes[row_index].entries.push(FlatJsonValue { pointer: root_node.pointer, value: Some(new_root_node_serialized_json) });
        }
        if value_changed {
            self.cache.borrow_mut().evict();
        }
        value_changed
    }

    // C

    #[inline]
    fn get_pointer_index<'a>(parent_pointer: &String, columns: &Vec<Column>, data: &&'a Vec<FlatJsonValue<String>>, index: usize, row_index: usize) -> Option<(usize)> {
        if let Some(column) = columns.get(index) {
            let key = &column.name;
            let key = Self::pointer_key(parent_pointer, row_index, key);
            return data.iter().position(|entry| {
                entry.pointer.pointer.eq(&key)
            });
        }
        None
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
    pub(crate) fn nodes(&self) -> &Vec<JsonArrayEntries<String>> {
        &self.nodes
    }

    pub fn reset_search(&mut self) {
        self.scroll_to_row.clear();
        self.matching_rows.clear();
        self.changed_scroll_to_row_value = Some(crate::compatibility::now().sub(Duration::from_millis(1000)));
        self.matching_row_selected = 0;
    }
}
