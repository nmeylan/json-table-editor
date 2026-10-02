use super::ArrayTable;
use super::row_view::{ColumnFilter, SortDirection};
use crate::ACTIVE_COLOR;
use crate::components::filter::Filter;
use crate::components::icon;
use crate::components::table::TableRow;
use crate::fonts::{FILTER, SEARCH, THUMBTACK};
use crate::panels::SearchReplacePanel;
use eframe::egui::{
    Align, Color32, Id, Label, Layout, Painter, Rect, Sense, Stroke, TextBuffer, Ui, Vec2,
    WidgetText, pos2,
};
use std::cell::OnceCell;

impl<'array> ArrayTable<'array> {
    pub(super) fn header(&mut self, pinned_column_table: bool, mut header: TableRow) {
        // Mutation after interaction
        let mut clicked_filter_non_null_column: Option<String> = None;
        let mut changed_filter: Option<(String, ColumnFilter)> = None;
        let mut pinned_column: Option<usize> = None;
        let mut clicked_replace_column: Option<usize> = None;
        let mut clicked_sort: Option<Option<(String, SortDirection)>> = None;
        header.cols(true, |ui, index| {
            let columns = self.columns(pinned_column_table);
            let column = columns.get(index).unwrap();
            let name = column.name.as_str();
            let strong = Label::new(WidgetText::RichText(egui::RichText::from(name).into()));
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
                        let checked_filtered_values = self.row_view.filter(column.name.as_str());
                        let is_include =
                            matches!(checked_filtered_values, Some(ColumnFilter::Include(_)));
                        let selected_values = match checked_filtered_values {
                            Some(ColumnFilter::Include(values) | ColumnFilter::Exclude(values)) => {
                                Some(values)
                            }
                            _ => None,
                        };
                        // Only computed when the popover is open
                        let values = OnceCell::new();
                        let values = || {
                            values.get_or_init(|| {
                                if Self::is_filterable(column) {
                                    self.row_view.distinct_values(&self.nodes, column)
                                } else {
                                    Default::default()
                                }
                            })
                        };
                        let mut filter = Filter::new(column_id.with("filter"));
                        // Include filter: checked values are the selected ones,
                        // otherwise selected values are the excluded ones.
                        if !is_include {
                            filter = filter.inverse_filter();
                        }
                        let mut filter_response = filter.show_ui(
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
                                let mut chcked =
                                    matches!(checked_filtered_values, Some(ColumnFilter::NonNull));
                                if ui.checkbox(&mut chcked, "Non null").clicked() {
                                    clicked_filter_non_null_column = Some(name.to_string());
                                }
                                ui.separator();
                            },
                            || selected_values.into_iter().flatten().map(String::as_str),
                            || values().iter().map(String::as_str),
                            |value| *value,
                        );
                        if let Some(selected) = filter_response.selected_items().take() {
                            let selected = selected.into_iter().map(str::to_string).collect();
                            changed_filter = Some((
                                name.to_string(),
                                if is_include {
                                    ColumnFilter::Include(selected)
                                } else {
                                    ColumnFilter::Exclude(selected)
                                },
                            ));
                        }

                        if Self::is_sortable(column) {
                            let active = self
                                .row_view
                                .sort()
                                .filter(|(sorted_column, _)| *sorted_column == name)
                                .map(|(_, direction)| direction);
                            if let Some(direction) = sort_carets(ui, active) {
                                // Clicking the active caret restores data order
                                clicked_sort = Some(
                                    (active != Some(direction))
                                        .then(|| (name.to_string(), direction)),
                                );
                            }
                        }

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
            self.row_view.toggle_non_null(&clicked_column);
            self.do_filter_column();
        }
        if let Some((column, filter)) = changed_filter {
            self.row_view.set_filter(column, Some(filter));
            self.do_filter_column();
        }
        if let Some(sort) = clicked_sort {
            self.row_view.set_sort(sort);
            self.do_filter_column();
        }
    }
}

/// ASC and DESC carets stacked, returns the clicked one.
fn sort_carets(ui: &mut Ui, active: Option<SortDirection>) -> Option<SortDirection> {
    let column_width = 12.0;
    ui.allocate_ui_with_layout(
        Vec2::new(column_width, 18.0),
        Layout::top_down(Align::Center),
        |ui| {
            let dim = ui.visuals().weak_text_color();
            let mut clicked = None;
            for (direction, up) in [(SortDirection::Asc, true), (SortDirection::Desc, false)] {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(column_width, 6.0), Sense::click());
                let color = if active == Some(direction) || response.hovered() {
                    ACTIVE_COLOR
                } else {
                    dim
                };
                draw_caret(ui.painter(), rect, up, color);
                if response.clicked() {
                    clicked = Some(direction);
                }
            }
            clicked
        },
    )
    .inner
}

fn draw_caret(painter: &Painter, rect: Rect, up: bool, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    let center = rect.center();
    let half_w = 3.5;
    let half_h = 2.0;
    let (tip_y, base_y) = if up {
        (center.y - half_h, center.y + half_h)
    } else {
        (center.y + half_h, center.y - half_h)
    };
    let tip = pos2(center.x, tip_y);
    let left = pos2(center.x - half_w, base_y);
    let right = pos2(center.x + half_w, base_y);
    painter.line_segment([left, tip], stroke);
    painter.line_segment([tip, right], stroke);
}
