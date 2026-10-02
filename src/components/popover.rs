/// Heavily inspired from egui codebase
///
/// Credit egui_extras: https://github.com/emilk/egui
/// Modification are:
/// - Open popover when clicking on a button
use eframe::egui::{InnerResponse, Response, ScrollArea, Ui, WidgetInfo, WidgetType};

use eframe::egui::*;

pub struct PopupMenu {
    id_source: Id,
    width: Option<f32>,
    height: Option<f32>,
}

impl PopupMenu {
    pub fn new(id_source: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id_source: Id::new(id_source),
            width: None,
            height: None,
        }
    }

    #[inline]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    #[inline]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui) -> Response,
        menu_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        self.show_ui_dyn(ui, button, Box::new(menu_contents))
    }

    fn show_ui_dyn<'c, R>(
        self,
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui) -> Response,
        menu_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    ) -> InnerResponse<Option<R>> {
        let Self {
            id_source,
            width: _,
            height,
        } = self;

        let button_id = ui.make_persistent_id(id_source);

        ui.horizontal(|ui| {
            let ir = popup(ui, button, button_id, menu_contents, height);
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
    button_id: Id,
    menu_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    height: Option<f32>,
) -> InnerResponse<Option<R>> {
    let popup_id = button_id.with("popup");

    let button_response = button.ui(ui);

    let height = height.unwrap_or_else(|| ui.spacing().combo_height);

    let inner = popup_below_or_above_widget(ui, popup_id, &button_response, |ui| {
        ScrollArea::vertical()
            .max_height(height)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                menu_contents(ui)
            })
            .inner
    });

    InnerResponse {
        inner,
        response: button_response,
    }
}

pub fn popup_below_or_above_widget<R>(
    ui: &Ui,
    popup_id: Id,
    widget_response: &Response,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let frame_margin = Frame::popup(ui.style()).total_margin();
    Popup::from_response(widget_response)
        .id(popup_id)
        .open_memory(widget_response.clicked().then_some(SetOpenCommand::Toggle))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_START)
        .align_alternatives(&[RectAlign::TOP_START])
        .layout(Layout::top_down_justified(Align::LEFT))
        .show(|ui| {
            ui.set_width(widget_response.rect.width() - frame_margin.sum().x);
            add_contents(ui)
        })
        .map(|response| response.inner)
}
