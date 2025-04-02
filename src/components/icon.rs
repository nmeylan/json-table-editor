use crate::components::icon;
use eframe::egui::{
    Button, Color32, NumExt, Response, Sense, Style, TextStyle, TextWrapMode, Ui, Widget,
    WidgetInfo, WidgetText, WidgetType,
};
use eframe::emath::{pos2, Vec2};
use eframe::epaint;
use eframe::epaint::{Rounding, Stroke};

pub fn icon(name: &'static str) -> egui::RichText {
    icon_with_size(name, 12.0)
}
pub fn icon_with_size(name: &'static str, size: f32) -> egui::RichText {
    egui::RichText::new(name)
        .family(egui::FontFamily::Name("fa".into()))
        .size(size)
}

pub fn button(
    ui: &mut Ui,
    name: &'static str,
    tooltip: Option<&str>,
    color: Option<Color32>,
) -> Response {
    let mut icon = icon::icon(name);
    if color.is_some() {
        icon = icon.color(color.unwrap());
    }
    let button = Button::new(icon);
    let mut response = ui.add(button);
    if let Some(tooltip) = tooltip {
        response = response.on_hover_ui(|ui| {
            ui.label(tooltip);
        });
    }

    response
}

pub struct ButtonWithIcon {
    text: Option<WidgetText>,
    icon: &'static str,

    shortcut_text: WidgetText,
    wrap: Option<TextWrapMode>,

    fill: Option<Color32>,
    stroke: Option<Stroke>,
    sense: Sense,
    small: bool,
    frame: Option<bool>,
    min_size: Vec2,
    rounding: Option<Rounding>,
    selected: bool,
}

impl ButtonWithIcon {
    pub fn new(text: impl Into<WidgetText>, icon: &'static str) -> Self {
        Self {
            text: Some(text.into()),
            icon,
            shortcut_text: Default::default(),
            wrap: Some(TextWrapMode::Extend),
            fill: None,
            stroke: None,
            sense: Sense::click(),
            small: false,
            frame: None,
            min_size: Default::default(),
            rounding: None,
            selected: false,
        }
    }
    #[inline]
    pub fn wrap(mut self, wrap: bool) -> Self {
        if wrap {
            self.wrap = Some(TextWrapMode::Wrap);
        } else {
            self.wrap = Some(TextWrapMode::Extend);
        }
        self
    }

    #[inline]
    pub fn fill(mut self, fill: impl Into<Color32>) -> Self {
        self.fill = Some(fill.into());
        self.frame = Some(true);
        self
    }

    #[inline]
    pub fn stroke(mut self, stroke: impl Into<Stroke>) -> Self {
        self.stroke = Some(stroke.into());
        self.frame = Some(true);
        self
    }

    #[inline]
    pub fn small(mut self) -> Self {
        if let Some(text) = self.text {
            self.text = Some(text.text_style(TextStyle::Body));
        }
        self.small = true;
        self
    }

    #[inline]
    pub fn frame(mut self, frame: bool) -> Self {
        self.frame = Some(frame);
        self
    }

    #[inline]
    pub fn sense(mut self, sense: Sense) -> Self {
        self.sense = sense;
        self
    }

    #[inline]
    pub fn min_size(mut self, min_size: Vec2) -> Self {
        self.min_size = min_size;
        self
    }

    #[inline]
    pub fn rounding(mut self, rounding: impl Into<Rounding>) -> Self {
        self.rounding = Some(rounding.into());
        self
    }

    #[inline]
    pub fn shortcut_text(mut self, shortcut_text: impl Into<WidgetText>) -> Self {
        self.shortcut_text = shortcut_text.into();
        self
    }

