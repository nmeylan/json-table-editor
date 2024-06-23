/// Heavily inspired from egui codebase
///
/// Credit egui_extras: https://github.com/emilk/egui
/// Modifications are:
/// - Do not render non visibile columns
/// - Optimization for empty cell


/// Size hint for table column/strip cell.
#[derive(Clone, Debug, Copy)]
pub enum Size {
    /// Absolute size in points, with a given range of allowed sizes to resize within.
    Absolute { initial: f32, range: Rangef },

    /// Relative size relative to all available space.
    Relative { fraction: f32, range: Rangef },

    /// Multiple remainders each get the same space.
    Remainder { range: Rangef },
}

impl Size {
    /// Exactly this big, with no room for resize.
    pub fn exact(points: f32) -> Self {
        Self::Absolute {
            initial: points,
            range: Rangef::new(points, points),
        }
    }

    /// Initially this big, but can resize.
    pub fn initial(points: f32) -> Self {
        Self::Absolute {
            initial: points,
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    /// Relative size relative to all available space. Values must be in range `0.0..=1.0`.
    pub fn relative(fraction: f32) -> Self {
        egui::egui_assert!((0.0..=1.0).contains(&fraction));
        Self::Relative {
            fraction,
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    /// Multiple remainders each get the same space.
    pub fn remainder() -> Self {
        Self::Remainder {
            range: Rangef::new(0.0, f32::INFINITY),
        }
    }

    /// Won't shrink below this size (in points).
    #[inline]
    pub fn at_least(mut self, minimum: f32) -> Self {
        match &mut self {
            Self::Absolute { range, .. }
            | Self::Relative { range, .. }
            | Self::Remainder { range, .. } => {
                range.min = minimum;
            }
        }
        self
    }

    /// Won't grow above this size (in points).
    #[inline]
    pub fn at_most(mut self, maximum: f32) -> Self {
        match &mut self {
            Self::Absolute { range, .. }
            | Self::Relative { range, .. }
            | Self::Remainder { range, .. } => {
                range.max = maximum;
            }
        }
        self
    }

    /// Allowed range of movement (in points), if in a resizable [`Table`](crate::array_table::ArrayTable).
    pub fn range(self) -> Rangef {
        match self {
            Self::Absolute { range, .. }
            | Self::Relative { range, .. }
            | Self::Remainder { range, .. } => range,
        }
    }
}

#[derive(Clone, Default)]
pub struct Sizing {
    pub(crate) sizes: Vec<Size>,
}

impl Sizing {
    pub fn add(&mut self, size: Size) {
        self.sizes.push(size);
    }

    pub fn to_lengths(&self, length: f32, spacing: f32) -> Vec<f32> {
        if self.sizes.is_empty() {
            return vec![];
        }

        let mut remainders = 0;
        let sum_non_remainder = self
            .sizes
            .iter()
            .map(|&size| match size {
                Size::Absolute { initial, .. } => initial,
                Size::Relative { fraction, range } => {
                    assert!((0.0..=1.0).contains(&fraction));
                    range.clamp(length * fraction)
                }
                Size::Remainder { .. } => {
                    remainders += 1;
                    0.0
                }
            })
            .sum::<f32>()
            + spacing * (self.sizes.len() - 1) as f32;

        let avg_remainder_length = if remainders == 0 {
            0.0
        } else {
            let mut remainder_length = length - sum_non_remainder;
            let avg_remainder_length = 0.0f32.max(remainder_length / remainders as f32).floor();
            self.sizes.iter().for_each(|&size| {
                if let Size::Remainder { range } = size {
                    if avg_remainder_length < range.min {
                        remainder_length -= range.min;
                        remainders -= 1;
                    }
                }
            });
            if remainders > 0 {
                0.0f32.max(remainder_length / remainders as f32)
            } else {
                0.0
            }
        };

        self.sizes
            .iter()
            .map(|&size| match size {
                Size::Absolute { initial, .. } => initial,
                Size::Relative { fraction, range } => range.clamp(length * fraction),
                Size::Remainder { range } => range.clamp(avg_remainder_length),
            })
            .collect()
    }
}

impl From<Vec<Size>> for Sizing {
    fn from(sizes: Vec<Size>) -> Self {
        Self { sizes }
    }
}

// Table view with (optional) fixed header and scrolling body.
// Cell widths are precalculated with given size hints so we can have tables like this:
// | fixed size | all available space/minimum | 30% of available width | fixed size |
// Takes all available height, so if you want something below the table, put it in a strip.


use egui::{scroll_area::ScrollBarVisibility, Align, NumExt as _, Rangef, Rect, Response, ScrollArea, Ui, Vec2, Vec2b, Pos2, Sense, Id, Widget, Color32};
use egui::scroll_area::ScrollAreaOutput;

#[derive(Clone, Copy)]
pub(crate) enum CellSize {
    /// Absolute size in points
    Absolute(f32),

    /// Take all available space
    Remainder,
}

/// Cells are positioned in two dimensions, cells go in one direction and form lines.
///
/// In a strip there's only one line which goes in the direction of the strip:
///
/// In a horizontal strip, a [`StripLayout`] with horizontal [`CellDirection`] is used.
/// Its cells go from left to right inside this [`StripLayout`].
///
/// In a table there's a [`StripLayout`] for each table row with a horizontal [`CellDirection`].
/// Its cells go from left to right. And the lines go from top to bottom.
pub(crate) enum CellDirection {
    /// Cells go from left to right.
    Horizontal,

    /// Cells go from top to bottom.
    Vertical,
}

/// Flags used by [`StripLayout::add`].
#[derive(Clone, Copy, Default)]
pub(crate) struct StripLayoutFlags {
    pub(crate) clip: bool,
    pub(crate) striped: bool,
    pub(crate) hovered: bool,
    pub(crate) selected: bool,
    pub(crate) highlighted: bool,
}

/// Positions cells in [`CellDirection`] and starts a new line on [`StripLayout::end_line`]
pub struct StripLayout<'l> {
    pub(crate) ui: &'l mut Ui,
    direction: CellDirection,
    pub(crate) rect: Rect,
    pub(crate) cursor: Pos2,

    /// Keeps track of the max used position,
    /// so we know how much space we used.
    max: Pos2,

    cell_layout: egui::Layout,
    sense: Sense,
}

impl<'l> StripLayout<'l> {
    pub(crate) fn new(
        ui: &'l mut Ui,
        direction: CellDirection,
        cell_layout: egui::Layout,
        sense: Sense,
    ) -> Self {
        let rect = ui.available_rect_before_wrap();
        let pos = rect.left_top();

        Self {
            ui,
            direction,
            rect,
            cursor: pos,
            max: pos,
            cell_layout,
            sense,
        }
    }

    fn cell_rect(&self, width: &CellSize, height: &CellSize) -> Rect {
        Rect {
            min: self.cursor,
            max: Pos2 {
                x: match width {
                    CellSize::Absolute(width) => self.cursor.x + width,
                    CellSize::Remainder => self.rect.right(),
                },
                y: match height {
                    CellSize::Absolute(height) => self.cursor.y + height,
                    CellSize::Remainder => self.rect.bottom(),
                },
            },
        }
    }

    fn set_pos(&mut self, rect: Rect) {
        self.max.x = self.max.x.max(rect.right());
        self.max.y = self.max.y.max(rect.bottom());

        match self.direction {
            CellDirection::Horizontal => {
                self.cursor.x = rect.right() + self.ui.spacing().item_spacing.x;
            }
            CellDirection::Vertical => {
                self.cursor.y = rect.bottom() + self.ui.spacing().item_spacing.y;
            }
        }
    }

    pub(crate) fn empty(&mut self, width: CellSize, height: CellSize) {
        self.set_pos(self.cell_rect(&width, &height));
    }

    /// This is the innermost part of [`crate::ArrayTable`] and [`crate::Strip`].
    ///
    /// Return the used space (`min_rect`) plus the [`Response`] of the whole cell.
    pub(crate) fn add(
        &mut self,
        flags: StripLayoutFlags,
        width: CellSize,
        height: CellSize,
        child_ui_id_source: Id,
        cell_index: usize,
        add_cell_contents: Option<impl FnOnce(&mut Ui, usize) -> Option<Response>>,
    ) -> (Rect, Response) {
        let max_rect = self.cell_rect(&width, &height);

        // Make sure we don't have a gap in the stripe/frame/selection background:
        let item_spacing = self.ui.spacing().item_spacing;
        let gapless_rect = max_rect.expand2(0.5 * item_spacing);

        if flags.striped {
            self.ui.painter().rect_filled(
                gapless_rect,
                egui::Rounding::ZERO,
                self.ui.visuals().faint_bg_color,
            );
        }

        if flags.selected {
            self.ui.painter().rect_filled(
                gapless_rect,
                egui::Rounding::ZERO,
                self.ui.visuals().selection.bg_fill,
            );
        }

        if flags.hovered && !flags.selected && self.sense.interactive() {
            self.ui.painter().rect_filled(
                gapless_rect,
                egui::Rounding::ZERO,
                self.ui.visuals().widgets.hovered.bg_fill,
            );
        }
        if flags.highlighted && !flags.hovered {
            self.ui.painter().rect_filled(
                gapless_rect,
                egui::Rounding::ZERO,
                Color32::YELLOW,
            );
        }

        let (child_ui, child_response) = self.cell(flags, max_rect, child_ui_id_source, cell_index, add_cell_contents);

        let used_rect = child_ui.min_rect();

        self.set_pos(max_rect);

        let allocation_rect = if flags.clip {
            max_rect
        } else {
            max_rect.union(used_rect)
        };

        self.ui.advance_cursor_after_rect(allocation_rect);

        let mut response = child_ui.interact(max_rect, child_ui.id(), self.sense);

        if let Some(child_response) = child_response {
            response = response.union(child_response);
        }
        (used_rect, response)
    }

    /// This is the innermost part of [`crate::ArrayTable`] and [`crate::Strip`].
    ///
    /// Return the used space (`min_rect`) plus the [`Response`] of the whole cell.
    pub(crate) fn add_empty(
        &mut self,
        width: CellSize,
        height: CellSize,
        color: Color32,
    ) -> Rect {
        let max_rect = self.cell_rect(&width, &height);

        // Make sure we don't have a gap in the stripe/frame/selection background:
        let item_spacing = self.ui.spacing().item_spacing;
        let gapless_rect = max_rect.expand2(0.5 * item_spacing);

        self.ui.painter().rect_filled(
            gapless_rect,
            egui::Rounding::ZERO,
            color,
        );

        self.set_pos(max_rect);

        let allocation_rect = max_rect;

        self.ui.advance_cursor_after_rect(allocation_rect);


        max_rect
    }

    /// only needed for layouts with multiple lines, like [`Table`](crate::ArrayTable).
    pub fn end_line(&mut self) {
        match self.direction {
            CellDirection::Horizontal => {
                self.cursor.y = self.max.y + self.ui.spacing().item_spacing.y;
                self.cursor.x = self.rect.left();
            }
            CellDirection::Vertical => {
                self.cursor.x = self.max.x + self.ui.spacing().item_spacing.x;
                self.cursor.y = self.rect.top();
            }
        }
    }

    /// Skip a lot of space.
    pub(crate) fn skip_space(&mut self, delta: egui::Vec2) {
        let before = self.cursor;
        self.cursor += delta;
        let rect = Rect::from_two_pos(before, self.cursor);
        self.ui.allocate_rect(rect, Sense::hover());
    }

    /// Return the Ui to which the contents where added
    fn cell(
        &mut self,
        flags: StripLayoutFlags,
        rect: Rect,
        child_ui_id_source: egui::Id,
        cell_index: usize,
        add_cell_contents: Option<impl FnOnce(&mut Ui, usize) -> Option<Response>>,
    ) -> (Ui, Option<Response>) {
        let mut child_ui =
            self.ui
                .child_ui_with_id_source(rect, self.cell_layout, child_ui_id_source);

        if flags.clip {
            let margin = egui::Vec2::splat(self.ui.visuals().clip_rect_margin);
            let margin = margin.min(0.5 * self.ui.spacing().item_spacing);
            let clip_rect = rect.expand2(margin);
            child_ui.set_clip_rect(clip_rect.intersect(child_ui.clip_rect()));
        }

        if flags.selected {
            let stroke_color = child_ui.style().visuals.selection.stroke.color;
            child_ui.style_mut().visuals.override_text_color = Some(stroke_color);
        }

        let response = if let Some(add_cell_contents) = add_cell_contents {
            
            add_cell_contents(&mut child_ui, cell_index)
        } else {
            None
        };
        (child_ui, response)
    }

    /// Allocate the rect in [`Self::ui`] so that the scrollview knows about our size
    pub fn allocate_rect(&mut self) -> Response {
        let mut rect = self.rect;
        rect.set_right(self.max.x);
        rect.set_bottom(self.max.y);

        self.ui.allocate_rect(rect, Sense::hover())
    }
}
// -----------------------------------------------------------------=----------

#[derive(Clone, Copy, Debug, PartialEq)]
enum InitialColumnSize {
    /// Absolute size in points
    Absolute(f32),

    /// Base on content
    Automatic(f32),

    /// Take all available space
    Remainder,
}

/// Specifies the properties of a column, like its width range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    initial_width: InitialColumnSize,

    width_range: Rangef,

    /// Clip contents if too narrow?
    clip: bool,

    resizable: Option<bool>,
}

impl Column {
    /// Automatically sized based on content.
    ///
    /// If you have many thousands of rows and are therefore using [`TableBody::rows`]
    /// or [`TableBody::heterogeneous_rows`], then the automatic size will only be based
    /// on the currently visible rows.
    pub fn auto() -> Self {
        Self::auto_with_initial_suggestion(100.0)
    }

    /// Automatically sized.
    ///
    /// The given fallback is a loose suggestion, that may be used to wrap
    /// cell contents, if they contain a wrapping layout.
    /// In most cases though, the given value is ignored.
    pub fn auto_with_initial_suggestion(suggested_width: f32) -> Self {
        Self::new(InitialColumnSize::Automatic(suggested_width))
    }

    /// With this initial width.
    pub fn initial(width: f32) -> Self {
        Self::new(InitialColumnSize::Absolute(width))
    }

    /// Always this exact width, never shrink or grow.
    pub fn exact(width: f32) -> Self {
        Self::new(InitialColumnSize::Absolute(width))
            .range(width..=width)
            .clip(true)
    }

    /// Take all the space remaining after the other columns have
    /// been sized.
    ///
    /// If you have multiple [`Column::remainder`] they all
    /// share the remaining space equally.
    pub fn remainder() -> Self {
        Self::new(InitialColumnSize::Remainder)
    }

    fn new(initial_width: InitialColumnSize) -> Self {
        Self {
            initial_width,
            width_range: Rangef::new(0.0, f32::INFINITY),
            resizable: None,
            clip: false,
        }
    }

    /// Can this column be resized by dragging the column separator?
    ///
    /// If you don't call this, the fallback value of
    /// [`TableBuilder::resizable`] is used (which by default is `false`).
    #[inline]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = Some(resizable);
        self
    }

    /// If `true`: Allow the column to shrink enough to clip the contents.
    /// If `false`: The column will always be wide enough to contain all its content.
    ///
    /// Clipping can make sense if you expect a column to contain a lot of things,
    /// and you don't want it too take up too much space.
    /// If you turn on clipping you should also consider calling [`Self::at_least`].
    ///
    /// Default: `false`.
    #[inline]
    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    /// Won't shrink below this width (in points).
    ///
    /// Default: 0.0
    #[inline]
    pub fn at_least(mut self, minimum: f32) -> Self {
        self.width_range.min = minimum;
        self
    }

    /// Won't grow above this width (in points).
    ///
    /// Default: [`f32::INFINITY`]
    #[inline]
    pub fn at_most(mut self, maximum: f32) -> Self {
        self.width_range.max = maximum;
        self
    }

    /// Allowed range of movement (in points), if in a resizable [`Table`].
    #[inline]
    pub fn range(mut self, range: impl Into<Rangef>) -> Self {
        self.width_range = range.into();
        self
    }

    fn is_auto(&self) -> bool {
        match self.initial_width {
            InitialColumnSize::Automatic(_) => true,
            InitialColumnSize::Absolute(_) | InitialColumnSize::Remainder => false,
        }
    }
}

fn to_sizing(columns: &[Column]) -> Sizing {
    let mut sizing = Sizing::default();
    for column in columns {
        let size = match column.initial_width {
            InitialColumnSize::Absolute(width) => Size::exact(width),
            InitialColumnSize::Automatic(suggested_width) => Size::initial(suggested_width),
            InitialColumnSize::Remainder => Size::remainder(),
        }
            .at_least(column.width_range.min)
            .at_most(column.width_range.max);
        sizing.add(size);
    }
    sizing
}

// -----------------------------------------------------------------=----------

struct TableScrollOptions {
    vscroll: bool,
    drag_to_scroll: bool,
    stick_to_bottom: bool,
    scroll_to_row: Option<(usize, Option<Align>)>,
    scroll_offset_y: Option<f32>,
    min_scrolled_height: f32,
    max_scroll_height: f32,
    auto_shrink: Vec2b,
    scroll_bar_visibility: ScrollBarVisibility,
}

impl Default for TableScrollOptions {
    fn default() -> Self {
        Self {
            vscroll: true,
            drag_to_scroll: true,
            stick_to_bottom: false,
            scroll_to_row: None,
            scroll_offset_y: None,
            min_scrolled_height: 200.0,
            max_scroll_height: 800.0,
            auto_shrink: Vec2b::TRUE,
            scroll_bar_visibility: ScrollBarVisibility::VisibleWhenNeeded,
        }
    }
}

// -----------------------------------------------------------------=----------

/// Builder for a [`Table`] with (optional) fixed header and scrolling body.
///
/// You must pre-allocate all columns with [`Self::column`]/[`Self::columns`].
///
/// If you have multiple [`Table`]:s in the same [`Ui`]
/// you will need to give them unique id:s by surrounding them with [`Ui::push_id`].
///
/// ### Example
/// ```
/// # egui::__run_test_ui(|ui| {
/// use egui_extras::{TableBuilder, Column};
/// TableBuilder::new(ui)
///     .column(Column::auto().resizable(true))
///     .column(Column::remainder())
///     .header(20.0, |mut header| {
///         header.col(|ui| {
///             ui.heading("First column");
///         });
///         header.col(|ui| {
///             ui.heading("Second column");
///         });
///     })
///     .body(|mut body| {
///         body.row(30.0, |mut row| {
///             row.col(|ui| {
///                 ui.label("Hello");
///             });
///             row.col(|ui| {
///                 ui.button("world!");
///             });
///         });
///     });
/// # });
/// ```
pub struct TableBuilder<'a> {
    ui: &'a mut Ui,
    columns: Vec<Column>,
    striped: Option<bool>,
    resizable: bool,
    cell_layout: egui::Layout,
    scroll_options: TableScrollOptions,
    sense: egui::Sense,
}

