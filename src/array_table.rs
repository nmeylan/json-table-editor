use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap};
use std::fmt::format;
use std::hash::{Hash, Hasher};
use std::mem;
use std::string::ToString;
use egui::{Align, Button, Context, CursorIcon, Id, ImageSource, Label, Response, Sense, TextBuffer, Ui, Vec2, Widget, WidgetText};
use egui::scroll_area::ScrollBarVisibility;
use indexmap::IndexSet;
use json_flat_parser::{FlatJsonValueOwned, JsonArrayEntriesOwned, ParseResultOwned, PointerKey, ValueType};

use crate::{concat_string, Window};
use crate::components::popover::PopupMenu;
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

#[derive(Default)]
pub struct ArrayTable {
    all_columns: Vec<Column>,
    column_selected: Vec<Column>,
    column_pinned: Vec<Column>,
    max_depth: u8,
    last_parsed_max_depth: u8,
    parse_result: Option<ParseResultOwned>,
    nodes: Vec<JsonArrayEntriesOwned>,
    filtered_nodes: Vec<JsonArrayEntriesOwned>,
    scroll_y: f32,
    columns_filter: HashMap<String, Vec<String>>,
    pub hovered_row_index: Option<usize>,
    columns_offset: Vec<f32>,
    parent_pointer: String,
    parent_value_type: ValueType,
    windows: Vec<SubTable>,
    pub scroll_to_column: String,

    pub next_frame_reset_scroll: bool,
    pub next_frame_scroll_to_column: bool,
}


const filter_icon: ImageSource = egui::include_image!("../icons/funnelRegular.svg");
const pin_icon: ImageSource = egui::include_image!("../icons/pinTab.svg");

impl super::View for ArrayTable {
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
                            let _scroll_area_output = scroll_area.show(ui, |ui| {
                                self.table_ui(ui, false);
                            });
                        });
                    });
                });
            });
    }
}

type ColumnFilterCache = egui::util::cache::FrameCache<IndexSet<String>, ArrayTable>;

