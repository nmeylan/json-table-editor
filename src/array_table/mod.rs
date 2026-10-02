mod cell_lookup;
mod header;
mod row_view;
pub mod table_source;
use table_source::{CompactRows, TableSource};

use crate::components::cell_text::CellText;
use crate::components::icon::ButtonWithIcon;
use crate::components::table::{CellLocation, TableBody};
use crate::fonts::{COPY, FILTER, PENCIL, PLUS, TABLE, TABLE_CELLS};
use crate::panels::{SearchReplacePanel, SearchReplaceResponse, PANEL_REPLACE};
use crate::parser::{column_id, replace_occurrences, row_number_entry, search_occurrences};
use crate::subtable_window::SubTable;
use row_view::{ColumnFilter, RowView};
use crate::{
    concat_string, set_open, ArrayResponse, Window, SHORTCUT_COPY, SHORTCUT_DELETE,
    SHORTCUT_REPLACE,
};
use eframe::egui::scroll_area::ScrollBarVisibility;
use eframe::egui::style::Spacing;
use eframe::egui::{
    Align, Context, CursorIcon, Id, Key, Label, Sense, Style, TextEdit, Ui, Vec2, Widget,
};
use eframe::epaint::text::TextWrapMode;
use egui::{EventFilter, InputState, Modifiers, Rangef, TextBuffer};
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
use std::collections::{BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Sub;
use std::string::ToString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Estimated width of a character, used to size columns from their name
pub(crate) const TEXT_WIDTH: f32 = 7.0;
const ROW_NUMBER_COLUMN_WIDTH: f32 = 40.0;
const LAST_COLUMN_MIN_WIDTH: f32 = 240.0;

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
    // Read only tables keep rows as positions in json instead of nodes
    compact: Option<CompactRows>,
    pub editable: bool,
    row_view: RowView,
    scroll_y: f32,
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
            row_view: RowView::new(nodes.len()),
            nodes,
            compact: None,
            editable: true,
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
    /// Read only table, parsed at full depth
    pub fn new_read_only(
        parse_result: Option<ParseResult<String>>,
        rows: CompactRows,
        all_columns: Vec<Column<'array>>,
        depth: u8,
        parent_pointer: PointerKey,
    ) -> Self {
        let mut table = Self::new(parse_result, vec![], all_columns, depth, parent_pointer);
        table.row_view = RowView::new(rows.rows_count());
        table.compact = Some(rows);
        table.editable = false;
        table
    }

    pub fn source(&self) -> &dyn TableSource {
        match self.compact {
            Some(ref rows) => rows,
            None => &self.nodes,
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
        let editable = self.editable;
        for updated_value in updated_values.into_iter().filter(|_| editable) {
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
            self.row_view.rows_changed();
            self.refresh_row_view();
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

        self.draw_table(ui, text_height, TEXT_WIDTH, pinned)
    }

    #[inline]
    fn initial_column_width(name: &str, text_width: f32) -> f32 {
        (name.len() + 3).max(10) as f32 * text_width
    }

    /// Width needed to show all columns at their initial width
    pub fn content_width(&self, spacing: &Spacing) -> f32 {
        let last = self.column_selected.len().saturating_sub(1);
        let selected = self.column_selected.iter().enumerate().map(|(i, column)| {
            let width = Self::initial_column_width(&column.name, TEXT_WIDTH);
            if i == last && self.column_selected.len() > 3 {
                width.max(LAST_COLUMN_MIN_WIDTH)
            } else {
                width
            }
        });
        // First pinned column is the row number
        let pinned = self
            .column_pinned
            .iter()
            .skip(1)
            .map(|column| Self::initial_column_width(&column.name, TEXT_WIDTH));
        let columns_count = self.column_pinned.len() + self.column_selected.len();
        ROW_NUMBER_COLUMN_WIDTH
            + pinned.chain(selected).sum::<f32>()
            + columns_count as f32 * spacing.item_spacing.x
            + spacing.scroll.allocated_width()
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
                    // Typed number is the "#" column value (data index): rows hidden by a filter are not reachable
                    let view_index = match self.scroll_to_row.parse::<usize>() {
                        Ok(data_index) => self.row_view.data_to_view(data_index),
                        Err(_) => {
                            self.scroll_to_row.clear();
                            Some(0)
                        }
                    };
                    if let Some(view_index) = view_index {
                        table = table.scroll_to_row(view_index, Some(Align::TOP));
                    }
                }
                ScrollToRowMode::MatchingTerm => {
                    if changed_scroll_to_row_value.elapsed().as_millis() >= 300 {
                        self.changed_scroll_to_row_value = None;
                        if !self.scroll_to_row.is_empty() {
                            self.search_matching_rows();
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
                    table = table.column(Column::initial(ROW_NUMBER_COLUMN_WIDTH).clip(true).resizable(true));
                } else {
                    table = table.column(Column::remainder().clip(true).resizable(true));
                }
            }
        } else {
            for i in 0..columns_count {
                if pinned_column_table && i == 0 {
                    table = table.column(Column::initial(ROW_NUMBER_COLUMN_WIDTH).clip(true).resizable(true));
                } else if i == columns_count - 1 {
                    table = table.column(Column::remainder().clip(false).resizable(true).range(Rangef::new(LAST_COLUMN_MIN_WIDTH, f32::INFINITY)));
                } else {
                    table = table.column(
                        Column::initial(Self::initial_column_width(&columns[i].name, text_width))
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

    fn body(
        &mut self,
        text_height: f32,
        pinned_column_table: bool,
        array_response: &mut ArrayResponse,
        mut request_repaint: bool,
        body: TableBody,
    ) {
        if self.compact.is_some() {
            self.body_read_only(text_height, pinned_column_table, array_response, body);
            return;
        }
        // Mutation after interaction
        let mut subtable = None;
        let mut focused_cell = None;
        let mut focused_changed = false;
        let mut updated_value: Option<(PointerKey, String)> = None;
        let mut filter_by_value: Option<(String, String)> = None; // col name, value
        let mut insert_row_at_index: Option<(usize, u8)> = None; // table_row_index, 0 = above, 1 = below
        let columns = self.columns(pinned_column_table);
        let hover_data = body.rows(text_height, self.row_view.len(), |mut row| {
            let table_row_index = row.index();
            let row_index = self.row_view.view_to_data(table_row_index);
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
                        } else if let Some(value) = self.cell_value(row_data, index) {
                            if !matches!(entry.pointer.value_type, ValueType::Null) {
                                let label = if value.len() > 1000 {
                                    CellText::new(&value[0..1000])
                                } else {
                                    CellText::new(&*value)
                                };

                                let mut response = label.ui(ui, cell_id);

                                if self.editable && response.double_clicked() {
                                    *self.editing_value.borrow_mut() = value.to_string();
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
                                            ui.label(&*value).request_focus();
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
                    if self.editable && response.double_clicked() {
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
                    if table_row_index < self.row_view.len() {
                        let row_index = self.row_view.view_to_data(table_row_index);
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
                                if let Some(value) = self.cell_value(row_data, index) {
                                    edit_value = value.to_string();
                                }
                                edit_entry = Some(entry);
                            }
                            // Context menu: edit
                            let button = ButtonWithIcon::new("Edit", PENCIL);
                            if self.editable && ui.add(button).clicked() {
                                *self.editing_index.borrow_mut() =
                                    Some((col_index, row_index, pinned_column_table));
                                *self.editing_value.borrow_mut() = mem::take(&mut edit_value);
                                ui.close();
                            }
                            if !edit_value.is_empty() {
                                // Context menu: copy
                                let button = ButtonWithIcon::new("Copy", COPY)
                                    .shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_COPY));
                                if ui.add(button).clicked() {
                                    ui.ctx().copy_text(edit_value.clone());
                                    ui.close();
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
                                        ui.close();
                                    }
                                }
                            }
                            ui.separator();
                            // Context menu: insert row above
                            let button = ButtonWithIcon::new("Insert row above", PLUS);
                            if self.editable && ui.add(button).clicked() {
                                insert_row_at_index = Some((table_row_index, 0));
                                ui.close();
                            }

                            // Context menu: insert row below
                            let button = ButtonWithIcon::new("Insert row below", PLUS);
                            if self.editable && ui.add(button).clicked() {
                                insert_row_at_index = Some((table_row_index, 1));
                                ui.close();
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
                                        ui.close();
                                        let content = edit_value.clone();
                                        subtable = self.open_subtable(row_index, &entry.pointer, content);
                                    }
                                }
                            }

                            // Context menu: Open row in subtable
                            if !self.is_sub_table {
                                ui.separator();
                                let button = ButtonWithIcon::new("Open row in sub table", TABLE);
                                if ui.add(button).clicked() {
                                    ui.close();
                                    let root_node = row_data.entries.last().unwrap();
                                    subtable = Some(SubTable::new(
                                        root_node.pointer.clone(),
                                        root_node.value.as_ref().unwrap().clone(),
                                        ValueType::Object(true, 0),
                                        row_index,
                                        root_node.pointer.depth,
                                        self.editable,
                                    ));
                                }
                            }
                            // Context menu: Open copy pointer
                            if let Some(entry) = edit_entry {
                                ui.separator();
                                if ui.button("Copy pointer").clicked() {
                                    ui.ctx().copy_text(entry.pointer.pointer.clone());
                                    ui.close();
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
            self.row_view.set_filter(
                column_name,
                Some(ColumnFilter::Include(HashSet::from([filter_value]))),
            );
            self.do_filter_column();
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

    /// Body of a read only table: cells come from its source, no edition
    fn body_read_only(
        &mut self,
        text_height: f32,
        pinned_column_table: bool,
        array_response: &mut ArrayResponse,
        body: TableBody,
    ) {
        // Mutation after interaction
        let mut subtable = None;
        let mut focused_cell = None;
        let mut focused_changed = false;
        let mut filter_by_value: Option<(String, String)> = None; // col name, value
        let columns = self.columns(pinned_column_table);
        let source = self.source();
        let hover_data = body.rows(text_height, self.row_view.len(), |mut row| {
            let table_row_index = row.index();
            let row_index = self.row_view.view_to_data(table_row_index);
            row.cols(false, |ui, col_index| {
                if pinned_column_table && col_index == 0 {
                    return Some(Label::new(row_index.to_string()).ui(ui));
                }
                let cell_id = row_index * columns.len()
                    + col_index
                    + if pinned_column_table {
                        self.seed1
                    } else {
                        self.seed2
                    };
                let value = source
                    .cell(row_index, columns[col_index].id)
                    .and_then(|cell| cell.value.filter(|_| !matches!(cell.value_type, ValueType::Null)));
                let response = match value {
                    Some(value) => {
                        let label = CellText::new(if value.len() > 1000 { &value[0..1000] } else { value });
                        let response = label.ui(ui, cell_id);
                        if value.len() > 100 {
                            response.on_hover_ui(|ui| {
                                ui.style_mut().interaction.selectable_labels = true;
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.label(value).request_focus();
                                });
                            })
                        } else {
                            response
                        }
                    }
                    None => {
                        let rect = ui.available_rect_before_wrap();
                        ui.interact(rect, Id::new(cell_id), Sense::click())
                    }
                };
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
                Some(response)
            });
        });
        // Context menu
        if let (Some(hover_cell), Some(response)) = (hover_data.hovered_cell.as_ref(), hover_data.response_rows.as_ref()) {
            response.context_menu(|ui| {
                let table_row_index = hover_cell.row_index;
                let col_index = hover_cell.column_index;
                if table_row_index >= self.row_view.len() {
                    return;
                }
                let row_index = self.row_view.view_to_data(table_row_index);
                let column = &columns[col_index];
                let cell = source.cell(row_index, column.id);
                let pointer = PointerKey {
                    pointer: Self::pointer_key(&self.parent_pointer.pointer, row_index, &column.name),
                    value_type: cell.as_ref().map_or(column.value_type, |cell| cell.value_type),
                    depth: column.depth,
                    position: 0,
                    column_id: column.id,
                };
                if let Some(value) = cell.as_ref().and_then(|cell| cell.value) {
                    let button = ButtonWithIcon::new("Copy", COPY)
                        .shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_COPY));
                    if ui.add(button).clicked() {
                        ui.ctx().copy_text(value.to_string());
                        ui.close();
                    }
                    if Self::is_filterable(column) {
                        let button = ButtonWithIcon::new("Filter by this value", FILTER);
                        if ui.add(button).clicked() {
                            filter_by_value = Some((column.name.to_string(), value.to_string()));
                            ui.close();
                        }
                    }
                    let is_array = matches!(pointer.value_type, ValueType::Array(_));
                    let is_object = matches!(pointer.value_type, ValueType::Object(..));
                    if is_array || is_object {
                        ui.separator();
                        let button = ButtonWithIcon::new(
                            format!("Open {} in sub table", if is_array { "array" } else { "object" }),
                            TABLE_CELLS,
                        );
                        if ui.add(button).clicked() {
                            ui.close();
                            subtable = self.open_subtable(row_index, &pointer, value.to_string());
                        }
                    }
                }
                // Row object is the cell of the column without name
                let row_pointer_name = "";
                if let Some(row) = source.cell(row_index, column_id(row_pointer_name)).and_then(|cell| cell.value) {
                    ui.separator();
                    let button = ButtonWithIcon::new("Open row in sub table", TABLE);
                    if ui.add(button).clicked() {
                        ui.close();
                        let row_column = self.all_columns.iter().find(|column| column.name == row_pointer_name);
                        subtable = Some(SubTable::new(
                            PointerKey {
                                pointer: Self::pointer_key(&self.parent_pointer.pointer, row_index, row_pointer_name),
                                value_type: ValueType::Object(true, 0),
                                depth: row_column.map_or(1, |column| column.depth),
                                position: 0,
                                column_id: column_id(row_pointer_name),
                            },
                            row.to_string(),
                            ValueType::Object(true, 0),
                            row_index,
                            row_column.map_or(1, |column| column.depth),
                            false,
                        ));
                    }
                }
                ui.separator();
                if ui.button("Copy pointer").clicked() {
                    ui.ctx().copy_text(pointer.pointer.clone());
                    ui.close();
                }
            });
        }

        if focused_changed {
            self.focused_cell = focused_cell;
        }
        if let Some(subtable) = subtable {
            self.windows.push(subtable);
        }
        if let Some((column_name, filter_value)) = filter_by_value {
            self.row_view.set_filter(
                column_name,
                Some(ColumnFilter::Include(HashSet::from([filter_value]))),
            );
            self.do_filter_column();
        }
        if self.hovered_row_index != hover_data.hovered_row {
            self.hovered_row_index = hover_data.hovered_row;
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
        let row_index = self.row_view.view_to_data(table_row_index);
        let depth = self.nodes[row_index].entries.last().unwrap().pointer.depth;
        let new_table_row_index = table_row_index + above_or_below as usize;
        let new_index = row_index + above_or_below as usize;
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
                            // Row root entry belongs to the "" column, as in parser::as_array
                            column_id: column_id(""),
                        },
                        value: Some("{}".to_string()),
                    },
                ],
                index: new_index,
            },
        );
        self.row_view.on_row_inserted(new_table_row_index, new_index);
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

    #[inline]
    fn is_filterable(column: &Column) -> bool {
        !(matches!(column.value_type, ValueType::Object(_, _))
            || matches!(column.value_type, ValueType::Array(_))
            || matches!(column.value_type, ValueType::Null))
    }

    /// Scalar columns only.
    #[inline]
    fn is_sortable(column: &Column) -> bool {
        !matches!(
            column.value_type,
            ValueType::Object(_, _) | ValueType::Array(_)
        )
    }

    fn open_subtable(
        &self,
        row_index: usize,
        pointer: &PointerKey,
        content: String,
    ) -> Option<SubTable<'array>> {
        Some(SubTable::new(
            pointer.clone(),
            content,
            pointer.value_type,
            row_index,
            pointer.depth,
            self.editable,
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
            self.row_view.rows_changed();
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
                    // Objects kept without raw data have their value computed from their content
                    row_entries
                        .iter_mut()
                        .find(|e| e.pointer.pointer.eq(&newly_updated_value.pointer.pointer))
                        .filter(|entry_to_update| entry_to_update.value.is_some())
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

    fn do_filter_column(&mut self) {
        self.refresh_row_view();
        self.next_frame_reset_scroll = true;
    }

    fn refresh_row_view(&mut self) {
        let source: &dyn TableSource = match self.compact {
            Some(ref rows) => rows,
            None => &self.nodes,
        };
        self.row_view.recompute(source);
        // Matching rows are view indices: they are stale once the view changed
        if !self.matching_rows.is_empty() {
            self.search_matching_rows();
            self.matching_row_selected = self
                .matching_row_selected
                .min(self.matching_rows.len().saturating_sub(1));
        }
    }

    /// Search occurrences of `scroll_to_row` among visible rows, as view indices.
    fn search_matching_rows(&mut self) {
        self.matching_rows =
            search_occurrences(self.source(), &self.scroll_to_row.to_lowercase())
                .into_iter()
                .filter_map(|data_index| self.row_view.data_to_view(data_index))
                .collect();
        // Navigate occurrences in view order
        self.matching_rows.sort_unstable();
    }

    #[inline]
    pub fn row_view(&self) -> &RowView {
        &self.row_view
    }

    pub fn remove_filter(&mut self, column: &str) {
        self.row_view.remove_filter(column);
        self.do_filter_column();
    }

    pub fn clear_filters(&mut self) {
        self.row_view.clear_filters();
        self.do_filter_column();
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
                            && focused_cell.row_index < self.row_view.len() - 1
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
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) && focused_cell.row_index < self.row_view.len() - 1 {
                        focused_cell.row_index += 1;
                        self.scroll_to_row_number = focused_cell.row_index;
                        self.changed_arrow_vertical_scroll = true;
                    }
                    let typed_alphanum = Self::get_typed_alphanum_from_events(i);
                    if (typed_alphanum.is_some() || i.consume_key(Modifiers::NONE, Key::Enter))
                        && !self.was_editing
                        && self.editable
                    {
                        let row_index = self.row_view.view_to_data(focused_cell.row_index);
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
                if self.editable && i.consume_shortcut(&SHORTCUT_REPLACE) {
                    self.open_replace_panel(None);
                }
            }
            let hovered_cell = array_response.hover_data.hovered_cell;
            let editable = self.editable;
            for event in i.events.iter().filter(|e| match e {
                egui::Event::Copy => hovered_cell.is_some(),
                egui::Event::Paste(_) => hovered_cell.is_some() && editable,
                egui::Event::Key {
                    key: Key::Delete, ..
                } => hovered_cell.is_some() && editable,
                _ => false,
            }) {
                let cell_location = hovered_cell.unwrap();
                let row_index = self.row_view.view_to_data(cell_location.row_index);
                if let Some(ref rows) = self.compact {
                    // Read only: only copy
                    if matches!(event, egui::Event::Copy) {
                        let columns = self.columns(cell_location.is_pinned_column_table);
                        copied_value = columns
                            .get(cell_location.column_index)
                            .and_then(|column| rows.cell(row_index, column.id))
                            .and_then(|cell| cell.value)
                            .map(str::to_string);
                    }
                    continue;
                }
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
                            if let Some(value) = self.cell_value(&self.nodes[row_index], index) {
                                copied_value = Some(value.to_string());
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
                self.row_view.remove_filter(column.name.as_str());
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
            self.row_view.rows_changed();
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