impl<'a> TableBuilder<'a> {
    pub fn new(ui: &'a mut Ui) -> Self {
        let cell_layout = *ui.layout();
        Self {
            ui,
            columns: Default::default(),
            striped: None,
            resizable: false,
            cell_layout,
            scroll_options: Default::default(),
            sense: egui::Sense::hover(),
        }
    }

    /// Enable striped row background for improved readability.
    ///
    /// Default is whatever is in [`egui::Visuals::striped`].
    #[inline]
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = Some(striped);
        self
    }

    /// What should table cells sense for? (default: [`egui::Sense::hover()`]).
    #[inline]
    pub fn sense(mut self, sense: egui::Sense) -> Self {
        self.sense = sense;
        self
    }

    /// Make the columns resizable by dragging.
    ///
    /// You can set this for individual columns with [`Column::resizable`].
    /// [`Self::resizable`] is used as a fallback for any column for which you don't call
    /// [`Column::resizable`].
    ///
    /// If the _last_ column is [`Column::remainder`], then it won't be resizable
    /// (and instead use up the remainder).
    ///
    /// Default is `false`.
    #[inline]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Enable vertical scrolling in body (default: `true`)
    #[inline]
    pub fn vscroll(mut self, vscroll: bool) -> Self {
        self.scroll_options.vscroll = vscroll;
        self
    }

    /// Enables scrolling the table's contents using mouse drag (default: `true`).
    ///
    /// See [`ScrollArea::drag_to_scroll`] for more.
    #[inline]
    pub fn drag_to_scroll(mut self, drag_to_scroll: bool) -> Self {
        self.scroll_options.drag_to_scroll = drag_to_scroll;
        self
    }

    /// Should the scroll handle stick to the bottom position even as the content size changes
    /// dynamically? The scroll handle remains stuck until manually changed, and will become stuck
    /// once again when repositioned to the bottom. Default: `false`.
    #[inline]
    pub fn stick_to_bottom(mut self, stick: bool) -> Self {
        self.scroll_options.stick_to_bottom = stick;
        self
    }

    /// Set a row to scroll to.
    ///
    /// `align` specifies if the row should be positioned in the top, center, or bottom of the view
    /// (using [`Align::TOP`], [`Align::Center`] or [`Align::BOTTOM`]).
    /// If `align` is `None`, the table will scroll just enough to bring the cursor into view.
    ///
    /// See also: [`Self::vertical_scroll_offset`].
    #[inline]
    pub fn scroll_to_row(mut self, row: usize, align: Option<Align>) -> Self {
        self.scroll_options.scroll_to_row = Some((row, align));
        self
    }

    /// Set the vertical scroll offset position, in points.
    ///
    /// See also: [`Self::scroll_to_row`].
    #[inline]
    pub fn vertical_scroll_offset(mut self, offset: f32) -> Self {
        self.scroll_options.scroll_offset_y = Some(offset);
        self
    }

    /// The minimum height of a vertical scroll area which requires scroll bars.
    ///
    /// The scroll area will only become smaller than this if the content is smaller than this
    /// (and so we don't require scroll bars).
    ///
    /// Default: `200.0`.
    #[inline]
    pub fn min_scrolled_height(mut self, min_scrolled_height: f32) -> Self {
        self.scroll_options.min_scrolled_height = min_scrolled_height;
        self
    }

    /// Don't make the scroll area higher than this (add scroll-bars instead!).
    ///
    /// In other words: add scroll-bars when this height is reached.
    /// Default: `800.0`.
    #[inline]
    pub fn max_scroll_height(mut self, max_scroll_height: f32) -> Self {
        self.scroll_options.max_scroll_height = max_scroll_height;
        self
    }

    /// For each axis (x,y):
    /// * If true, add blank space outside the table, keeping the table small.
    /// * If false, add blank space inside the table, expanding the table to fit the containing ui.
    ///
    /// Default: `true`.
    ///
    /// See [`ScrollArea::auto_shrink`] for more.
    #[inline]
    pub fn auto_shrink(mut self, auto_shrink: impl Into<Vec2b>) -> Self {
        self.scroll_options.auto_shrink = auto_shrink.into();
        self
    }

    /// Set the visibility of both horizontal and vertical scroll bars.
    ///
    /// With `ScrollBarVisibility::VisibleWhenNeeded` (default), the scroll bar will be visible only when needed.
    #[inline]
    pub fn scroll_bar_visibility(mut self, scroll_bar_visibility: ScrollBarVisibility) -> Self {
        self.scroll_options.scroll_bar_visibility = scroll_bar_visibility;
        self
    }

    /// What layout should we use for the individual cells?
    #[inline]
    pub fn cell_layout(mut self, cell_layout: egui::Layout) -> Self {
        self.cell_layout = cell_layout;
        self
    }

    /// Allocate space for one column.
    #[inline]
    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Allocate space for several columns at once.
    #[inline]
    pub fn columns(mut self, column: Column, count: usize) -> Self {
        for _ in 0..count {
            self.columns.push(column);
        }
        self
    }

    fn available_width(&self) -> f32 {
        self.ui.available_rect_before_wrap().width()
            - if self.scroll_options.vscroll {
            self.ui.spacing().scroll.bar_inner_margin
                + self.ui.spacing().scroll.bar_width
                + self.ui.spacing().scroll.bar_outer_margin
        } else {
            0.0
        }
    }

    /// Create a header row which always stays visible and at the top
    pub fn header(self, height: f32, add_header_row: impl FnOnce(TableRow<'_, '_>)) -> Table<'a> {
        let available_width = self.available_width();

        let Self {
            ui,
            columns,
            striped,
            resizable,
            cell_layout,
            scroll_options,
            sense,
        } = self;

        let striped = striped.unwrap_or(ui.visuals().striped);

        let state_id = ui.id().with("__table_state");

        let initial_widths =
            to_sizing(&columns).to_lengths(available_width, ui.spacing().item_spacing.x);
        let mut max_used_widths = vec![0.0; initial_widths.len()];
        let (had_state, state) = TableState::load(ui, initial_widths, state_id);
        let is_first_frame = !had_state;
        let first_frame_auto_size_columns = is_first_frame && columns.iter().any(|c| c.is_auto());

        let table_top = ui.cursor().top();
        let clip_rect = ui.clip_rect();

        let mut x_offset = 0.0;
        // Hide first-frame-jitters when auto-sizing.
        ui.add_visible_ui(!first_frame_auto_size_columns, |ui| {
            let mut layout = StripLayout::new(ui, CellDirection::Horizontal, cell_layout, sense);
            let mut response: Option<Response> = None;

            let end_x = clip_rect.right();
            let start_x = clip_rect.left();
            let scroll_offset_x = start_x - layout.rect.left();
            let mut visible_index = Vec::with_capacity(state.column_widths.len());
            let mut first_col_visible_offset = -layout.ui.spacing().item_spacing[0];
            let mut first_visible_seen = false;
            let mut last_visible_seen = false;
            let mut remainder_with = 0.0;
            for (index, width) in state.column_widths.iter().enumerate() {
                if x_offset + width >= scroll_offset_x && x_offset <= end_x + scroll_offset_x {
                    first_visible_seen = true;
                    visible_index.push(index);
                } else if first_visible_seen && !last_visible_seen {
                    last_visible_seen = true;
                }
                if !first_visible_seen {
                    first_col_visible_offset += width + layout.ui.spacing().item_spacing[0];
                }
                x_offset += width + layout.ui.spacing().item_spacing[0];
                if last_visible_seen {
                    remainder_with += width + layout.ui.spacing().item_spacing[0];
                }
            }

            add_header_row(TableRow {
                layout: &mut layout,
                columns: &columns,
                widths: &state.column_widths,
                visible_columns: visible_index.as_slice(),
                max_used_widths: &mut max_used_widths,
                row_index: 0,
                col_index: 0,
                start_x,
                first_col_visible_offset,
                remainder_with,
                height,
                striped: false,
                hovered: false,
                selected: false,
                response: &mut response,
                highlighted: false,
            });
            layout.allocate_rect();
        });

        Table {
            ui,
            table_top,
            state_id,
            columns,
            available_width,
            state,
            max_used_widths,
            first_frame_auto_size_columns,
            resizable,
            striped,
            cell_layout,
            scroll_options,
            sense,
        }
    }

    /// Create table body without a header row
    pub fn body<F>(self, add_body_contents: F)
        where
            F: for<'b> FnOnce(TableBody<'b>),
    {
        let available_width = self.available_width();

        let Self {
            ui,
            columns,
            striped,
            resizable,
            cell_layout,
            scroll_options,
            sense,
        } = self;

        let striped = striped.unwrap_or(ui.visuals().striped);

        let state_id = ui.id().with("__table_state");

        let initial_widths =
            to_sizing(&columns).to_lengths(available_width, ui.spacing().item_spacing.x);
        let max_used_widths = vec![0.0; initial_widths.len()];
        let (had_state, state) = TableState::load(ui, initial_widths, state_id);
        let is_first_frame = !had_state;
        let first_frame_auto_size_columns = is_first_frame && columns.iter().any(|c| c.is_auto());

        let table_top = ui.cursor().top();

        Table {
            ui,
            table_top,
            state_id,
            columns,
            available_width,
            state,
            max_used_widths,
            first_frame_auto_size_columns,
            resizable,
            striped,
            cell_layout,
            scroll_options,
            sense,
        }
            .body(None, None, add_body_contents);
    }
}

