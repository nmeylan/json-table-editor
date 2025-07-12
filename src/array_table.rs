use crate::components::cell_text::CellText;
use crate::components::icon;
use crate::components::icon::ButtonWithIcon;
use crate::components::popover::PopupMenu;
use crate::components::table::{CellLocation, TableBody, TableRow};
use crate::fonts::{COPY, FILTER, PENCIL, PLUS, SEARCH, TABLE, TABLE_CELLS, THUMBTACK};
use crate::panels::{SearchReplacePanel, SearchReplaceResponse, PANEL_REPLACE};
use crate::parser::{replace_occurrences, row_number_entry, search_occurrences};
use crate::subtable_window::SubTable;
use crate::{
    concat_string, set_open, ArrayResponse, Window, ACTIVE_COLOR, SHORTCUT_COPY, SHORTCUT_DELETE,
    SHORTCUT_REPLACE,
};
use eframe::egui::scroll_area::ScrollBarVisibility;
use eframe::egui::style::Spacing;
use eframe::egui::{
    Align, Context, CursorIcon, Id, Key, Label, Sense, Style, TextEdit, Ui, Vec2, Widget,
    WidgetText,
};
use eframe::epaint::text::TextWrapMode;
use egui::{EventFilter, InputState, Modifiers, Rangef, TextBuffer};
use indexmap::IndexSet;
use json_flat_parser::serializer::serialize_to_json_with_option;
use json_flat_parser::{
    FlatJsonValue, JSONParser, JsonArrayEntries, ParseOptions, ParseResult, PointerKey, ValueType,
};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use rayon::prelude::ParallelSliceMut;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Sub;
use std::string::ToString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Column<'col> {
    pub name: Cow<'col, str>,
    pub depth: u8,
    pub value_type: ValueType,
    pub seen_count: usize,
    pub order: usize,
    pub id: usize,
}

impl Hash for Column<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state)
    }
}

impl Column<'_> {
    pub fn new(name: String, value_type: ValueType) -> Self {
        Self {
            name: Cow::from(name),
            depth: 0,
            value_type,
            seen_count: 0,
            order: 0,
            id: 0,
        }
    }
}

impl Eq for Column<'_> {}

impl PartialEq<Self> for Column<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl PartialOrd<Self> for Column<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Column<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.seen_count.cmp(&self.seen_count) {
            Ordering::Equal => other.order.cmp(&self.order),
            cmp => cmp,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
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

pub struct ArrayTable<'array> {
    table_id: Id,
    all_columns: Vec<Column<'array>>,
    column_selected: Vec<Column<'array>>,
    column_pinned: Vec<Column<'array>>,
    pub max_depth: u8,
    last_parsed_max_depth: u8,
    parse_result: Option<ParseResult<String>>,
    pub nodes: Vec<JsonArrayEntries<String>>,
    filtered_nodes: Vec<usize>,
    scroll_y: f32,
    pub columns_filter: HashMap<String, Vec<String>>,
    pub hovered_row_index: Option<usize>,
    columns_offset: Vec<f32>,
    windows: Vec<SubTable<'array>>,
    // Indicate if this array table is a subtable
    pub(crate) is_sub_table: bool,
    // For subtable we need to get parent_pointer info
    pub parent_pointer: PointerKey,
    cache: RefCell<crate::components::cache::CacheStorage>,
    seed1: usize, // seed for Id
    seed2: usize, // seed for Id
    pub matching_rows: Vec<usize>,
    pub matching_row_selected: usize,
    pub matching_columns: Vec<usize>,
    pub matching_column_selected: usize,
    pub scroll_to_column: String,
    pub scroll_to_row: String,
    pub scroll_to_row_number: usize,
    pub scroll_to_column_number: usize,
    pub scroll_to_row_mode: ScrollToRowMode,
    pub focused_cell: Option<CellLocation>,

    // Visibility information
    pub first_visible_index: usize,
    pub last_visible_index: usize,
    pub first_visible_offset: f32,
    pub last_visible_offset: f32,

    // Handle interaction
    pub next_frame_reset_scroll: bool,
    pub changed_scroll_to_column_value: bool,
    pub changed_matching_column_selected: bool,
    pub changed_matching_row_selected: bool,
    pub changed_arrow_horizontal_scroll: bool,
    pub changed_arrow_vertical_scroll: bool,
    pub was_editing: bool,

    #[cfg(not(target_arch = "wasm32"))]
    pub changed_scroll_to_row_value: Option<std::time::Instant>,
    #[cfg(target_arch = "wasm32")]
    pub changed_scroll_to_row_value: Option<crate::compatibility::InstantWrapper>,

    pub editing_index: RefCell<Option<(usize, usize, bool)>>,
    pub editing_value: RefCell<String>,

    opened_windows: BTreeSet<String>,
    search_replace_panel: SearchReplacePanel<'array>,
}