    #[inline]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for ButtonWithIcon {
    fn ui(self, ui: &mut Ui) -> Response {
        let ButtonWithIcon {
            text,
            icon,
            shortcut_text,
            wrap,
            fill,
            stroke,
            sense,
            small,
            frame,
            min_size,
            rounding,
            selected,
        } = self;

        let frame = frame.unwrap_or_else(|| ui.visuals().button_frame);

        let mut button_padding = if frame {
            ui.spacing().button_padding
        } else {
            Vec2::ZERO
        };
        if small {
            button_padding.y = 0.0;
        }

        let space_available_for_icon = if let Some(text) = &text {
            let font_height = ui.fonts(|fonts| font_height(text, fonts, ui.style()));
            Vec2::splat(font_height) // Reasonable?
        } else {
            ui.available_size() - 2.0 * button_padding
        };

        let icon_size = space_available_for_icon;
        let icon = WidgetText::from(icon_with_size(icon, icon_size.y - 2.0));

        let mut text_wrap_width = ui.available_width() - 2.0 * button_padding.x;
        text_wrap_width -= icon_size.x + ui.spacing().icon_spacing;
        if !shortcut_text.is_empty() {
            text_wrap_width -= 60.0; // Some space for the shortcut text (which we never wrap).
        }

        let galley =
            text.map(|text| text.into_galley(ui, wrap, text_wrap_width, TextStyle::Button));
        let icon_galley = icon.into_galley(ui, wrap, text_wrap_width, TextStyle::Button);
        let shortcut_galley = (!shortcut_text.is_empty()).then(|| {
            shortcut_text.into_galley(
                ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Button,
            )
        });

        let mut desired_size = Vec2::ZERO;

        desired_size.x += ui.spacing().icon_spacing;

        if let Some(text) = &galley {
            desired_size.x += text.size().x;
            desired_size.y = desired_size.y.max(text.size().y);
        }

        desired_size.x += icon_galley.size().x;
        desired_size.y = desired_size.y.max(icon_galley.size().y);

        if let Some(shortcut_text) = &shortcut_galley {
            desired_size.x += ui.spacing().item_spacing.x + shortcut_text.size().x;
            desired_size.y = desired_size.y.max(shortcut_text.size().y);
        }
        desired_size += 2.0 * button_padding;
        if !small {
            desired_size.y = desired_size.y.at_least(ui.spacing().interact_size.y);
        }
        desired_size = desired_size.at_least(min_size);

        let (rect, response) = ui.allocate_at_least(desired_size, sense);
        response.widget_info(|| {
            if let Some(galley) = &galley {
                WidgetInfo::labeled(WidgetType::Button, true, galley.text())
            } else {
                WidgetInfo::new(WidgetType::Button)
            }
        });

        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);

            let (frame_expansion, frame_rounding, frame_fill, frame_stroke) = if selected {
                let selection = ui.visuals().selection;
                (
                    Vec2::ZERO,
                    Rounding::ZERO,
                    selection.bg_fill,
                    selection.stroke,
                )
            } else if frame {
                let expansion = Vec2::splat(visuals.expansion);
                (
                    expansion,
                    visuals.rounding,
                    visuals.weak_bg_fill,
                    visuals.bg_stroke,
                )
            } else {
                Default::default()
            };
            let frame_rounding = rounding.unwrap_or(frame_rounding);
            let frame_fill = fill.unwrap_or(frame_fill);
            let frame_stroke = stroke.unwrap_or(frame_stroke);
            ui.painter().rect(
                rect.expand2(frame_expansion),
                frame_rounding,
                frame_fill,
                frame_stroke,
            );

            let mut cursor_x = rect.min.x + button_padding.x;

            let text_pos = pos2(cursor_x, rect.center().y - 0.5 * icon_galley.size().y);
            cursor_x += icon_galley.size().x;
            ui.painter()
                .galley(text_pos, icon_galley, visuals.text_color());

            if let Some(galley) = galley {
                cursor_x += ui.spacing().icon_spacing;
                let text_pos = pos2(cursor_x, rect.center().y - 0.5 * galley.size().y);
                ui.painter().galley(text_pos, galley, visuals.text_color());
            }

            if let Some(shortcut_galley) = shortcut_galley {
                let shortcut_text_pos = pos2(
                    rect.max.x - button_padding.x - shortcut_galley.size().x,
                    rect.center().y - 0.5 * shortcut_galley.size().y,
                );
                ui.painter().galley(
                    shortcut_text_pos,
                    shortcut_galley,
                    ui.visuals().weak_text_color(),
                );
            }
        }

        if let Some(cursor) = ui.visuals().interact_cursor {
            if response.hovered {
                ui.ctx().set_cursor_icon(cursor);
            }
        }

        response
    }
}

fn font_height(text: &WidgetText, fonts: &epaint::Fonts, style: &Style) -> f32 {
    match text {
        WidgetText::RichText(text) => text.font_height(fonts, style),
        WidgetText::LayoutJob(job) => job.font_height(fonts),
        WidgetText::Galley(galley) => {
            if let Some(row) = galley.rows.first() {
                row.height()
            } else {
                galley.size().y
            }
        }
    }
}