// ----------------------------------------------------------------------------

#[derive(Clone)]
struct TableState {
    column_widths: Vec<f32>,
}

impl TableState {
    /// Returns `true` if it did load.
    fn load(ui: &egui::Ui, default_widths: Vec<f32>, state_id: egui::Id) -> (bool, Self) {
        let rect = Rect::from_min_size(ui.available_rect_before_wrap().min, Vec2::ZERO);
        ui.ctx().check_for_id_clash(state_id, rect, "Table");

        if let Some(state) = ui.data_mut(|d| d.get_persisted::<Self>(state_id)) {
            // make sure that the stored widths aren't out-dated
            if state.column_widths.len() == default_widths.len() {
                return (true, state);
            }
        }

        (
            false,
            Self {
                column_widths: default_widths,
            },
        )
    }

    fn store(self, ui: &egui::Ui, state_id: egui::Id) {
        ui.data_mut(|d| d.insert_persisted(state_id, self));
    }
}

// ----------------------------------------------------------------------------

/// Table struct which can construct a [`TableBody`].
///
/// Is created by [`TableBuilder`] by either calling [`TableBuilder::body`] or after creating a header row with [`TableBuilder::header`].
pub struct Table<'a> {
    ui: &'a mut Ui,
    table_top: f32,
    state_id: egui::Id,
    columns: Vec<Column>,
    available_width: f32,
    state: TableState,

    /// Accumulated maximum used widths for each column.
    max_used_widths: Vec<f32>,

    first_frame_auto_size_columns: bool,
    resizable: bool,
    striped: bool,
    cell_layout: egui::Layout,

    scroll_options: TableScrollOptions,

    sense: egui::Sense,
}

