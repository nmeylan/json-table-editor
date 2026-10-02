/// Heavily inspired from egui codebase
///
/// Credit egui_extras: https://github.com/emilk/egui
/// Modification are:
/// - Open popover when clicking on a button
/// - Optional text filter, with select/unselect matching, select all and "view selected only"
use eframe::egui::{InnerResponse, Response, ScrollArea, Ui, WidgetInfo, WidgetType};

use eframe::egui::*;

enum AboveOrBelow {
    Above,
    Below,
}

pub struct PopupMenu {
    id_source: Id,
    with_filter: bool,
    height: Option<f32>,
}

impl PopupMenu {
    pub fn new(id_source: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id_source: Id::new(id_source),
            height: None,
            with_filter: false,
        }
    }

    #[inline]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    #[inline]
    pub fn with_filter(mut self) -> Self {
        self.with_filter = true;
        self
    }

    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui) -> Response,
        menu_contents: impl FnOnce(&mut Ui, &mut PopupStateResponse) -> R,
    ) -> InnerResponse<Option<R>> {
        self.show_ui_dyn(ui, button, Box::new(menu_contents))
    }

    fn show_ui_dyn<'c, R>(
        self,
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui) -> Response,
        menu_contents: Box<dyn FnOnce(&mut Ui, &mut PopupStateResponse) -> R + 'c>,
    ) -> InnerResponse<Option<R>> {
        let Self {
            id_source,
            height,
            with_filter,
        } = self;

        let popup_id = ui.make_persistent_id(id_source);

        ui.horizontal(|ui| {
            let ir = popup(ui, button, popup_id, menu_contents, height, with_filter);
            ir.response
                .widget_info(|| WidgetInfo::new(WidgetType::ComboBox));
            ir
        })
        .inner
    }
}

fn popup<'c, R>(
    ui: &mut Ui,
    button: impl FnOnce(&mut Ui) -> Response,
    popup_id: Id,
    menu_contents: Box<dyn FnOnce(&mut Ui, &mut PopupStateResponse) -> R + 'c>,
    height: Option<f32>,
    with_filter: bool,
) -> InnerResponse<Option<R>> {
    let popup_height = height.unwrap_or(100.0);

    let above_or_below =
        if ui.next_widget_position().y + ui.spacing().interact_size.y + popup_height
            < ui.ctx().content_rect().bottom()
        {
            AboveOrBelow::Below
        } else {
            AboveOrBelow::Above
        };

    let button_response = button.ui(ui);
    if button_response.clicked() {
        Popup::toggle_id(ui.ctx(), popup_id);
    }

    let height = height.unwrap_or_else(|| ui.spacing().combo_height);

    let filter_id = ui.make_persistent_id(popup_id.with("popup_filter"));
    let mut popup_state = ui
        .data_mut(|data| data.get_temp::<PopupState>(filter_id))
        .unwrap_or_default();

    let is_sizing_pass = popup_state.size.is_none();
    let inner = popup_above_or_below_widget(
        ui,
        popup_id,
        &button_response,
        above_or_below,
        is_sizing_pass,
        &mut popup_state,
        |ui, popup_state: &mut PopupState| {
            let mut popup_state_response = if with_filter {
                if !popup_state.setup {
                    popup_state.setup = true;
                    ui.data_mut(|data| data.insert_temp(filter_id, popup_state.clone()));
                }

                ui.vertical(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut popup_state.filter)
                            .hint_text("Filter")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        for filter_mode in [
                            FilterMode::Contains,
                            FilterMode::StartsWith,
                            FilterMode::Equals,
                        ] {
                            if ui
                                .radio(
                                    popup_state.filter_mode.eq(&filter_mode),
                                    filter_mode.as_str(),
                                )
                                .clicked()
                            {
                                popup_state.filter_mode = filter_mode;
                            }
                        }
                    });
                });
                let mut clicked_select_all = false;
                let mut clicked_unselect_all = false;
                let mut clicked_select_all_matching_selected = false;
                let mut clicked_unselect_all_matching_selected = false;
                let mut clicked_replace_with_matching = false;
                let mut clicked_replace_without_matching = false;
                if !popup_state.filter.is_empty() {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(Button::new("Select matching").wrap_mode(TextWrapMode::Extend))
                                .on_hover_text("Replace selected filters with matching elements")
                                .clicked()
                            {
                                clicked_replace_with_matching = true;
                            }
                            if ui
                                .add(
                                    Button::new("Unselect matching")
                                        .wrap_mode(TextWrapMode::Extend),
                                )
                                .on_hover_text("Select all elements except matching")
                                .clicked()
                            {
                                clicked_replace_without_matching = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    Button::new("Add to selection").wrap_mode(TextWrapMode::Extend),
                                )
                                .on_hover_text("Add matching elements to selected filters")
                                .clicked()
                            {
                                clicked_select_all_matching_selected = true;
                            }
                            if ui
                                .add(
                                    Button::new("Remove from selection")
                                        .wrap_mode(TextWrapMode::Extend),
                                )
                                .on_hover_text("Remove matching elements from selected filters")
                                .clicked()
                            {
                                clicked_unselect_all_matching_selected = true;
                            }
                        });
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Select all").clicked() {
                        clicked_select_all = true;
                    }
                    if ui.button("Unselect all").clicked() {
                        clicked_unselect_all = true;
                    }
                });
                ui.separator();
                ui.checkbox(&mut popup_state.show_only_selected, "View selected only");
                ui.separator();

                PopupStateResponse {
                    is_sizing_pass: popup_state.size.is_none(),
                    filter: popup_state.filter.clone(),
                    filter_mode: popup_state.filter_mode,
                    show_only_selected: popup_state.show_only_selected,
                    clicked_select_all,
                    clicked_unselect_all,
                    clicked_select_all_matching: clicked_select_all_matching_selected,
                    clicked_unselect_all_matching: clicked_unselect_all_matching_selected,
                    clicked_replace_with_matching,
                    clicked_replace_without_matching,
                }
            } else {
                PopupStateResponse {
                    is_sizing_pass: popup_state.size.is_none(),
                    filter: Default::default(),
                    filter_mode: Default::default(),
                    show_only_selected: Default::default(),
                    clicked_select_all: Default::default(),
                    clicked_unselect_all: Default::default(),
                    clicked_select_all_matching: Default::default(),
                    clicked_unselect_all_matching: Default::default(),
                    clicked_replace_with_matching: Default::default(),
                    clicked_replace_without_matching: Default::default(),
                }
            };

            ScrollArea::vertical()
                .max_height(height)
                .min_scrolled_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    menu_contents(ui, &mut popup_state_response)
                })
                .inner
        },
    );
    ui.memory_mut(|mem| {
        mem.data.insert_temp(filter_id, popup_state.clone());
    });

    InnerResponse {
        inner,
        response: button_response,
    }
}

