use std::borrow::Cow;
use std::cell::RefCell;
use eframe::egui::Context;
use eframe::egui::{Ui};
use eframe::emath::Align;
use eframe::epaint::text::TextWrapMode;
use egui::{Button, Grid, Layout, RichText, Sense, TextBuffer, TextEdit};
use json_flat_parser::ValueType;
use crate::ACTIVE_COLOR;
use crate::array_table::Column;
use crate::components::popover::PopupMenu;
use crate::panels::ReplaceMode::Simple;

pub const PANEL_ABOUT: &'static str = "About";
pub const PANEL_REPLACE: &'static str = "Replace";

#[derive(Default)]
pub struct AboutPanel {}

#[derive(Default)]
pub struct SearchReplacePanel<'array> {
    search_criteria: String,
    replace_value: String,
    selected_columns: RefCell<Vec<Column<'array>>>,
    columns: Vec<Column<'array>>,
    replace_mode: ReplaceMode,
    title: Option<String>
}
#[derive(Clone)]
pub enum ReplaceMode {
    Simple,
    Regex,
    ExactWord,
    MatchingCase,
}
impl Default for ReplaceMode {
    fn default() -> Self {
        Simple
    }
}

pub struct SearchReplaceResponse<'array> {
    pub search_criteria: String,
    pub replace_value: Option<String>,
    pub selected_column: Option<Vec<Column<'array>>>,
    pub replace_mode: ReplaceMode,
}


impl super::Window<()> for AboutPanel {
    fn name(&self) -> &'static str {
        PANEL_ABOUT
    }

    fn show(&mut self, ctx: &Context, open: &mut bool) {
        egui::Window::new(self.name())
            .collapsible(false)
            .open(open)
            .resizable([true, true])
            .default_width(280.0)
            .show(ctx, |ui| {
                use super::View as _;
                self.ui(ui);
            });
    }

}

impl super::View<()> for AboutPanel {
    fn ui(&mut self, ui: &mut Ui) {
        ui.heading("About");
        ui.label("Licence: Apache-2.0 license");
        ui.hyperlink_to("View project on Github", "https://github.com/nmeylan/json-table-editor");
        ui.separator();
        ui.heading("Credits");
        ui.hyperlink_to("egui project and its community", "https://github.com/emilk/egui");
        ui.hyperlink_to("Maintainers of dependencies used by this project", "https://github.com/nmeylan/json-table-editor/blob/master/Cargo.lock");
    }
}

impl <'array>SearchReplacePanel<'array> {
    pub fn set_columns(&mut self, columns: Vec<Column<'array>>) {
        self.columns = columns;
    }

    pub fn set_title(&mut self, title: String) {
        self.title = Some(title);
    }
    pub fn set_select_column(&mut self, selected_column: Column<'array>) {
        *self.selected_columns.borrow_mut() = vec![selected_column];
    }

    pub fn can_be_replaced(c: &Column<'array>) -> bool {
        !(matches!(c.value_type, ValueType::Array(_)) || matches!(c.value_type, ValueType::Object(..)))
    }
}
impl <'array>super::Window<Option<SearchReplaceResponse<'array>>> for SearchReplacePanel<'array> {
    fn name(&self) -> &'static str {
        PANEL_REPLACE
    }

    fn show(&mut self, ctx: &Context, open: &mut bool) -> Option<SearchReplaceResponse<'array>> {
        let window_title = if let Some(ref title) = self.title {
            title.as_str()
        } else {
            self.name()
        };
        let maybe_inner_response = egui::Window::new(window_title)
            .collapsible(true)
            .open(open)
            .resizable([true, true])
            .default_width(280.0)
            .show(ctx, |ui| {
                use super::View as _;
                self.ui(ui)
            });

        if let Some(inner_response) = maybe_inner_response {
            if let Some(inner_response2) = inner_response.inner {
                return inner_response2;
            }
        }
        None
    }
}