impl<'a> Table<'a> {
    /// Access the contained [`egui::Ui`].
    ///
    /// You can use this to e.g. modify the [`egui::Style`] with [`egui::Ui::style_mut`].
    pub fn ui_mut(&mut self) -> &mut egui::Ui {
        self.ui
    }

    /// Create table body after adding a header row
    pub fn body<F>(self, stored_hovered_row_index: Option<usize>, search_matching_row_index: Option<usize>, add_body_contents: F) -> ScrollAreaOutput<Vec<f32>>
        where
            F: for<'b> FnOnce(TableBody<'b>),
    {
        let Table {
            ui,
            table_top,
            state_id,
            columns,
            resizable,
            mut available_width,
            mut state,
            mut max_used_widths,
            first_frame_auto_size_columns,
            striped,
            cell_layout,
            scroll_options,
            sense,
        } = self;

        let TableScrollOptions {
            vscroll,
            drag_to_scroll,
            stick_to_bottom,
            scroll_to_row,
            scroll_offset_y,
            min_scrolled_height,
            max_scroll_height,
            auto_shrink,
            scroll_bar_visibility,
        } = scroll_options;

        let cursor_position = ui.cursor().min;

        let mut scroll_area = ScrollArea::new([false, vscroll])
            .auto_shrink(true)
            .drag_to_scroll(drag_to_scroll)
            .stick_to_bottom(stick_to_bottom)
            .min_scrolled_height(min_scrolled_height)
            .max_height(max_scroll_height)
            .auto_shrink(auto_shrink)
            .scroll_bar_visibility(scroll_bar_visibility).animated(false);

        if let Some(scroll_offset_y) = scroll_offset_y {
            scroll_area = scroll_area.vertical_scroll_offset(scroll_offset_y);
        }

        let columns_ref = &columns;
        let widths_ref = &state.column_widths;
        let max_used_widths_ref = &mut max_used_widths;

        let number_of_columns = widths_ref.len();

        let scroll_area_output = scroll_area.show(ui, move |ui| {
            let mut columns_offset = Vec::with_capacity(number_of_columns);
            let mut scroll_to_y_range = None;

            let clip_rect = ui.clip_rect();

            // Hide first-frame-jitters when auto-sizing.
            ui.add_visible_ui(!first_frame_auto_size_columns, |ui| {
                let hovered_row_index_id = self.state_id.with("__table_hovered_row");
                let mut hovered_row_index = ui.data_mut(|data| data.remove_temp::<usize>(hovered_row_index_id));
                if hovered_row_index.is_none() {
                    hovered_row_index = stored_hovered_row_index;
                }
                let layout = StripLayout::new(ui, CellDirection::Horizontal, cell_layout, sense);

                let mut x_offset = 0.0;
                let end_x = clip_rect.right();
                let start_x = clip_rect.left();
                let scroll_offset_x = start_x - layout.rect.left();
                let mut visible_index = Vec::with_capacity(number_of_columns);
                let mut first_col_visible_offset = -layout.ui.spacing().item_spacing[0];
                let mut first_visible_seen = false;
                for (index, width) in widths_ref.iter().enumerate() {
                    columns_offset.push(x_offset);
                    if x_offset + width >= scroll_offset_x && x_offset <= end_x + scroll_offset_x {
                        first_visible_seen = true;
                        visible_index.push(index);
                    }
                    if !first_visible_seen {
                        first_col_visible_offset += width + layout.ui.spacing().item_spacing[0];
                    }
                    x_offset += width + layout.ui.spacing().item_spacing[0];
                }

                add_body_contents(TableBody {
                    layout,
                    columns: columns_ref,
                    widths: widths_ref,
                    visible_columns: visible_index.as_slice(),
                    max_used_widths: max_used_widths_ref,
                    striped,
                    row_index: 0,
                    start_y: clip_rect.top(),
                    start_x: clip_rect.left(),
                    end_y: clip_rect.bottom(),
                    end_x: clip_rect.right(),
                    scroll_to_row: scroll_to_row.map(|(r, _)| r),
                    scroll_to_y_range: &mut scroll_to_y_range,
                    scroll_offset_x,
                    first_col_visible_width: first_col_visible_offset,
                    hovered_row_index,
                    hovered_row_index_id,
                    search_matching_row_index,
                });

                if scroll_to_row.is_some() && scroll_to_y_range.is_none() {
                    // TableBody::row didn't find the right row, so scroll to the bottom:
                    scroll_to_y_range = Some(Rangef::new(f32::INFINITY, f32::INFINITY));
                }
            });

            if let Some(y_range) = scroll_to_y_range {
                let x = 0.0; // ignored, we only have vertical scrolling
                let rect = egui::Rect::from_x_y_ranges(x..=x, y_range);
                let align = scroll_to_row.and_then(|(_, a)| a);
                ui.scroll_to_rect(rect, align);
            }
            columns_offset
        });

        let bottom = ui.min_rect().bottom();

        let spacing_x = ui.spacing().item_spacing.x;
        let mut x = cursor_position.x - spacing_x * 0.5;
        for (i, column_width) in state.column_widths.iter_mut().enumerate() {
            let column = &columns[i];
            let column_is_resizable = column.resizable.unwrap_or(resizable);
            let width_range = column.width_range;

            if !column.clip {
                // Unless we clip we don't want to shrink below the
                // size that was actually used:
                *column_width = column_width.at_least(max_used_widths[i]);
            }
            *column_width = width_range.clamp(*column_width);

            let is_last_column = i + 1 == columns.len();

            if is_last_column && column.initial_width == InitialColumnSize::Remainder {
                // If the last column is 'remainder', then let it fill the remainder!
                let eps = 0.1; // just to avoid some rounding errors.
                *column_width = available_width - eps;
                if !column.clip {
                    *column_width = column_width.at_least(max_used_widths[i]);
                }
                *column_width = width_range.clamp(*column_width);
                break;
            }

            x += *column_width + spacing_x;

            if column.is_auto() && (first_frame_auto_size_columns || !column_is_resizable) {
                *column_width = max_used_widths[i];
                *column_width = width_range.clamp(*column_width);
            } else if column_is_resizable {
                let column_resize_id = ui.id().with("resize_column").with(i);

                let mut p0 = egui::pos2(x, table_top);
                let mut p1 = egui::pos2(x, bottom);
                let line_rect = egui::Rect::from_min_max(p0, p1)
                    .expand(ui.style().interaction.resize_grab_radius_side);

                let resize_response =
                    ui.interact(line_rect, column_resize_id, egui::Sense::click_and_drag());

                if resize_response.double_clicked() {
                    // Resize to the minimum of what is needed.

                    *column_width = width_range.clamp(max_used_widths[i]);
                } else if resize_response.dragged() {
                    if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                        let mut new_width = *column_width + pointer.x - x;
                        if !column.clip {
                            // Unless we clip we don't want to shrink below the
                            // size that was actually used.
                            // However, we still want to allow content that shrinks when you try
                            // to make the column less wide, so we allow some small shrinkage each frame:
                            // big enough to allow shrinking over time, small enough not to look ugly when
                            // shrinking fails. This is a bit of a HACK around immediate mode.
                            let max_shrinkage_per_frame = 8.0;
                            new_width =
                                new_width.at_least(max_used_widths[i] - max_shrinkage_per_frame);
                        }
                        new_width = width_range.clamp(new_width);

                        let x = x - *column_width + new_width;
                        (p0.x, p1.x) = (x, x);

                        *column_width = new_width;
                    }
                }

                let dragging_something_else =
                    ui.input(|i| i.pointer.any_down() || i.pointer.any_pressed());
                let resize_hover = resize_response.hovered() && !dragging_something_else;

                if resize_hover || resize_response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                }

                let mut stroke = if resize_response.dragged() {
                    ui.style().visuals.widgets.active.bg_stroke
                } else if resize_hover {
                    ui.style().visuals.widgets.hovered.bg_stroke
                } else {
                    // ui.visuals().widgets.inactive.bg_stroke
                    ui.visuals().widgets.noninteractive.bg_stroke
                };
                if i == number_of_columns - 1 {
                    stroke.color = Color32::DARK_GRAY;
                    stroke.width = 2.0;
                }

                ui.painter().line_segment([p0, p1], stroke);
            };

            available_width -= *column_width + spacing_x;
        }
        state.store(ui, state_id);
        scroll_area_output
    }
}

