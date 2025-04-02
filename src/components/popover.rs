/// Heavily inspired from egui codebase
///
/// Credit egui_extras: https://github.com/emilk/egui
/// Modification are:
/// - Open popover when clicking on a button
use eframe::egui::{InnerResponse, Response, ScrollArea, Ui, WidgetInfo, WidgetType};

use eframe::egui::*;
use egui::CursorIcon::Text;

pub struct PopupMenu {
    id_source: Id,
    width: Option<f32>,
    height: Option<f32>,
}

impl PopupMenu {
    pub fn new(id_source: impl std::hash::Hash) -> Self {
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

    let _is_popup_open = ui.memory(|m| m.is_popup_open(popup_id));

    let popup_height = 100.0;
    // let popup_height = ui.memory(|m| m.areas().get(popup_id).map_or(100.0, |state| state.size.y));

    let above_or_below =
        if ui.next_widget_position().y + ui.spacing().interact_size.y + popup_height
            < ui.ctx().screen_rect().bottom()
        {
            AboveOrBelow::Below
        } else {
            AboveOrBelow::Above
        };

    let button_response = button.ui(ui);
    if button_response.clicked() {
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
    }

    let height = height.unwrap_or_else(|| ui.spacing().combo_height);

    let inner = popup_above_or_below_widget(ui, popup_id, &button_response, above_or_below, |ui| {
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

pub fn popup_above_or_below_widget<R>(
    ui: &Ui,
    popup_id: Id,
    widget_response: &Response,
    above_or_below: AboveOrBelow,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    if ui.memory(|mem| mem.is_popup_open(popup_id)) {
        let (pos, pivot) = match above_or_below {
            AboveOrBelow::Above => (widget_response.rect.left_top(), Align2::LEFT_BOTTOM),
            AboveOrBelow::Below => (widget_response.rect.left_bottom(), Align2::LEFT_TOP),
        };

        let inner = Area::new(popup_id)
            .order(Order::Foreground)
            .constrain(true)
            .fixed_pos(pos)
            .pivot(pivot)
            .show(ui.ctx(), |ui| {
                let frame = Frame::popup(ui.style());
                let frame_margin = frame.total_margin();
                frame
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                            ui.set_width(widget_response.rect.width() - frame_margin.sum().x);
                            add_contents(ui)
                        })
                        .inner
                    })
                    .inner
            });

        if ui.input(|i| i.key_pressed(Key::Escape))
            || (!widget_response.clicked() && inner.response.clicked_elsewhere())
        {
            ui.memory_mut(|mem| mem.close_popup());
        }
        Some(inner.inner)
    } else {
        None
    }
}