impl super::View<ArrayResponse> for ArrayTable<'_> {
    fn ui(&mut self, ui: &mut egui::Ui) -> ArrayResponse {
        let mut array_response = ArrayResponse::default();
        self.windows(ui.ctx(), &mut array_response);
        let parent_height_available = ui.available_rect_before_wrap().height();
        let parent_width_available = ui.available_rect_before_wrap().width();
        ui.interact(
            ui.available_rect_before_wrap(),
            self.table_id,
            Sense::focusable_noninteractive(),
        );
        ui.horizontal(|ui| {
            ui.set_height(parent_height_available);
            ui.push_id("table-pinned-column", |ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(parent_width_available / 2.0);
                    let scroll_area = egui::ScrollArea::horizontal();
                    scroll_area.show(ui, |ui| {
                        // Pinned table
                        array_response = array_response.union(self.table_ui(ui, true));
                    });
                });
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
                            if column
                                .name
                                .to_lowercase()
                                .eq(&concat_string!("/", &self.scroll_to_column.to_lowercase()))
                                || column
                                    .name
                                    .to_lowercase()
                                    .contains(&self.scroll_to_column.to_lowercase())
                            {
                                self.matching_columns.push(index);
                            }
                        }
                    }
                }

                if self.changed_arrow_horizontal_scroll {
                    self.changed_arrow_horizontal_scroll = false;
                    if !(self.first_visible_index < self.scroll_to_column_number
                        && self.scroll_to_column_number <= self.last_visible_index)
                    {
                        if let Some(offset) = self.columns_offset.get(self.scroll_to_column_number)
                        {
                            scroll_to_x = Some(*offset);
                        }
                    }
                }

                if self.changed_matching_column_selected {
                    self.changed_matching_column_selected = false;
                    if !self.matching_columns.is_empty() {
                        if let Some(offset) = self
                            .columns_offset
                            .get(self.matching_columns[self.matching_column_selected])
                        {
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

        if self.focused_cell.is_some() && self.editing_index.borrow().is_none() {
            ui.ctx().memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.table_id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
        }

        self.cache.borrow_mut().update();

        if self.editing_index.borrow().is_none() {
            self.handle_shortcut(ui, &mut array_response);
        }
        self.was_editing = false;
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

impl<'array>
    crate::components::cache::ComputerMut<
        (&Column<'array>, &String),
        &Vec<JsonArrayEntries<String>>,
        IndexSet<String>,
    > for CacheFilterOptions
{
    fn compute(
        &mut self,
        (column, parent_pointer): (&Column<'array>, &String),
        nodes: &Vec<JsonArrayEntries<String>>,
    ) -> IndexSet<String> {
        let mut unique_values = IndexSet::new();
        if ArrayTable::is_filterable(column) {
            nodes
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    ArrayTable::get_pointer_for_column(parent_pointer, &&row.entries, i, column)
                        .filter(|entry| entry.value.is_some())
                        .map(|entry| entry.value.clone().unwrap())
                })
                .for_each(|value| {
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
                    (Err(_), Err(_)) => std::cmp::Ordering::Equal,  // Treat errors equally
                }
            });
        } else {
            unique_values.sort_by(|a, b| a.cmp(b));
        }
        unique_values
    }
}

impl<'array>
    crate::components::cache::ComputerMut<CachePointerKey, &ArrayTable<'array>, Option<usize>>
    for CacheGetPointer
{
    fn compute(
        &mut self,
        cache_pointer_key: CachePointerKey,
        table: &ArrayTable<'array>,
    ) -> Option<usize> {
        let columns = if cache_pointer_key.pinned_column_table {
            &table.column_pinned
        } else {
            &table.column_selected
        };
        ArrayTable::get_pointer_index(
            &table.parent_pointer,
            columns,
            &table.nodes()[cache_pointer_key.row_index].entries(),
            cache_pointer_key.index,
            cache_pointer_key.row_index,
        )
    }
}

pub const NON_NULL_FILTER_VALUE: &str = "__non_null";

impl<'array> ArrayTable<'array> {
    pub fn new(
        parse_result: Option<ParseResult<String>>,
        nodes: Vec<JsonArrayEntries<String>>,
        all_columns: Vec<Column<'array>>,
        depth: u8,
        parent_pointer: PointerKey,
    ) -> Self {
        let last_parsed_max_depth = parse_result.as_ref().map_or(depth, |p| p.parsing_max_depth);
        Self {
            table_id: Id::new(format!("table-container-{}", parent_pointer.pointer)),
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
            seed1: Id::new(&parent_pointer.pointer).value() as usize,
            seed2: Id::new(format!("{}pinned", &parent_pointer.pointer)).value() as usize,
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
            scroll_to_row_number: 0,
            scroll_to_column_number: 0,
            changed_scroll_to_row_value: None,
            changed_matching_row_selected: false,
            changed_matching_column_selected: false,
            changed_arrow_horizontal_scroll: false,
            changed_arrow_vertical_scroll: false,
            editing_index: RefCell::new(None),
            editing_value: RefCell::new(String::new()),
            is_sub_table: false,
            focused_cell: None,
            first_visible_index: 0,
            last_visible_index: 0,
            first_visible_offset: 0.0,
            last_visible_offset: 0.0,
            cache: Default::default(),
            opened_windows: Default::default(),
            search_replace_panel: Default::default(),
            was_editing: false,
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
                    for entry in response.edited_value {
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
                array_response.edited_value.push(updated_value.0.clone())
            }
        }
        self.windows.retain(|w| !closed_windows.contains(w.name()));

        let mut is_open = self
            .opened_windows
            .contains(self.search_replace_panel.name());
        let response = self.search_replace_panel.show(ctx, &mut is_open);
        set_open(
            &mut self.opened_windows,
            self.search_replace_panel.name(),
            is_open,
        );
        if let Some(search_replace_response) = response {
            self.replace_columns(search_replace_response, array_response);
        }
    }

    pub fn update_selected_columns(&mut self, depth: u8) -> Option<usize> {
        self.cache.borrow_mut().update();
        if depth <= self.last_parsed_max_depth {
            let mut column_selected = Self::selected_columns(&self.all_columns, depth);
            column_selected.retain(|c| !self.column_pinned.contains(c));
            self.column_selected = column_selected;
            if self.column_selected.is_empty() {
                self.column_selected.push(Column {
                    name: Cow::from(""),
                    depth,
                    value_type: Default::default(),
                    seen_count: 0,
                    order: 0,
                    id: 0,
                })
            }
            None
        } else {
            let previous_parse_result = self.parse_result.clone().unwrap();
            let (new_json_array, new_columns, new_max_depth) = crate::parser::change_depth_array(
                previous_parse_result,
                mem::take(&mut self.nodes),
                depth as usize,
            )
            .unwrap();
            self.all_columns = new_columns;
            let mut column_selected = Self::selected_columns(&self.all_columns, depth);
            column_selected.retain(|c| !self.column_pinned.contains(c));
            self.column_selected = column_selected;
            self.nodes = new_json_array;
            self.last_parsed_max_depth = depth;
            self.parse_result.as_mut().unwrap().parsing_max_depth = depth;
            self.parse_result.as_mut().unwrap().max_json_depth = new_max_depth;
            if self.opened_windows.contains(PANEL_REPLACE) {
                // Refresh list of columns
                self.open_replace_panel(None);
            }
            Some(new_max_depth)
        }
    }
    pub fn update_max_depth(&mut self, depth: u8) -> Option<usize> {
        self.max_depth = depth;
        self.update_selected_columns(depth)
    }

    fn selected_columns(all_columns: &Vec<Column<'array>>, depth: u8) -> Vec<Column<'array>> {
        let mut column_selected: Vec<Column<'array>> = vec![];
        for col in Self::visible_columns(all_columns, depth) {
            column_selected.push(col.clone())
        }
        column_selected
    }

    pub fn all_columns(&self) -> &Vec<Column<'array>> {
        &self.all_columns
    }

    pub fn visible_columns<'a>(
        all_columns: &'a Vec<Column<'array>>,
        depth: u8,
    ) -> impl Iterator<Item = &'a Column<'array>> {
        all_columns.iter().filter(move |column: &&Column<'array>| {
            column.depth == depth
                || (column.depth < depth && !matches!(column.value_type, ValueType::Object(_, _)))
        })
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
    fn draw_table(
        &mut self,
        ui: &mut Ui,
        text_height: f32,
        text_width: f32,
        pinned_column_table: bool,
    ) -> ArrayResponse {
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
            .scroll_bar_visibility(if pinned_column_table {
                ScrollBarVisibility::AlwaysHidden
            } else {
                ScrollBarVisibility::AlwaysVisible
            });

        if self.next_frame_reset_scroll {
            table = table.scroll_to_row(0, Some(Align::Center));
            self.next_frame_reset_scroll = false;
        }
        if let Some(changed_scroll_to_row_value) = self.changed_scroll_to_row_value {
            match self.scroll_to_row_mode {
                ScrollToRowMode::RowNumber => {
                    self.changed_scroll_to_row_value = None;
                    table = table.scroll_to_row(
                        self.scroll_to_row.parse::<usize>().unwrap_or_else(|_| {
                            self.scroll_to_row.clear();
                            0
                        }),
                        Some(Align::TOP),
                    );
                }
                ScrollToRowMode::MatchingTerm => {
                    if changed_scroll_to_row_value.elapsed().as_millis() >= 300 {
                        self.changed_scroll_to_row_value = None;
                        if !self.scroll_to_row.is_empty() {
                            self.matching_rows =
                                search_occurrences(&self.nodes, &self.scroll_to_row.to_lowercase());
                            self.matching_row_selected = 0;
                            if !self.matching_rows.is_empty() {
                                self.changed_matching_row_selected = true;
                            }
                        }
                    }
                }
            }
        }
        if self.changed_arrow_vertical_scroll {
            self.changed_arrow_vertical_scroll = false;
            table = table.scroll_to_row(self.scroll_to_row_number, Some(Align::Center));
        }
        if self.changed_matching_row_selected {
            self.changed_matching_row_selected = false;
            table = table.scroll_to_row(
                self.matching_rows[self.matching_row_selected],
                Some(Align::Center),
            );
        }
        table = table.vertical_scroll_offset(self.scroll_y);

        let columns_count = if pinned_column_table {
            self.column_pinned.len()
        } else {
            self.column_selected.len()
        };
        let columns = self.columns(pinned_column_table);
        if columns_count <= 3 {
            for i in 0..columns_count {
                if pinned_column_table && i == 0 {
                    table = table.column(Column::initial(40.0).clip(true).resizable(true));
                } else {
                    table = table.column(Column::remainder().clip(true).resizable(true));
                }
            }
        } else {
            for i in 0..columns_count {
                if pinned_column_table && i == 0 {
                    table = table.column(Column::initial(40.0).clip(true).resizable(true));
                } else if i == columns_count - 1 {
                    table = table.column(Column::remainder().clip(false).resizable(true).range(Rangef::new(240.0, f32::INFINITY)));
                } else {
                    table = table.column(
                        Column::initial((columns[i].name.len() + 3).max(10) as f32 * text_width)
                            .clip(true)
                            .resizable(true),
                    );
                }
                // table = table.column(Column::initial(10.0).clip(true).resizable(true));

            }
        }

        let request_repaint = false;
        let search_highlight_row = if !self.matching_rows.is_empty() {
            Some(self.matching_rows[self.matching_row_selected])
        } else {
            None
        };
        let focused_cell = self.focused_cell.or(self.editing_index.borrow().map(
            |(column_index, row_index, is_pinned_column_table)| CellLocation {
                column_index,
                row_index,
                is_pinned_column_table,
            },
        ));
        let table_response = table
            .header(text_height * 2.0, |header| {
                self.header(pinned_column_table, header);
            })
            .body(
                self.hovered_row_index,
                search_highlight_row,
                focused_cell,
                |body| {
                    self.body(
                        text_height,
                        pinned_column_table,
                        &mut array_response,
                        request_repaint,
                        body,
                    );
                },
            );

        let table_scroll_output = table_response.scroll_area_output;
        if self.scroll_y != table_scroll_output.state.offset.y {
            self.scroll_y = table_scroll_output.state.offset.y;
        }
        if !pinned_column_table {
            self.columns_offset = table_response.columns_offset;
            self.first_visible_index = table_response.first_visible_index;
            self.first_visible_offset = table_response.first_visible_offset;
            self.last_visible_index = table_response.last_visible_index;
            self.last_visible_offset = table_response.last_visible_offset;
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
        let mut clicked_replace_column: Option<usize> = None;
        header.cols(true, |ui, index| {
            let columns = self.columns(pinned_column_table);
            let column = columns.get(index).unwrap();
            let name = column.name.as_str();
            let strong = Label::new(WidgetText::RichText(egui::RichText::from(name)));
            let label = Label::new(name);
            let response = ui.vertical(|ui| {
                let response = ui.add(strong).on_hover_ui(|ui| {
                    ui.add(label);
                });

                if !pinned_column_table || index > 0 {
                    ui.horizontal(|ui| {
                        if column.name.eq("") {
                            return;
                        }
                        let response = icon::button(
                            ui,
                            THUMBTACK,
                            Some(if pinned_column_table {
                                "Unpin column"
                            } else {
                                "Pin column to left"
                            }),
                            None,
                        );
                        if response.clicked() {
                            pinned_column = Some(index);
                        }
                        let column_id = Id::new(name);
                        let checked_filtered_values = self.columns_filter.get(column.name.as_str());
                        PopupMenu::new(column_id.with("filter")).show_ui(
                            ui,
                            |ui| {
                                icon::button(
                                    ui,
                                    FILTER,
                                    None,
                                    if checked_filtered_values.is_some() {
                                        Some(ACTIVE_COLOR)
                                    } else {
                                        None
                                    },
                                )
                            },
                            |ui| {
                                let mut chcked = if let Some(filters) = checked_filtered_values {
                                    filters.contains(&NON_NULL_FILTER_VALUE.to_owned())
                                } else {
                                    false
                                };
                                if ui.checkbox(&mut chcked, "Non null").clicked() {
                                    clicked_filter_non_null_column = Some(name.to_string());
                                }

                                if Self::is_filterable(column) {
                                    let mut cache_ref_mut = self.cache.borrow_mut();
                                    let cache = cache_ref_mut
                                        .cache::<crate::components::cache::FrameCache<
                                            IndexSet<String>,
                                            CacheFilterOptions,
                                        >>();

                                    let values = cache
                                        .get((column, &self.parent_pointer.pointer), &self.nodes);
                                    if !values.is_empty() {
                                        let checked_filtered_values =
                                            self.columns_filter.get(column.name.as_str());
                                        ui.separator();
                                        values.iter().for_each(|value| {
                                            let mut chcked =
                                                if let Some(filters) = checked_filtered_values {
                                                    filters.contains(value)
                                                } else {
                                                    false
                                                };
                                            if ui.checkbox(&mut chcked, value).clicked() {
                                                clicked_filter_column_value =
                                                    Some((column.name.to_string(), value.clone()));
                                            }
                                        });
                                    }
                                }
                            },
                        );

                        if SearchReplacePanel::can_be_replaced(column) {
                            let response =
                                icon::button(ui, SEARCH, Some("Replace in column"), None);
                            if response.clicked() {
                                clicked_replace_column = Some(index);
                            }
                        }
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
        if let Some(replace_column) = clicked_replace_column {
            let column = self.columns(pinned_column_table)[replace_column].clone();
            self.open_replace_panel(Some(column));
        }
        if let Some(clicked_column) = clicked_filter_non_null_column {
            self.on_filter_column_value((clicked_column, NON_NULL_FILTER_VALUE.to_string()));
        }
        if let Some(clicked_column) = clicked_filter_column_value {
            self.on_filter_column_value(clicked_column);
        }
    }

    fn body(
        &mut self,
        text_height: f32,
        pinned_column_table: bool,
        array_response: &mut ArrayResponse,
        mut request_repaint: bool,
        body: TableBody,
    ) {
        // Mutation after interaction
        let mut subtable = None;
        let mut focused_cell = None;
        let mut focused_changed = false;
        let mut updated_value: Option<(PointerKey, String)> = None;
        let mut filter_by_value: Option<(String, String)> = None; // col name, value
        let mut insert_row_at_index: Option<(usize, u8)> = None; // table_row_index, 0 = above, 1 = below
        let columns = self.columns(pinned_column_table);
        let hover_data = body.rows(text_height, self.filtered_nodes.len(), |mut row| {
            let table_row_index = row.index();
            let row_index = self.filtered_nodes[table_row_index];
            let node = self.nodes().get(row_index);

            if let Some(row_data) = node.as_ref() {
                row.cols(false, |ui, col_index| {
                    let cell_id = row_index * columns.len()
                        + col_index
                        + if pinned_column_table {
                            self.seed1
                        } else {
                            self.seed2
                        };
                    let index =
                        self.get_pointer_index_from_cache(pinned_column_table, row_data, col_index);
                    let mut editing_index = self.editing_index.borrow_mut();
                    if editing_index.is_some()
                        && editing_index.unwrap() == (col_index, row_index, pinned_column_table)
                    {
                        focused_changed = true;
                        focused_cell = None;
                        let ref_mut = &mut *self.editing_value.borrow_mut();
                        let text_edit = TextEdit::singleline(ref_mut);
                        let textedit_response = ui.add(text_edit.desired_width(f32::INFINITY));
                        if textedit_response.lost_focus()
                            || ui
                                .ctx()
                                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
                        {
                            let pointer = PointerKey {
                                pointer: Self::pointer_key(
                                    &self.parent_pointer.pointer,
                                    row_index,
                                    &columns.get(col_index).as_ref().unwrap().name,
                                ),
                                value_type: columns[col_index].value_type,
                                depth: columns[col_index].depth,
                                position: 0,
                                column_id: columns[col_index].id,
                            };
                            updated_value = Some((pointer, mem::take(ref_mut)));
                            focused_changed = true;
                            focused_cell = Some(CellLocation {
                                column_index: col_index,
                                row_index: table_row_index,
                                is_pinned_column_table: pinned_column_table,
                            });
                        } else {
                            textedit_response.request_focus();
                        }
                    } else if let Some(index) = index {
                        let entry = &row_data.entries()[index];

                        if pinned_column_table && col_index == 0 {
                            let label = Label::new(row_index.to_string());
                            return Some(label.ui(ui));
                        } else if let Some(value) = entry.value.as_ref() {
                            if !matches!(entry.pointer.value_type, ValueType::Null) {
                                let label = if value.len() > 1000 {
                                    CellText::new(&value[0..1000])
                                } else {
                                    CellText::new(value)
                                };

                                let mut response = label.ui(ui, cell_id);

                                if response.double_clicked() {
                                    *self.editing_value.borrow_mut() = value.clone();
                                    *editing_index =
                                        Some((col_index, row_index, pinned_column_table));
                                }
                                if response.secondary_clicked() || response.clicked() {
                                    focused_cell = Some(CellLocation {
                                        column_index: col_index,
                                        row_index: table_row_index,
                                        is_pinned_column_table: pinned_column_table,
                                    });

                                    ui.ctx().memory_mut(|m| m.request_focus(self.table_id));

                                    focused_changed = true;
                                }

                                if response.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::Cell);
                                }

                                if value.len() > 100 {
                                    response = response.on_hover_ui(|ui| {
                                        ui.style_mut().interaction.selectable_labels = true;
                                        let scroll_area = egui::ScrollArea::vertical();
                                        scroll_area.show(ui, |ui| {
                                            ui.label(value).request_focus();
                                        });
                                    });
                                };
                                return Some(response);
                            }
                        }
                    }
                    // No value cell
                    let rect = ui.available_rect_before_wrap();
                    let response = ui.interact(rect, Id::new(cell_id), Sense::click());
                    if response.double_clicked() {
                        *self.editing_value.borrow_mut() = String::new();
                        *editing_index = Some((col_index, row_index, pinned_column_table));
                    }

                    if response.secondary_clicked() || response.clicked() {
                        focused_cell = Some(CellLocation {
                            column_index: col_index,
                            row_index: table_row_index,
                            is_pinned_column_table: pinned_column_table,
                        });
                        ui.ctx().memory_mut(|m| m.request_focus(self.table_id));
                        focused_changed = true;
                    }

                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Cell);
                    }
                    if updated_value.is_some() {
                        ui.ctx().memory_mut(|m| m.request_focus(self.table_id));
                    }
                    Some(response)
                });
            }
        });
        // Context menu
        if let Some(ref hover_cell) = hover_data.hovered_cell {
            if let Some(ref response) = hover_data.response_rows {
                response.context_menu(|ui| {
                    let table_row_index = hover_cell.row_index;
                    let col_index = hover_cell.column_index;
                    let row_index = self.filtered_nodes.get(table_row_index);
                    if let Some(row_index) = row_index {
                        let row_index = *row_index;
                        let node = self.nodes().get(row_index);
                        if let Some(row_data) = node.as_ref() {
                            let index = self.get_pointer_index_from_cache(
                                pinned_column_table,
                                row_data,
                                col_index,
                            );
                            let mut edit_value = String::new();
                            let mut edit_entry: Option<&FlatJsonValue<String>> = None;
                            if let Some(index) = index {
                                let entry = &row_data.entries()[index];
                                if let Some(value) = entry.value.as_ref() {
                                    edit_value = value.clone();
                                }
                                edit_entry = Some(entry);
                            }
                            // Context menu: edit
                            let button = ButtonWithIcon::new("Edit", PENCIL);
                            if ui.add(button).clicked() {
                                *self.editing_index.borrow_mut() =
                                    Some((col_index, row_index, pinned_column_table));
                                *self.editing_value.borrow_mut() = mem::take(&mut edit_value);
                                ui.close_menu();
                            }
                            if !edit_value.is_empty() {
                                // Context menu: copy
                                let button = ButtonWithIcon::new("Copy", COPY)
                                    .shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_COPY));
                                if ui.add(button).clicked() {
                                    ui.ctx().copy_text(edit_value.clone());
                                    ui.close_menu();
                                }
                                // Context menu: filter by value
                                if Self::is_filterable(&columns[col_index]) {
                                    let button =
                                        ButtonWithIcon::new("Filter by this value", FILTER);
                                    if ui.add(button).clicked() {
                                        filter_by_value = Some((
                                            columns[col_index].name.to_string(),
                                            edit_value.clone(),
                                        ));
                                        ui.close_menu();
                                    }
                                }
                            }
                            ui.separator();
                            // Context menu: insert row above
                            let button = ButtonWithIcon::new("Insert row above", PLUS);
                            if ui.add(button).clicked() {
                                insert_row_at_index = Some((table_row_index, 0));
                                ui.close_menu();
                            }

                            // Context menu: insert row below
                            let button = ButtonWithIcon::new("Insert row below", PLUS);
                            if ui.add(button).clicked() {
                                insert_row_at_index = Some((table_row_index, 1));
                                ui.close_menu();
                            }
                            // Context menu: Open array or object in subtable
                            if let Some(entry) = edit_entry {
                                let is_array =
                                    matches!(entry.pointer.value_type, ValueType::Array(_));
                                let is_object =
                                    matches!(entry.pointer.value_type, ValueType::Object(..));
                                if is_array || is_object {
                                    ui.separator();
                                    let button = ButtonWithIcon::new(
                                        format!(
                                            "Open {} in sub table",
                                            if is_array { "array" } else { "object" }
                                        ),
                                        TABLE_CELLS,
                                    );
                                    if ui.add(button).clicked() {
                                        ui.close_menu();
                                        let content = edit_value.clone();
                                        subtable = Self::open_subtable(row_index, entry, content);
                                    }
                                }
                            }

                            // Context menu: Open row in subtable
                            if !self.is_sub_table {
                                ui.separator();
                                let button = ButtonWithIcon::new("Open row in sub table", TABLE);
                                if ui.add(button).clicked() {
                                    ui.close_menu();
                                    let root_node = row_data.entries.last().unwrap();
                                    subtable = Some(SubTable::new(
                                        root_node.pointer.clone(),
                                        root_node.value.as_ref().unwrap().clone(),
                                        ValueType::Object(true, 0),
                                        row_index,
                                        root_node.pointer.depth,
                                    ));
                                }
                            }
                            // Context menu: Open copy pointer
                            if let Some(entry) = edit_entry {
                                ui.separator();
                                if ui.button("Copy pointer").clicked() {
                                    ui.ctx().copy_text(entry.pointer.pointer.clone());
                                    ui.close_menu();
                                }
                            }
                        }
                    }
                });
            }
        }

        if focused_changed {
            self.focused_cell = focused_cell;
        }
        if let Some(subtable) = subtable {
            self.windows.push(subtable);
        }
        if let Some((column_name, filter_value)) = filter_by_value {
            self.on_filter_column_value((column_name, filter_value));
        }
        if let Some((table_row_index, above_or_below)) = insert_row_at_index {
            self.insert_new_row(table_row_index, above_or_below);
        }
        if let Some((pointer, value)) = updated_value {
            let editing_index = mem::take(&mut *self.editing_index.borrow_mut());
            let value = if value.is_empty() { None } else { Some(value) };
            let (_, row_index, _) = editing_index.unwrap();
            let value_changed = FlatJsonValue {
                pointer: pointer.clone(),
                value: value.clone(),
            };

            self.edit_cell(array_response, value_changed, row_index);
            self.was_editing = true;
        }
        if self.hovered_row_index != hover_data.hovered_row {
            self.hovered_row_index = hover_data.hovered_row;
            request_repaint = true;
        }
        array_response.hover_data = hover_data;
    }

    fn edit_cell(
        &mut self,
        array_response: &mut ArrayResponse,
        new_entry: FlatJsonValue<String>,
        row_index: usize,
    ) {
        if self.is_sub_table {
            let value_changed = self.update_value(new_entry, row_index, false);

            if value_changed {
                let mut entries = self
                    .nodes
                    .iter()
                    .flat_map(|row| row.entries.clone())
                    .collect::<Vec<FlatJsonValue<String>>>();
                let mut parent_pointer = PointerKey {
                    pointer: String::new(),
                    value_type: ValueType::Array(self.nodes.len()),
                    depth: 0,
                    position: 0,
                    column_id: 0,
                };
                entries.push(FlatJsonValue {
                    pointer: parent_pointer.clone(),
                    value: None,
                });
                // entries.iter().for_each(|e| println!("{} -> {:?}", e.pointer.pointer, e.value));
                let updated_array = serialize_to_json_with_option::<String>(
                    &mut entries,
                    self.parent_pointer.depth + 1,
                )
                .to_json();
                parent_pointer.pointer = self.parent_pointer.pointer.clone();
                array_response.edited_value.push(FlatJsonValue {
                    pointer: parent_pointer,
                    value: Some(updated_array),
                });
            }
        } else {
            let value_changed = self.update_value(new_entry.clone(), row_index, true);
            if value_changed {
                array_response.edited_value.push(new_entry);
            }
        }
    }

    fn insert_new_row(&mut self, table_row_index: usize, above_or_below: u8) {
        let row_index = self.filtered_nodes[table_row_index];
        let depth = self.nodes[row_index].entries.last().unwrap().pointer.depth;
        let new_table_row_index = table_row_index + above_or_below as usize;
        let new_index = row_index + above_or_below as usize;
        for i in new_table_row_index..self.filtered_nodes.len() {
            self.filtered_nodes[i] += 1;
        }
        // Performance are not good on large json but hopefully the feature is used rarely
        // We need to update all json pointer coming after the new row
        // For that we substring the pointer to remove the "prefix" containing the index in the json array
        let substring_len = self.parent_pointer.pointer.len() + 1;
        for i in new_index..self.nodes.len() {
            self.nodes[i].index = i + 1;
            let substring_len = substring_len + (i.checked_ilog10().unwrap_or(0) + 1) as usize;
            let new_prefix = concat_string!(self.parent_pointer.pointer, "/", (i + 1).to_string());
            self.nodes[i].entries.iter_mut().for_each(|e| {
                e.pointer.pointer = concat_string!(new_prefix, e.pointer.pointer[substring_len..]);
            })
        }
        let new_entry_pointer =
            concat_string!(self.parent_pointer.pointer, "/", new_index.to_string());
        self.nodes.insert(
            new_index,
            JsonArrayEntries {
                entries: vec![
                    row_number_entry(new_index, 0, new_entry_pointer.as_str()),
                    FlatJsonValue {
                        pointer: PointerKey {
                            pointer: new_entry_pointer,
                            value_type: ValueType::Object(true, 0),
                            depth,
                            position: 0,
                            column_id: 0,
                        },
                        value: Some("{}".to_string()),
                    },
                ],
                index: new_index,
            },
        );
        self.filtered_nodes
            .insert(table_row_index + above_or_below as usize, new_index);
        self.cache.borrow_mut().evict();
    }

    #[inline]
    fn columns<'a>(&'a self, pinned_column_table: bool) -> &'a Vec<Column<'array>> {
        if pinned_column_table {
            &self.column_pinned
        } else {
            &self.column_selected
        }
    }

    fn get_pointer_index_from_cache(
        &self,
        pinned_column_table: bool,
        row_data: &&JsonArrayEntries<String>,
        col_index: usize,
    ) -> Option<usize> {
        let index = {
            let mut cache_ref_mut = self.cache.borrow_mut();
            let cache = cache_ref_mut
                .cache::<crate::components::cache::FrameCache<Option<usize>, CacheGetPointer>>();
            let key = CachePointerKey {
                pinned_column_table,
                index: col_index,
                row_index: row_data.index(),
            };
            cache.get(key, self)
        };
        index
    }

    #[inline]
    fn is_filterable(column: &Column) -> bool {
        !(matches!(column.value_type, ValueType::Object(_, _))
            || matches!(column.value_type, ValueType::Array(_))
            || matches!(column.value_type, ValueType::Null))
    }

    fn open_subtable(
        row_index: usize,
        entry: &FlatJsonValue<String>,
        content: String,
    ) -> Option<SubTable<'array>> {
        Some(SubTable::new(
            entry.pointer.clone(),
            content,
            entry.pointer.value_type,
            row_index,
            entry.pointer.depth,
        ))
    }

    #[inline]
    fn update_value(
        &mut self,
        mut updated_entry: FlatJsonValue<String>,
        row_index: usize,
        should_update_subtable: bool,
    ) -> bool {
        if should_update_subtable {
            self.update_sub_tables_value(&mut updated_entry, row_index);
        }

        let value_changed = Self::update_row(
            &mut self.nodes[row_index].entries,
            updated_entry,
            self.is_sub_table,
            self.last_parsed_max_depth,
        );
        if value_changed {
            self.cache.borrow_mut().evict();
        }
        value_changed
    }

    #[inline]
    fn update_sub_tables_value(&mut self, updated_entry: &FlatJsonValue<String>, row_index: usize) {
        for subtable in self.windows.iter_mut() {
            if subtable.id() == row_index {
                subtable.update_nodes(updated_entry.pointer.clone(), updated_entry.value.clone());
                break;
            }
        }
    }

    #[inline]
    fn update_row(
        row_entries: &mut Vec<FlatJsonValue<String>>,
        mut updated_entry: FlatJsonValue<String>,
        is_sub_table: bool,
        last_parsed_max_depth: u8,
    ) -> bool {
        let mut value_changed = false;
        if let Some(entry) = row_entries
            .iter_mut()
            .find(|entry| entry.pointer.pointer.eq(&updated_entry.pointer.pointer))
        {
            if !entry.value.eq(&updated_entry.value) {
                value_changed = true;
                entry.value = updated_entry.value;
                if matches!(entry.pointer.value_type, ValueType::Null) {
                    entry.pointer.value_type = updated_entry.pointer.value_type;
                }
            }
        } else if updated_entry.value.is_some() {
            value_changed = true;
            updated_entry.pointer.position = usize::MAX;
            row_entries.insert(
                row_entries.len() - 1,
                FlatJsonValue::<String> {
                    pointer: updated_entry.pointer,
                    value: updated_entry.value,
                },
            );
        }
        // After update we serialize root element then parse it again so nested serialized object are updated as well
        if value_changed && !is_sub_table {
            let root_node = row_entries.pop().unwrap();
            let value1 = serialize_to_json_with_option::<String>(
                &mut row_entries.clone(),
                root_node.pointer.depth + 1,
            );
            let new_root_node_serialized_json = serde_json::to_string_pretty(&value1).unwrap();
            let result = JSONParser::parse(
                new_root_node_serialized_json.as_str(),
                ParseOptions::default()
                    .prefix(root_node.pointer.pointer.clone())
                    .start_depth(root_node.pointer.depth + 1)
                    .parse_array(false)
                    .max_depth(last_parsed_max_depth),
            )
            .unwrap()
            .to_owned();
            for newly_updated_value in result.json {
                if matches!(
                    newly_updated_value.pointer.value_type,
                    ValueType::Object(..)
                ) {
                    row_entries
                        .iter_mut()
                        .find(|e| e.pointer.pointer.eq(&newly_updated_value.pointer.pointer))
                        .map(|entry_to_update| entry_to_update.value = newly_updated_value.value);
                }
            }
            // let line_number_entry = mem::take(&mut self.nodes[row_index].entries[0]);
            // self.nodes[row_index].entries.clear();
            // self.nodes[row_index].entries.push(line_number_entry);
            // self.nodes[row_index].entries.extend(result.json);
            row_entries.push(FlatJsonValue {
                pointer: root_node.pointer,
                value: Some(new_root_node_serialized_json),
            });
        }
        value_changed
    }

    #[inline]
    fn get_pointer_index(
        parent_pointer: &PointerKey,
        columns: &Vec<Column>,
        data: &&Vec<FlatJsonValue<String>>,
        index: usize,
        row_index: usize,
    ) -> Option<usize> {
        if let Some(column) = columns.get(index) {
            let key = column.name.as_str();
            let key = Self::pointer_key(&parent_pointer.pointer, row_index, key);
            return data.iter().position(|entry| entry.pointer.pointer.eq(&key));
        }
        None
    }
    #[inline]
    fn get_pointer<'a>(
        &self,
        columns: &Vec<Column>,
        data: &&'a Vec<FlatJsonValue<String>>,
        index: usize,
        row_index: usize,
    ) -> Option<&'a FlatJsonValue<String>> {
        if let Some(column) = columns.get(index) {
            return Self::get_pointer_for_column(
                &self.parent_pointer.pointer,
                data,
                row_index,
                column,
            );
        }
        None
    }

    #[inline]
    fn get_pointer_for_column<'a>(
        parent_pointer: &String,
        data: &&'a Vec<FlatJsonValue<String>>,
        row_index: usize,
        column: &Column,
    ) -> Option<&'a FlatJsonValue<String>> {
        let key = column.name.as_str();
        let key = Self::pointer_key(parent_pointer, row_index, key);
        data.iter().find(|entry| entry.pointer.pointer.eq(&key))
    }

    #[inline]
    fn pointer_key(parent_pointer: &String, row_index: usize, key: &str) -> String {
        concat_string!(parent_pointer, "/", row_index.to_string(), key)
    }

    fn on_filter_column_value(&mut self, (column, value): (String, String)) {
        let maybe_filter = self.columns_filter.get_mut(column.as_str());
        if let Some(filter) = maybe_filter {
            if filter.contains(&value) {
                filter.retain(|v| !v.eq(&value));
                if filter.is_empty() {
                    self.columns_filter.remove(column.as_str());
                }
            } else {
                filter.push(value);
            }
        } else {
            self.columns_filter.insert(column, vec![value]);
        }
        self.do_filter_column();
    }

    fn do_filter_column(&mut self) {
        if self.columns_filter.is_empty() {
            self.filtered_nodes = (0..self.nodes.len()).collect::<Vec<usize>>();
        } else {
            self.filtered_nodes = crate::parser::filter_columns(
                &self.nodes,
                &self.parent_pointer.pointer,
                &self.columns_filter,
            );
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
        self.changed_scroll_to_row_value =
            Some(crate::compatibility::now().sub(Duration::from_millis(1000)));
        self.matching_row_selected = 0;
    }

    fn handle_shortcut(&mut self, ui: &mut Ui, array_response: &mut ArrayResponse) {
        let mut copied_value = None;
        let maybe_focused_id = ui.ctx().memory(|m| m.focused());
        ui.input_mut(|i| {
            if i.key_pressed(Key::Escape) {
                self.focused_cell = None;
            }

            let mut is_table_focused = false;
            if let Some(focused_id) = maybe_focused_id {
                if focused_id == self.table_id {
                    is_table_focused = true;
                }
            }
            if is_table_focused {
                if let Some(focused_cell) = self.focused_cell.as_mut() {
                    if i.consume_key(Modifiers::NONE, Key::Tab) {
                        if !focused_cell.is_pinned_column_table
                            && focused_cell.column_index < self.column_selected.len() - 1
                        {
                            focused_cell.column_index += 1;
                            self.scroll_to_column_number = focused_cell.column_index;
                            self.changed_arrow_horizontal_scroll = true;
                        } else if !focused_cell.is_pinned_column_table
                            && focused_cell.row_index < self.filtered_nodes.len() - 1
                        {
                            focused_cell.column_index = 0;
                            focused_cell.row_index += 1;
                            self.scroll_to_row_number = focused_cell.row_index;
                            self.changed_arrow_vertical_scroll = true;
                        } else if focused_cell.is_pinned_column_table
                            && focused_cell.column_index < self.column_pinned.len() - 1
                        {
                            focused_cell.column_index += 1;
                        } else if focused_cell.is_pinned_column_table {
                            focused_cell.column_index = 1;
                            focused_cell.row_index += 1;
                            self.scroll_to_row_number = focused_cell.row_index;
                            self.changed_arrow_vertical_scroll = true;
                        }
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                        if !focused_cell.is_pinned_column_table && focused_cell.column_index > 0 {
                            focused_cell.column_index -= 1;
                            self.scroll_to_column_number = focused_cell.column_index;
                            self.changed_arrow_horizontal_scroll = true;
                        } else if focused_cell.is_pinned_column_table
                            && focused_cell.column_index > 1
                        {
                            focused_cell.column_index -= 1;
                        }
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                        if !focused_cell.is_pinned_column_table
                            && focused_cell.column_index < self.column_selected.len() - 1
                        {
                            focused_cell.column_index += 1;
                            self.scroll_to_column_number = focused_cell.column_index;
                            self.changed_arrow_horizontal_scroll = true;
                        } else if focused_cell.is_pinned_column_table
                            && focused_cell.column_index < self.column_pinned.len() - 1
                        {
                            focused_cell.column_index += 1;
                        }
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) && focused_cell.row_index > 0 {
                        focused_cell.row_index -= 1;
                        self.scroll_to_row_number = focused_cell.row_index;
                        self.changed_arrow_vertical_scroll = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) && focused_cell.row_index < self.filtered_nodes.len() - 1 {
                        focused_cell.row_index += 1;
                        self.scroll_to_row_number = focused_cell.row_index;
                        self.changed_arrow_vertical_scroll = true;
                    }
                    let typed_alphanum = Self::get_typed_alphanum_from_events(i);
                    if (typed_alphanum.is_some() || i.consume_key(Modifiers::NONE, Key::Enter))
                        && !self.was_editing
                    {
                        let row_index = self.filtered_nodes[focused_cell.row_index];
                        *self.editing_index.borrow_mut() = Some((
                            focused_cell.column_index,
                            row_index,
                            focused_cell.is_pinned_column_table,
                        ));
                        let col_index = focused_cell.column_index;
                        let is_pinned_column_table = focused_cell.is_pinned_column_table;
                        let mut editing_value = String::new();
                        if let Some(typed_key) = typed_alphanum {
                            editing_value = typed_key;
                        } else {
                            {
                                let node = self.nodes().get(row_index);
                                if let Some(row_data) = node.as_ref() {
                                    let index = self.get_pointer_index_from_cache(
                                        is_pinned_column_table,
                                        row_data,
                                        col_index,
                                    );
                                    if let Some(index) = index {
                                        row_data.entries()[index]
                                            .value
                                            .clone()
                                            .map(|v| editing_value = v);
                                    }
                                }
                            }
                        }
                        *self.editing_value.borrow_mut() = editing_value;
                    }
                }

                if i.consume_shortcut(&SHORTCUT_DELETE) {
                    i.events.push(egui::Event::Key {
                        key: Key::Delete,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: Default::default(),
                    })
                }
                if i.consume_shortcut(&SHORTCUT_REPLACE) {
                    self.open_replace_panel(None);
                }
            }
            let hovered_cell = array_response.hover_data.hovered_cell;
            for event in i.events.iter().filter(|e| match e {
                egui::Event::Copy => hovered_cell.is_some(),
                egui::Event::Paste(_) => hovered_cell.is_some(),
                egui::Event::Key {
                    key: Key::Delete, ..
                } => hovered_cell.is_some(),
                _ => false,
            }) {
                let cell_location = hovered_cell.unwrap();
                let row_index = self.filtered_nodes[cell_location.row_index];
                let index = self.get_pointer_index_from_cache(
                    cell_location.is_pinned_column_table,
                    &&self.nodes[row_index],
                    cell_location.column_index,
                );

                match event {
                    egui::Event::Key {
                        key: Key::Delete, ..
                    } => {
                        let columns = self.columns(cell_location.is_pinned_column_table);
                        let pointer = Self::pointer_key(
                            &self.parent_pointer.pointer,
                            row_index,
                            columns
                                .get(cell_location.column_index)
                                .as_ref()
                                .unwrap()
                                .name
                                .as_str(),
                        );
                        let flat_json_value = FlatJsonValue::<String> {
                            pointer: PointerKey {
                                pointer,
                                value_type: columns[cell_location.column_index].value_type,
                                depth: columns[cell_location.column_index].depth,
                                position: 0,
                                column_id: columns[cell_location.column_index].id,
                            },
                            value: None,
                        };
                        self.update_value(flat_json_value, row_index, !self.is_sub_table);
                    }
                    egui::Event::Paste(v) => {
                        let columns = self.columns(cell_location.is_pinned_column_table);
                        let pointer = Self::pointer_key(
                            &self.parent_pointer.pointer,
                            row_index,
                            &columns
                                .get(cell_location.column_index)
                                .as_ref()
                                .unwrap()
                                .name,
                        );
                        let mut flat_json_value = FlatJsonValue::<String> {
                            pointer: PointerKey {
                                pointer,
                                value_type: columns[cell_location.column_index].value_type,
                                depth: columns[cell_location.column_index].depth,
                                position: 0,
                                column_id: columns[cell_location.column_index].id,
                            },
                            value: Some(v.clone()),
                        };
                        match flat_json_value.pointer.value_type {
                            // When we paste an object it should not be considered as parsed
                            ValueType::Object(..) => {
                                flat_json_value.pointer.value_type = ValueType::Object(false, 0)
                            }
                            _ => {}
                        }
                        self.edit_cell(array_response, flat_json_value, row_index);
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

    pub fn get_typed_alphanum_from_events(i: &mut InputState) -> Option<String> {
        let mut typed_alphanum: Option<String> = None;
        i.events.retain(|e| match e {
            egui::Event::Key { key, modifiers, .. }
                if matches!(
                    key,
                    Key::A
                        | Key::B
                        | Key::C
                        | Key::D
                        | Key::E
                        | Key::F
                        | Key::G
                        | Key::H
                        | Key::I
                        | Key::J
                        | Key::K
                        | Key::L
                        | Key::M
                        | Key::N
                        | Key::O
                        | Key::P
                        | Key::Q
                        | Key::R
                        | Key::S
                        | Key::T
                        | Key::U
                        | Key::V
                        | Key::W
                        | Key::X
                        | Key::Y
                        | Key::Z
                        | Key::Num0
                        | Key::Num1
                        | Key::Num2
                        | Key::Num3
                        | Key::Num4
                        | Key::Num5
                        | Key::Num6
                        | Key::Num7
                        | Key::Num8
                        | Key::Num9
                ) =>
            {
                if modifiers.ctrl || modifiers.command || modifiers.alt || modifiers.mac_cmd {
                    typed_alphanum = None;
                    return true;
                } else {
                    let mut typed_char = key.name().to_string();
                    if !matches!(modifiers, &Modifiers::SHIFT) {
                        typed_char = typed_char.to_lowercase();
                    }
                    typed_alphanum = Some(typed_char);
                }
                false
            }
            _ => true,
        });
        typed_alphanum
    }

    pub fn replace_columns(
        &mut self,
        search_replace_response: SearchReplaceResponse,
        array_response: &mut ArrayResponse,
    ) {
        // let start = std::time::Instant::now();
        if let Some(ref columns) = search_replace_response.selected_column {
            for column in columns {
                self.columns_filter.remove(column.name.as_str());
            }
        }
        let mut occurrences = replace_occurrences(&mut self.nodes, search_replace_response);
        if self.is_sub_table || occurrences.len() < 100 {
            for (flat_json_value, row_index) in occurrences {
                self.edit_cell(array_response, flat_json_value, row_index);
            }
        } else {
            for (flat_json_value, row_index) in occurrences.iter() {
                self.update_sub_tables_value(flat_json_value, *row_index);
            }
            let json_array = mem::take(&mut self.nodes);
            let mut len = json_array.len();
            let new_json_array = Arc::new(Mutex::new(json_array));

            if len < 8 {
                len = 8;
            }
            let chunks = occurrences.par_chunks_mut(len / 8);
            chunks.into_par_iter().for_each(|chunk| {
                for (updated_entry, row_index) in chunk {
                    let mut json_array_entry = {
                        let mut new_json_array_guard = new_json_array.lock().unwrap();
                        mem::take(&mut new_json_array_guard[*row_index].entries)
                    };
                    Self::update_row(
                        &mut json_array_entry,
                        mem::take(updated_entry),
                        self.is_sub_table,
                        self.last_parsed_max_depth,
                    );
                    let mut new_json_array_guard = new_json_array.lock().unwrap();
                    new_json_array_guard[*row_index].entries = json_array_entry;
                }
            });
            let mut new_json_array_guard = new_json_array.lock().unwrap();
            self.nodes = mem::take(&mut new_json_array_guard);
            self.cache.borrow_mut().evict();
        }
        // println!("took {}ms to update columns", start.elapsed().as_millis());
        self.do_filter_column();
    }

    pub fn open_replace_panel(&mut self, selected_column: Option<Column<'array>>) {
        set_open(&mut self.opened_windows, PANEL_REPLACE, true);
        if let Some(selected_column) = selected_column {
            self.search_replace_panel.set_select_column(selected_column);
        }
        if self.is_sub_table {
            self.search_replace_panel
                .set_title(format!("Replace in {}", self.parent_pointer.pointer));
        }
        self.search_replace_panel
            .set_columns(self.all_columns().clone());
    }
}