/// The body of a table.
///
/// Is created by calling `body` on a [`Table`] (after adding a header row) or [`TableBuilder`] (without a header row).
pub struct TableBody<'a> {
    layout: StripLayout<'a>,

    columns: &'a [Column],

    /// Current column widths.
    widths: &'a [f32],
    visible_columns: &'a [usize],

    /// Accumulated maximum used widths for each column.
    max_used_widths: &'a mut [f32],

    striped: bool,
    row_index: usize,
    start_y: f32,
    start_x: f32,
    end_y: f32,
    end_x: f32,

    /// Look for this row to scroll to.
    scroll_to_row: Option<usize>,

    /// If we find the correct row to scroll to,
    /// this is set to the y-range of the row.
    scroll_to_y_range: &'a mut Option<Rangef>,

    hovered_row_index: Option<usize>,

    /// Used to store the hovered row index between frames.
    hovered_row_index_id: egui::Id,
    pub scroll_offset_x: f32,
    pub first_col_visible_width: f32,
    pub search_matching_row_index: Option<usize>,
}

impl<'a> TableBody<'a> {
    /// Access the contained [`egui::Ui`].
    ///
    /// You can use this to e.g. modify the [`egui::Style`] with [`egui::Ui::style_mut`].
    pub fn ui_mut(&mut self) -> &mut egui::Ui {
        self.layout.ui
    }