impl <'array>super::View<Option<SearchReplaceResponse<'array>>> for SearchReplacePanel<'array> {
    fn ui(&mut self, ui: &mut Ui) -> Option<SearchReplaceResponse<'array>> {
        let is_replace_value_empty = self.replace_value.is_empty();
        let search = TextEdit::singleline(&mut self.search_criteria);
        let replace = TextEdit::singleline(&mut self.replace_value);
        let mut button_set_to_null = Button::new("Set to null").sense(Sense::hover());
        let mut button = Button::new("Replace all");
        let grid_response = Grid::new("replace_panel:grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .striped(false)
            .show(ui, |ui| {
                ui.label("Column: ");
                PopupMenu::new("select_column_to_replace")
                    .show_ui(ui, |ui| {
                        let button_label = if self.selected_columns.borrow().is_empty() {
                            "No column selected".to_lowercase()
                        } else if self.selected_columns.borrow().len() > 5 {
                            format!("{} columns selected", self.selected_columns.borrow().len())
                        } else {
                            self.selected_columns.borrow().iter().map(|c| c.name.clone()).collect::<Vec<Cow<'array, str>>>().join(", ")
                        };
                        let response = ui.add(Button::new(button_label));
                        response.on_hover_ui(|ui| {
                            // ui.set_min_width(140.0);
                            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                            self.selected_columns.borrow().iter().for_each(|c| {ui.label(c.name.as_str());})
                        })
                    }, |ui| {
                        for col in self.columns.iter().filter(|c| Self::can_be_replaced(c)) {
                            if col.name.is_empty() {
                                continue;
                            }
                            let mut chcked = false;

                            for selected in self.selected_columns.borrow().iter() {
                                if selected.eq(col) {
                                    chcked = true;
                                    break;
                                }
                            }
                            if ui.checkbox(&mut chcked, col.name.as_str()).clicked() {
                                if self.selected_columns.borrow().contains(col) {
                                    self.selected_columns.borrow_mut().retain(|c| !c.eq(col));
                                } else {
                                    self.selected_columns.borrow_mut().push(col.clone());
                                }
                            }
                        }
                    });

                ui.end_row();
                ui.label("Search: ");
                ui.add(search);
                ui.end_row();
                ui.label("Replace: ");
                ui.add(replace);
                ui.end_row();

                ui.label("");
                let mut replace_match_case_text = RichText::new("Cc");
                let mut replace_exact_word_text = RichText::new("W");
                let mut replace_regex_text = RichText::new(".*");
                if matches!(self.replace_mode, ReplaceMode::MatchingCase) {
                    replace_match_case_text = replace_match_case_text.color(ACTIVE_COLOR);
                }
                if matches!(self.replace_mode, ReplaceMode::ExactWord) {
                    replace_exact_word_text = replace_exact_word_text.color(ACTIVE_COLOR);
                }
                if matches!(self.replace_mode, ReplaceMode::Regex) {
                    replace_regex_text = replace_regex_text.color(ACTIVE_COLOR);
                }
                let replace_regex_mode = Button::new(replace_regex_text);
                let replace_exact_word_mode = Button::new(replace_exact_word_text);
                let replace_match_case_mode = Button::new(replace_match_case_text);
                let mut replace_response = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.selected_columns.borrow().len() == 0 {
                        button = button.sense(Sense::hover());
                    }
                    if is_replace_value_empty {
                        button_set_to_null = button_set_to_null.sense(Sense::click());
                    }
                    let response_button_replace_with_null = ui.add(button_set_to_null);
                    let response_button_replace = ui.add(button);
                    let mut response_replace_regex_mode = ui.add(replace_regex_mode);
                    response_replace_regex_mode = response_replace_regex_mode.on_hover_ui(|ui| { ui.label("Regex"); });
                    if response_replace_regex_mode.clicked() {
                        if matches!(self.replace_mode, ReplaceMode::Regex) {
                            self.replace_mode = ReplaceMode::Simple;
                        } else {
                            self.replace_mode = ReplaceMode::Regex;
                        }
                    }
                    let mut response_replace_exact_word_mode = ui.add(replace_exact_word_mode);
                    response_replace_exact_word_mode = response_replace_exact_word_mode.on_hover_ui(|ui| { ui.label("Exact word"); });
                    if response_replace_exact_word_mode.clicked() {
                        if matches!(self.replace_mode, ReplaceMode::ExactWord) {
                            self.replace_mode = ReplaceMode::Simple;
                        } else {
                            self.replace_mode = ReplaceMode::ExactWord;
                        }
                    }
                    let mut response_replace_match_case_mode = ui.add(replace_match_case_mode);
                    response_replace_match_case_mode = response_replace_match_case_mode.on_hover_ui(|ui| { ui.label("Matching case"); });
                    if response_replace_match_case_mode.clicked() {
                        if matches!(self.replace_mode, ReplaceMode::MatchingCase) {
                            self.replace_mode = ReplaceMode::Simple;
                        } else {
                            self.replace_mode = ReplaceMode::MatchingCase;
                        }
                    }
                    (response_button_replace, response_button_replace_with_null)
                }).inner;
                ui.end_row();

                replace_response
            });
        if grid_response.inner.0.clicked() {
            return Some(SearchReplaceResponse {
                search_criteria: self.search_criteria.clone(),
                replace_value: Some(self.replace_value.clone()),
                replace_mode: self.replace_mode.clone(),
                selected_column: Some(self.selected_columns.borrow().clone()),
            })
        } else if grid_response.inner.1.clicked() {
            return Some(SearchReplaceResponse {
                search_criteria: self.search_criteria.clone(),
                replace_value: None,
                replace_mode: self.replace_mode.clone(),
                selected_column: Some(self.selected_columns.borrow().clone()),
            })
        }
        None
        // return grid_response.inner
    }
}
