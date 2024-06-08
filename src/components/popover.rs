use egui::{InnerResponse, Response, ScrollArea, Ui, WidgetInfo, WidgetType};

use egui::{style::WidgetVisuals, *};

#[allow(unused_imports)] // Documentation
use egui::style::Spacing;

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
        self.show_ui_dyn(ui,button, Box::new(menu_contents))
    }

    fn show_ui_dyn<'c, R>(
        self,
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui) -> Response,
        menu_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    ) -> InnerResponse<Option<R>> {
        let Self {
            id_source,
            width,
            height,
        } = self;

        let button_id = ui.make_persistent_id(id_source);

        ui.horizontal(|ui| {
            let mut ir = popup(
                ui,
                button,
                button_id,
                menu_contents,
                height,
            );
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

    let is_popup_open = ui.memory(|m| m.is_popup_open(popup_id));

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

    let inner = egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &button_response,
        above_or_below,
        |ui| {
            ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    ui.style_mut().wrap = Some(false);
                    menu_contents(ui)
                })
                .inner
        },
    );

    InnerResponse {
        inner,
        response: button_response,
    }
}