    /// Where in screen-space is the table body?
    pub fn max_rect(&self) -> Rect {
        self.layout
            .rect
            .translate(egui::vec2(0.0, self.scroll_offset_y()))
    }

    fn scroll_offset_y(&self) -> f32 {
        self.start_y - self.layout.rect.top()
    }

    pub fn scroll_offset_x(&self) -> f32 {
        self.start_x - self.layout.rect.left()
    }

    /// Return a vector containing all column widths for this table body.
    ///
    /// This is primarily meant for use with [`TableBody::heterogeneous_rows`] in cases where row
    /// heights are expected to according to the width of one or more cells -- for example, if text
    /// is wrapped rather than clipped within the cell.
    pub fn widths(&self) -> &[f32] {
        self.widths
    }

    /// Add many rows with same height.
    ///
    /// Is a lot more performant than adding each individual row as non visible rows must not be rendered.
    ///
    /// If you need many rows with different heights, use [`Self::heterogeneous_rows`] instead.
    ///
    /// ### Example
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// use egui_extras::{TableBuilder, Column};
    /// TableBuilder::new(ui)
    ///     .column(Column::remainder().at_least(100.0))
    ///     .body(|mut body| {
    ///         let row_height = 18.0;
    ///         let num_rows = 10_000;
    ///         body.rows(row_height, num_rows, |mut row| {
    ///             let row_index = row.index();
    ///             row.col(|ui| {
    ///                 ui.label(format!("First column of row {row_index}"));
    ///             });
    ///         });
    ///     });
    /// # });
    /// ```
    pub fn rows(
        mut self,
        row_height_sans_spacing: f32,
        total_rows: usize,
        mut add_row_content: impl FnMut(TableRow<'_, '_>),
    ) -> Option<usize> {
        let spacing = self.layout.ui.spacing().item_spacing;
        let row_height_with_spacing = row_height_sans_spacing + spacing.y;

        if let Some(scroll_to_row) = self.scroll_to_row {
            let scroll_to_row = scroll_to_row.at_most(total_rows.saturating_sub(1)) as f32;
            *self.scroll_to_y_range = Some(Rangef::new(
                self.layout.cursor.y + scroll_to_row * row_height_with_spacing,
                self.layout.cursor.y + (scroll_to_row + 1.0) * row_height_with_spacing,
            ));
        }

        let scroll_offset_y = self
            .scroll_offset_y()
            .min(total_rows as f32 * row_height_with_spacing);
        let max_height = self.end_y - self.start_y;
        let mut min_row = 0;

        if scroll_offset_y > 0.0 {
            min_row = (scroll_offset_y / row_height_with_spacing).floor() as usize;
            self.add_buffer(min_row as f32 * row_height_with_spacing);
        }

        let max_row =
            ((scroll_offset_y + max_height) / row_height_with_spacing).ceil() as usize + 1;
        let max_row = max_row.min(total_rows);

        for row_index in min_row..max_row {
            let mut response: Option<Response> = None;
            add_row_content(TableRow {
                layout: &mut self.layout,
                columns: self.columns,
                widths: self.widths,
                visible_columns: self.visible_columns,
                max_used_widths: self.max_used_widths,
                row_index,
                col_index: 0,
                start_x: self.start_x,
                first_col_visible_offset: self.first_col_visible_width,
                height: row_height_sans_spacing,
                striped: self.striped && (row_index + self.row_index) % 2 == 0,
                hovered: self.hovered_row_index == Some(row_index),
                highlighted: self.search_matching_row_index == Some(row_index),
                selected: false,
                response: &mut response,
                remainder_with: 0.0,
            });
            self.capture_hover_state(&response, row_index);
        }

        if total_rows - max_row > 0 {
            let skip_height = (total_rows - max_row) as f32 * row_height_with_spacing;
            self.add_buffer(skip_height - spacing.y);
        }
        self.hovered_row_index
    }