impl egui::util::cache::ComputerMut<(&Column, &Vec<JsonArrayEntriesOwned>, &String), IndexSet<String>> for ArrayTable {
    fn compute(&mut self, (column, nodes, parent_pointer): (&Column, &Vec<JsonArrayEntriesOwned>, &String)) -> IndexSet<String> {
        let mut unique_values = IndexSet::new();
        if matches!(column.value_type, ValueType::String) {
            nodes.iter().enumerate().map(|(i, row)| {
                Self::get_pointer_for_column(parent_pointer, &&row.entries, i, column).map(|(_, value)| value.clone().unwrap())
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
    pub fn new(parse_result: Option<ParseResultOwned>, nodes: Vec<JsonArrayEntriesOwned>, all_columns: Vec<Column>, depth: u8, parent_pointer: String, parent_value_type: ValueType) -> Self {
        let last_parsed_max_depth = parse_result.as_ref().map_or(depth, |p| p.parsing_max_depth);
        Self {
            column_selected: Self::selected_columns(&all_columns, depth),
            all_columns,
            max_depth: depth,
            nodes,
            parse_result,
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
            columns_filter: HashMap::new(),
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
            table = table.column(Column::initial((columns[i].name.len() + 3).max(10) as f32 * text_width).clip(true).resizable(true));
        }
        let mut request_repaint = false;
        let mut click_on_array_row_index: Option<(usize, PointerKey)> = None;
        let mut hovered_on_array_row_index: Option<(usize, PointerKey)> = None;
        let table_scroll_output = table
            .header(text_height * 2.0, |mut header| {
                let clicked_filter_non_null_column: RefCell<Option<String>> = RefCell::new(None);
                let clicked_filter_column_value: RefCell<Option<(String, String)>> = RefCell::new(None);
                let pinned_column: RefCell<Option<usize>> = RefCell::new(None);
                let i: RefCell<usize> = RefCell::new(0);
                header.cols(true, |index| {
                    let columns = if pinned_column_table { &self.column_pinned } else { &self.column_selected };
                    let column = columns.get(index).unwrap();
                    let name = format!("{}", column.name.clone());
                    // let name = format!("{} - {}", column.name.clone(), column.depth);
                    let strong = Label::new(WidgetText::RichText(egui::RichText::from(&name)));
                    let label = Label::new(&name);
                    *i.borrow_mut() = index;
                    Some(Box::new(|ui: &mut Ui| {
                        let response = ui.vertical(|ui| {
                            let response = ui.add(strong).on_hover_ui(|ui| { ui.add(label); });

                            if !pinned_column_table || *i.borrow() > 0 {
                                ui.horizontal(|ui| {
                                    if column.name.eq("") {
                                        return;
                                    }
                                    let button = Button::image(pin_icon).frame(false);
                                    if ui.add(button).clicked() {
                                        *pinned_column.borrow_mut() = Some(*i.borrow());
                                    }
                                    let column_id = Id::new(&name);
                                    PopupMenu::new(column_id.with("filter"))
                                        .show_ui(ui, |ui| ui.add(Button::image(filter_icon).frame(false)),
                                                 |ui| {
                                                     let mut checked_filtered_values = self.columns_filter.get(&column.name);
                                                     let mut chcked = if let Some(filters) = checked_filtered_values {
                                                         filters.contains(&NON_NULL_FILTER_VALUE.to_owned())
                                                     } else {
                                                         false
                                                     };
                                                     if ui.checkbox(&mut chcked, "Non null").clicked() {
                                                         *clicked_filter_non_null_column.borrow_mut() = Some(name);
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
                                                                     *clicked_filter_column_value.borrow_mut() = Some((column.name.clone(), value.clone()));
                                                                 }
                                                             });
                                                         }
                                                     }
                                                 });
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
                let clicked_filter_non_null_column = clicked_filter_non_null_column.borrow();
                if let Some(clicked_column) = clicked_filter_non_null_column.as_ref() {
                    self.on_filter_column_value((clicked_column.clone(), NON_NULL_FILTER_VALUE.to_string()));
                }
                let clicked_filter_column_value = clicked_filter_column_value.borrow();
                if let Some(clicked_column) = clicked_filter_column_value.as_ref() {
                    self.on_filter_column_value(clicked_column.clone());
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
                                let is_array = matches!(pointer.value_type, ValueType::Array(_));
                                let is_object = matches!(pointer.value_type, ValueType::Object(_));
                                if is_array || is_object {
                                    click_on_array_row_index = Some((row_index, pointer.clone()));
                                }
                            }
                        }
                        if let Some(index) = response.hovered_col_index {
                            let data = self.get_pointer(columns, &data.entries(), index, data.index());
                            if let Some((pointer, _value)) = data {
                                if matches!(pointer.value_type, ValueType::Array(_)) || matches!(pointer.value_type, ValueType::Object(_)) {
                                    hovered_on_array_row_index = Some((row_index, pointer.clone()));
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

        if let Some(_) = hovered_on_array_row_index {
            ui.ctx().set_cursor_icon(CursorIcon::ZoomIn);
        }
        if let Some((row_index, pointer)) = click_on_array_row_index {
            let json_array_entries = &self.nodes()[row_index];
            if let Some((_, value)) = json_array_entries.find_node_at(pointer.pointer.as_str()) {
                let content = value.clone().unwrap();
                self.windows.push(SubTable::new(pointer.pointer, content,
                                                if matches!(pointer.value_type, ValueType::Array(_)) { ValueType::Array(0) } else { ValueType::Object(true) }))
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

    fn get_pointer<'a>(&self, columns: &Vec<Column>, data: &&'a FlatJsonValueOwned, index: usize, row_index: usize) -> Option<&'a (PointerKey, Option<String>)> {
        if let Some(column) = columns.get(index) {
            return Self::get_pointer_for_column(&self.parent_pointer, data, row_index, column);
        }
        None
    }

    fn get_pointer_for_column<'a>(parent_pointer: &String, data: &&'a FlatJsonValueOwned, row_index: usize, column: &Column) -> Option<&'a (PointerKey, Option<String>)> {
        let key = &column.name;
        let key = concat_string!(parent_pointer, "/", row_index.to_string(), key);
        return data.iter().find(|(pointer, _)| {
            pointer.pointer.eq(&key)
        });
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
            self.filtered_nodes.clear();
        } else {
            self.filtered_nodes = crate::parser::filter_columns(&self.nodes, &self.parent_pointer, &self.columns_filter);
        }
        self.next_frame_reset_scroll = true;
    }

    #[inline]
    fn nodes(&self) -> &Vec<JsonArrayEntriesOwned> {
        if self.columns_filter.is_empty() {
            &self.nodes
        } else {
            &self.filtered_nodes
        }
    }
}