fn popup_above_or_below_widget<R>(
    ui: &Ui,
    popup_id: Id,
    widget_response: &Response,
    above_or_below: AboveOrBelow,
    is_sizing_pass: bool,
    popup_state: &mut PopupState,
    add_contents: impl FnOnce(&mut Ui, &mut PopupState) -> R,
) -> Option<R> {
    if Popup::is_id_open(ui.ctx(), popup_id) {
        // egui garbage-collects a popup whose `open_this_frame` flag isn't re-affirmed every
        // frame (Memory::end_pass). `keep_popup_open` is private, so re-open it to keep it alive;
        // the close logic below still closes it explicitly when needed.
        Popup::open_id(ui.ctx(), popup_id);

        let (pos, pivot) = match above_or_below {
            AboveOrBelow::Above => (widget_response.rect.left_top(), Align2::LEFT_BOTTOM),
            AboveOrBelow::Below => (widget_response.rect.left_bottom(), Align2::LEFT_TOP),
        };

        let inner = Area::new(popup_id)
            .order(Order::Tooltip)
            .constrain(true)
            .fixed_pos(pos)
            .pivot(pivot)
            .show(ui.ctx(), |ui| {
                let frame = Frame::popup(&ui.global_style());
                let frame_margin = frame.total_margin();
                frame
                    .show(ui, |ui| {
                        let mut ui_builder = egui::UiBuilder::new();
                        if is_sizing_pass {
                            ui_builder = ui_builder.sizing_pass();
                        }
                        let response = ui.scope_builder(ui_builder, |ui| {
                            let response =
                                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                                    if let Some(size) = popup_state.size {
                                        ui.set_width(size);
                                    } else {
                                        ui.set_width(
                                            (widget_response.rect.width() - frame_margin.sum().x)
                                                .max(1.0),
                                        );
                                    }

                                    add_contents(ui, popup_state)
                                });
                            response.inner
                        });
                        if is_sizing_pass {
                            popup_state.size = Some(response.response.rect.width());
                        }
                        response.inner
                    })
                    .inner
            });

        if ui.input(|i| i.key_pressed(Key::Escape))
            || (!widget_response.clicked() && inner.response.clicked_elsewhere())
        {
            Popup::close_id(ui.ctx(), popup_id);
        }
        Some(inner.inner)
    } else {
        None
    }
}

#[derive(Default, Clone)]
pub struct PopupState {
    pub filter: String,
    pub filter_mode: FilterMode,
    pub show_only_selected: bool,
    pub setup: bool,
    size: Option<f32>,
}

#[derive(Clone)]
pub struct PopupStateResponse {
    pub filter: String,
    pub filter_mode: FilterMode,
    pub show_only_selected: bool,
    pub clicked_select_all: bool,
    pub clicked_unselect_all: bool,
    pub clicked_select_all_matching: bool,
    pub clicked_unselect_all_matching: bool,
    pub clicked_replace_with_matching: bool,
    pub clicked_replace_without_matching: bool,
    pub is_sizing_pass: bool,
}

#[repr(u8)]
#[derive(Default, Clone, Copy, Eq, PartialEq)]
pub enum FilterMode {
    #[default]
    Contains,
    StartsWith,
    Equals,
}

impl FilterMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterMode::Contains => "contains",
            FilterMode::StartsWith => "starts with",
            FilterMode::Equals => "equals",
        }
    }
}