    // Create a table row buffer of the given height to represent the non-visible portion of the
    // table.
    fn add_buffer(&mut self, height: f32) {
        self.layout.skip_space(egui::vec2(0.0, height));
    }

    // Capture the hover information for the just created row. This is used in the next render
    // to ensure that the entire row is highlighted.
    fn capture_hover_state(&mut self, response: &Option<Response>, row_index: usize) {
        let is_row_hovered = response.as_ref().map_or(false, |r| r.hovered());
        if is_row_hovered {
            self.layout
                .ui
                .data_mut(|data| data.insert_temp(self.hovered_row_index_id, row_index));
        }
    }
}

impl<'a> Drop for TableBody<'a> {
    fn drop(&mut self) {
        self.layout.allocate_rect();
    }
}

/// The row of a table.
/// Is created by [`TableRow`] for each created [`TableBody::row`] or each visible row in rows created by calling [`TableBody::rows`].
pub struct TableRow<'a, 'b> {
    layout: &'b mut StripLayout<'a>,
    columns: &'b [Column],
    widths: &'b [f32],
    visible_columns: &'b [usize],

    /// grows during building with the maximum widths
    max_used_widths: &'b mut [f32],

    start_x: f32,
    first_col_visible_offset: f32,

    row_index: usize,
    col_index: usize,
    height: f32,

    striped: bool,
    hovered: bool,
    selected: bool,

    response: &'b mut Option<Response>,
    pub remainder_with: f32,
    pub highlighted: bool,
}

pub struct ColumnResponse {
    pub clicked_col_index: Option<usize>,
    pub double_clicked_col_index: Option<usize>,
    pub hovered_col_index: Option<usize>,
}

impl<'a, 'b> TableRow<'a, 'b> {
    /// Add the contents of a column.
    ///
    /// Returns the used space (`min_rect`) plus the [`Response`] of the whole cell.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn col(&mut self, add_cell_contents: impl FnMut(&mut Ui, usize) -> Option<Response>) -> (Rect, Response) {
        let col_index = self.col_index;

        let clip = self.columns.get(col_index).map_or(false, |c| c.clip);

        let width = if let Some(width) = self.widths.get(col_index) {
            self.col_index += 1;
            *width
        } else {
            panic!(
                "Added more `Table` columns than were pre-allocated ({} pre-allocated)",
                self.widths.len()
            );
        };

        let width = CellSize::Absolute(width);
        let height = CellSize::Absolute(self.height);

        let flags = StripLayoutFlags {
            clip,
            striped: self.striped,
            hovered: self.hovered,
            selected: self.selected,
            highlighted: self.highlighted,
        };

        let (used_rect, response) = self.layout.add(
            flags,
            width,
            height,
            egui::Id::new((self.row_index, col_index)),
            col_index,
            Some(add_cell_contents),
        );

        if let Some(max_w) = self.max_used_widths.get_mut(col_index) {
            *max_w = max_w.max(used_rect.width());
        }

        *self.response = Some(
            self.response
                .as_ref()
                .map_or(response.clone(), |r| r.union(response.clone())),
        );

        (used_rect, response)
    }
    pub fn cols(&mut self, is_header: bool, mut add_cell_contents: impl FnMut(&mut Ui, usize) -> Option<Response>) -> ColumnResponse {
        let width = self.first_col_visible_offset;

        self.layout.add_empty(
            CellSize::Absolute(width),
            CellSize::Absolute(self.height),
            Color32::GOLD,
        );

        let mut last_index = 0;
        let mut column_response = ColumnResponse { clicked_col_index: None, double_clicked_col_index: None, hovered_col_index: None };
        for col_index in self.visible_columns {
            let clip = self.columns.get(*col_index).map_or(false, |c| c.clip);
            let width = if let Some(width) = self.widths.get(*col_index) {
                *width
            } else {
                panic!(
                    "Added more `Table` columns than were pre-allocated ({} pre-allocated)",
                    self.widths.len()
                );
            };
            let width = CellSize::Absolute(width);
            let height = CellSize::Absolute(self.height);


            let flags = StripLayoutFlags {
                clip,
                striped: self.striped,
                hovered: self.hovered,
                selected: self.selected,
                highlighted: self.highlighted,
            };

            let (used_rect, response) = self.layout.add(
                flags,
                width,
                height,
                egui::Id::new((self.row_index, *col_index)),
                *col_index,
                Some(&mut add_cell_contents),
            );

            if let Some(max_w) = self.max_used_widths.get_mut(*col_index) {
                *max_w = max_w.max(used_rect.width());
            }
            if response.clicked() {
                column_response.clicked_col_index = Some(*col_index);
            }
            if response.double_clicked() {
                column_response.double_clicked_col_index = Some(*col_index);
            }
            if response.hovered() {
                column_response.hovered_col_index = Some(*col_index);
            }
            *self.response = Some(
                self.response
                    .as_ref()
                    .map_or(response.clone(), |r| r.union(response.clone())),
            );
            last_index = *col_index;
        }
        if !self.columns.is_empty() && is_header && last_index < self.columns.len() - 1 {
            self.layout.add_empty(
                CellSize::Absolute(self.remainder_with),
                CellSize::Absolute(self.height),
                Color32::GOLD,
            );
        }
        column_response
    }

    /// Set the selection highlight state for cells added after a call to this function.
    #[inline]
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    /// Returns a union of the [`Response`]s of the cells added to the row up to this point.
    ///
    /// You need to add at least one row to the table before calling this function.
    pub fn response(&self) -> Response {
        self.response
            .clone()
            .expect("Should only be called after `col`")
    }

    /// Returns the index of the row.
    #[inline]
    pub fn index(&self) -> usize {
        self.row_index
    }

    /// Returns the index of the column. Incremented after a column is added.
    #[inline]
    pub fn col_index(&self) -> usize {
        self.col_index
    }
}

impl<'a, 'b> Drop for TableRow<'a, 'b> {
    #[inline]
    fn drop(&mut self) {
        self.layout.end_line();
    }
}
