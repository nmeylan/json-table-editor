extern crate core;

mod array_table;
mod compatibility;
mod components;
pub mod fonts;
mod object_table;
mod panels;
pub mod parser;
mod replace_panel;
mod subtable_window;
mod web;

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt::{format, Write};
use std::fs::File;
use std::io::Read;
use std::{env, mem};

use crate::components::fps::FrameHistory;
use parking_lot_mpsc::{Receiver, SyncSender};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::array_table::{ArrayTable, ScrollToRowMode};
use crate::components::icon;
use crate::components::table::HoverData;
use crate::fonts::{CHEVRON_DOWN, CHEVRON_UP};
use crate::panels::{AboutPanel, PANEL_ABOUT};
use crate::parser::save_to_file;
use eframe::egui::Context;
use eframe::egui::{
    Align, Align2, Button, Color32, ComboBox, CursorIcon, Id, Key, KeyboardShortcut, Label,
    LayerId, Layout, Modifiers, Order, RichText, Sense, Separator, TextEdit, TextStyle, Vec2,
    Widget,
};
use eframe::epaint::text::TextWrapMode;
use eframe::{CreationContext, Renderer};
use egui::style::ScrollStyle;
use egui::{ScrollArea, TextBuffer};
use json_flat_parser::{FlatJsonValue, JSONParser, ParseOptions, PointerKey, ValueType};

pub const ACTIVE_COLOR: Color32 = Color32::from_rgb(63, 142, 252);

pub const SHORTCUT_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
pub const SHORTCUT_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
pub const SHORTCUT_COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
pub const SHORTCUT_PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
pub const SHORTCUT_DELETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
pub const SHORTCUT_REPLACE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);

/// Something to view in the demo windows
pub trait View<R> {
    fn ui(&mut self, ui: &mut egui::Ui) -> R;
}

/// Something to view
pub trait Window<R> {
    /// Is the demo enabled for this integration?
    fn is_enabled(&self, _ctx: &eframe::egui::Context) -> bool {
        true
    }

    /// `&'static` so we can also use it as a key to store open/close state.
    fn name(&self) -> &'static str;

    /// Show windows, etc
    fn show(&mut self, ctx: &eframe::egui::Context, open: &mut bool) -> R;
}

#[derive(Default, Clone)]
struct ArrayResponse {
    pub(crate) edited_value: Vec<FlatJsonValue<String>>,
    pub(crate) hover_data: HoverData,
    pub(crate) focused_cell: bool,
}

impl ArrayResponse {
    pub fn union(&mut self, other: ArrayResponse) -> Self {
        let mut new_response = mem::take(self);
        new_response.edited_value.extend(other.edited_value);
        if new_response.hover_data.hovered_cell.is_none() && other.hover_data.hovered_cell.is_some()
        {
            new_response.hover_data.hovered_cell = other.hover_data.hovered_cell;
        }
        new_response
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let options = eframe::NativeOptions {
            persist_window: false,
            renderer: Renderer::Glow,
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size(Vec2 {
                    x: 1200.0,
                    y: 900.0,
                })
                .with_maximized(true)
                .with_icon(Arc::new(
                    eframe::icon_data::from_png_bytes(include_bytes!("../icons/logo.png")).unwrap(),
                )),
            // viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1900.0, y: 1200.0 }).with_maximized(true),
            ..eframe::NativeOptions::default()
        };
        eframe::run_native(
            "JSON table editor",
            options,
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);
                let mut style = (*cc.egui_ctx.style()).clone();
                style.spacing.scroll.floating = false;
                style.spacing.scroll.bar_width = 4.0;
                style.spacing.scroll.bar_inner_margin = 6.0;
                cc.egui_ctx.set_style(style);
                let mut app = MyApp::new(cc);

                let args: Vec<_> = env::args().collect();
                if args.len() >= 2 {
                    println!("Opening {}", args[1].as_str());
                    app.selected_file = Some(PathBuf::from(args[1].as_str()));
                    app.should_parse_again = true;
                }
                if args.len() >= 3 {
                    app.selected_pointer = Some(args[2].clone());
                }
                Ok(Box::new(app))
            }),
        )
        .unwrap();
    }
}

struct MyApp<'array> {
    frame_history: FrameHistory,
    table: Option<ArrayTable<'array>>,
    open: BTreeSet<String>,
    about_panel: AboutPanel,
    max_depth: u8,
    depth: u8,
    selected_file: Option<PathBuf>,
    should_parse_again: bool,
    parsing_invalid: bool,
    parsing_invalid_pointers: Vec<String>,
    selected_pointer: Option<String>,
    min_depth: u8,
    unsaved_changes: bool,
    show_fps: bool,
    web_loaded_json: Option<Vec<u8>>,
    async_events_channel: (SyncSender<AsyncEvent>, Receiver<AsyncEvent>),
    failed_to_load_sample_json: Option<String>,
    force_repaint: bool,
}

enum AsyncEvent {
    LoadJson(Vec<u8>),
    LoadSampleErr(String),
}

impl<'array> MyApp<'array> {
    fn new(cc: &CreationContext) -> Self {
        let mut fonts = eframe::egui::FontDefinitions::default();

        let font_data =
            eframe::egui::FontData::from_static(include_bytes!("../icons/fa-solid-900.ttf"));
        fonts.font_data.insert("fa".into(), font_data);
        fonts.families.insert(
            eframe::egui::FontFamily::Name("fa".into()),
            vec!["fa".into()],
        );
        cc.egui_ctx.set_fonts(fonts);
        let (sender, receiver) = parking_lot_mpsc::sync_channel::<AsyncEvent>(1);
        // let path = Path::new(args[1].as_str());
        Self {
            frame_history: FrameHistory::default(),
            table: None,
            open: Default::default(),
            about_panel: Default::default(),
            max_depth: 0,
            depth: 0,
            selected_file: None,
            parsing_invalid: false,
            should_parse_again: false,
            parsing_invalid_pointers: vec![],
            selected_pointer: None,
            min_depth: 0,
            unsaved_changes: false,
            show_fps: true,
            web_loaded_json: None,
            async_events_channel: (sender, receiver),
            failed_to_load_sample_json: None,
            force_repaint: false,
        }
    }
    pub fn windows(&mut self, ctx: &Context) {
        let Self { open, .. } = self;
        let mut is_open = open.contains(self.about_panel.name());
        self.about_panel.show(ctx, &mut is_open);
        set_open(open, self.about_panel.name(), is_open);
    }

    pub fn open_json(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut file = File::open(self.selected_file.as_ref().unwrap()).unwrap();
            let metadata1 = file.metadata().unwrap();

            let size = (metadata1.len() / 1024 / 1024) as usize;
            let max_depth = if size < 100 {
                // 1
                u8::MAX
            } else {
                1 // should start after prefix
            };
            let mut content = String::with_capacity(metadata1.len() as usize);
            // let mut reader = LfToCrlfReader::new(file);
            // reader.read_to_string(&mut content);
            file.read_to_string(&mut content).unwrap();

            self.open_json_content(max_depth, content.as_bytes());
        }
        #[cfg(target_arch = "wasm32")]
        {
            if self.web_loaded_json.is_some() {
                let json = mem::take(&mut self.web_loaded_json);
                self.open_json_content(u8::MAX, json.unwrap().as_slice());
                self.selected_file = Some(PathBuf::default());
            }
        }
    }

    fn open_json_content(&mut self, max_depth: u8, json: &[u8]) {
        let mut found_array = false;
        let size = json.len() / 1024 / 1024;
        log!(
            "open_json_content with size {}mb, found array {}",
            size,
            found_array
        );
        for byte in json {
            if *byte == b'[' {
                found_array = true;
                break;
            }
            if *byte == b'{' {
                break;
            }
        }
        if found_array || self.selected_pointer.is_some() {
            let start = crate::compatibility::now();
            let mut options = ParseOptions::default()
                .parse_array(false)
                .max_depth(max_depth);
            if let Some(ref start_at) = self.selected_pointer {
                options = options.start_parse_at(start_at.clone());
            }
            let parse_result = JSONParser::parse_bytes(json, options);

            let result = parse_result.unwrap().to_owned();
            let parsing_max_depth = result.parsing_max_depth;
            log!(
                "Custom parser took {}ms for a {}mb file, max depth {}, {}",
                start.elapsed().as_millis(),
                size,
                parsing_max_depth,
                result.json.len()
            );
            let parse_result = result.clone_except_json();

            let start = crate::compatibility::now();
            let (result1, columns) = crate::parser::as_array(result).unwrap();
            log!(
                "Transformation to array took {}ms, root array len {}, columns {}",
                start.elapsed().as_millis(),
                result1.len(),
                columns.len()
            );

            let max_depth = parse_result.max_json_depth;
            let depth =
                (parse_result.depth_after_start_at + 1).max(parsing_max_depth.min(max_depth as u8));
            let min_depth = if parse_result.depth_after_start_at + 1 > 1 {
                parse_result.depth_after_start_at + 1
            } else {
                1
            };
            let mut prefix = "".to_owned();
            if let Some(ref start_at) = self.selected_pointer {
                prefix = start_at.clone();
            }
            let len = result1.len();
            let table = ArrayTable::new(
                Some(parse_result),
                result1,
                columns,
                depth,
                PointerKey::from_pointer(prefix, ValueType::Array(len), 1, 0),
            );
            self.table = Some(table);
            self.depth = depth;
            self.max_depth = max_depth as u8;
            self.min_depth = min_depth;
            self.parsing_invalid_pointers.clear();
            self.should_parse_again = false;
            self.parsing_invalid = false;
            self.selected_pointer = None;
            self.unsaved_changes = false;
        } else {
            let options = ParseOptions::default()
                .parse_array(false)
                .max_depth(max_depth);
            let result = JSONParser::parse_bytes(json, options.clone()).unwrap();
            self.should_parse_again = true;
            self.parsing_invalid = true;
            self.unsaved_changes = false;
            #[cfg(target_arch = "wasm32")]
            {
                self.web_loaded_json = Some(json.to_vec());
            }
            self.parsing_invalid_pointers = result
                .json
                .iter()
                .filter(|entry| matches!(entry.pointer.value_type, ValueType::Array(_)))
                .map(|entry| entry.pointer.pointer.clone())
                .collect();
        }
    }

    fn file_picker(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                self.selected_file = Some(path);
                self.should_parse_again = true;
                self.table = None;
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let sender = self.async_events_channel.0.clone();
            self.force_repaint = true;
            let future = async move {
                if let Some(file_handle) = rfd::AsyncFileDialog::new().pick_file().await {
                    sender.send(AsyncEvent::LoadJson(file_handle.read().await));
                }
            };
            wasm_bindgen_futures::spawn_local(future);
        }
    }

    fn goto_next_matching_row_occurrence(table: &mut ArrayTable) -> bool {
        if table.matching_rows.is_empty() {
            return false;
        }
        if table.matching_row_selected == table.matching_rows.len() - 1 {
            table.matching_row_selected = 0;
        } else {
            table.matching_row_selected += 1;
        }
        table.changed_matching_row_selected = true;
        true
    }

    fn goto_next_matching_column_occurrence(table: &mut ArrayTable) -> bool {
        if table.matching_columns.is_empty() {
            return false;
        }
        if table.matching_column_selected == table.matching_columns.len() - 1 {
            table.matching_column_selected = 0;
        } else {
            table.matching_column_selected += 1;
        }
        table.changed_matching_column_selected = true;
        true
    }

    fn save(&mut self) {
        let table = self.table.as_ref().unwrap();
        save_to_file(
            table.parent_pointer.pointer.as_str(),
            table.nodes(),
            self.selected_file.as_ref().unwrap(),
        )
        .unwrap();
        self.unsaved_changes = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn save_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new().save_file() {
            self.selected_file = Some(path);
            let table = self.table.as_ref().unwrap();
            save_to_file(
                table.parent_pointer.pointer.as_str(),
                table.nodes(),
                self.selected_file.as_ref().unwrap(),
            )
            .unwrap();
            self.unsaved_changes = false;
        }
    }
}

fn set_open(open: &mut BTreeSet<String>, key: &'static str, is_open: bool) {
    if is_open {
        if !open.contains(key) {
            open.insert(key.to_owned());
        }
    } else {
        open.remove(key);
    }
}

impl<'array> eframe::App for MyApp<'array> {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // ctx.set_theme(Theme::Light);
        ctx.style_mut(|style| {
            style.spacing.scroll = ScrollStyle::thin();
            style.spacing.scroll.bar_width = 4.0;
            style.spacing.scroll.floating = false;
            style.spacing.scroll.foreground_color = true;
        });
        if let Ok(event) = self.async_events_channel.1.try_recv() {
            self.force_repaint = false;
            match event {
                AsyncEvent::LoadJson(json_bytes) => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        self.web_loaded_json = Some(json_bytes);
                        self.open_json();
                    }
                }
                AsyncEvent::LoadSampleErr(err) => {
                    self.failed_to_load_sample_json = Some(err);
                    ctx.request_repaint();
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut title = format!(
                "json table editor - {}{}",
                self.selected_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or("No file selected".to_string()),
                if self.unsaved_changes { " *" } else { "" }
            );

            if self.show_fps {
                self.frame_history
                    .on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);
                title = format!("{} - {:.2}", title, self.frame_history.fps())
            }

            ctx.send_viewport_cmd_to(
                ctx.parent_viewport_id(),
                egui::ViewportCommand::Title(title),
            );
        }
        self.windows(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.table.is_some() {
                    ui.menu_button("File", |ui| {
                        ui.set_min_width(220.0);
                        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                        if ui.button("Open json file").clicked() {
                            ui.close_menu();
                            self.file_picker();
                        }
                        #[cfg(not(target_arch = "wasm32"))] {
                            ui.separator();
                            let button = Button::new("Save").shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_SAVE));
                            if ui.add(button).clicked() {
                                ui.close_menu();
                                self.save();
                            }
                            ui.separator();
                            let button = Button::new("Save as").shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_SAVE_AS));
                            if ui.add(button).clicked() {
                                ui.close_menu();
                                self.save_as();
                            }
                        }
                    });

                    ui.separator();
                    ui.menu_button("Edit", |ui| {
                        ui.set_min_width(220.0);
                        let replace_button = Button::new("Replace").shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_REPLACE));
                        if ui.add(replace_button).clicked() {
                            ui.close_menu();
                            self.table.as_mut().unwrap().open_replace_panel(None);
                        }
                    });
                }
                if let Some(ref mut table) = self.table {
                    ui.separator();
                    let change_depth_slider_response = ui.add(
                        egui::Slider::new(&mut self.depth, self.min_depth..=self.max_depth).text("Depth"),
                    );
                    ui.add(Separator::default().vertical());
                    let scroll_to_column_response = ui.allocate_ui(Vec2::new(180.0, ui.spacing().interact_size.y), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(Label::new("Scroll to column: ").extend());
                            let text_edit = TextEdit::singleline(&mut table.scroll_to_column).hint_text("named");
                            let response = ui.add(text_edit);
                            if !table.matching_columns.is_empty() {
                                let response_prev = icon::button(ui, CHEVRON_UP, Some("Previous occurrence"), None);
                                let response_next = icon::button(ui, CHEVRON_DOWN, Some("Next occurrence"), None);
                                ui.label(RichText::new(format!("{}/{}", table.matching_column_selected + 1, table.matching_columns.len())));

                                if response_prev.clicked() {
                                    if table.matching_column_selected == 0 {
                                        table.matching_column_selected = table.matching_columns.len() - 1;
                                    } else {
                                        table.matching_column_selected -= 1;
                                    }
                                    table.changed_matching_column_selected = true;
                                }
                                if response_next.clicked() {
                                    Self::goto_next_matching_column_occurrence(table);
                                }
                            }
                            response
                        }).inner
                    }).inner;

                    ui.add(Separator::default().vertical());

                    let (scroll_to_row_mode_response, scroll_to_row_response) = ui.allocate_ui(Vec2::new(410.0, ui.spacing().interact_size.y), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(Label::new("Scroll to row: ").extend());
                            let scroll_to_row_mode_response = ComboBox::from_id_source("scroll_mode").selected_text(table.scroll_to_row_mode.as_str()).show_ui(ui, |ui| {
                                ui.selectable_value(&mut table.scroll_to_row_mode, ScrollToRowMode::RowNumber, ScrollToRowMode::RowNumber.as_str()).changed()
                                    || ui.selectable_value(&mut table.scroll_to_row_mode, ScrollToRowMode::MatchingTerm, ScrollToRowMode::MatchingTerm.as_str()).changed()
                            });
                            let hint_text = match &table.scroll_to_row_mode {
                                ScrollToRowMode::RowNumber => "Type row number",
                                ScrollToRowMode::MatchingTerm => "Type term contained in string value"
                            };
                            let text_edit = TextEdit::singleline(&mut table.scroll_to_row).hint_text(hint_text);
                            let scroll_to_row_response = ui.add(text_edit);
                            if !table.matching_rows.is_empty() {
                                let response_prev = icon::button(ui, CHEVRON_UP, Some("Previous occurrence"), None);
                                let response_next = icon::button(ui, CHEVRON_DOWN, Some("Next occurrence"), None);
                                ui.label(RichText::new(format!("{}/{}", table.matching_row_selected + 1, table.matching_rows.len())));

                                if response_prev.clicked() {
                                    if table.matching_row_selected == 0 {
                                        table.matching_row_selected = table.matching_rows.len() - 1;
                                    } else {
                                        table.matching_row_selected -= 1;
                                    }
                                    table.changed_matching_row_selected = true;
                                }
                                if response_next.clicked() {
                                    Self::goto_next_matching_row_occurrence(table);
                                }
                            }
                            (scroll_to_row_mode_response, scroll_to_row_response)
                        }).inner
                    }).inner;


                    // interaction handling
                    if scroll_to_column_response.changed() {
                        table.changed_scroll_to_column_value = true;
                    } else if scroll_to_column_response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) && Self::goto_next_matching_column_occurrence(table) {
                        scroll_to_column_response.request_focus();
                    }
                    if scroll_to_row_response.changed() {
                        table.changed_scroll_to_row_value = Some(crate::compatibility::now());
                        if table.scroll_to_row.is_empty() {
                            table.reset_search();
                        }
                    } else if scroll_to_row_response.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) && Self::goto_next_matching_row_occurrence(table) {
                        scroll_to_row_response.request_focus();
                    }
                    if scroll_to_row_mode_response.inner.is_some() && scroll_to_row_mode_response.inner.unwrap() {
                        table.reset_search();
                    }
                    if change_depth_slider_response.changed() {
                        table.changed_scroll_to_column_value = true;
                        if let Some(new_max_depth) = table.update_max_depth(self.depth) {
                            self.max_depth = new_max_depth as u8;
                        }
                    }
                }
            });
            #[cfg(target_arch = "wasm32")] {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::RED, "Currently, Web version is only here as a demo, performances are better on desktop version.");
                    ui.hyperlink_to("Download desktop version on GitHub for a better experience", "https://github.com/nmeylan/json-table-editor/releases");
                });
            }
        });

        egui::TopBottomPanel::bottom("bottom-panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.table.is_some() {
                    let table = self.table.as_ref().unwrap();
                    ui.label(format!("{} rows ", table.nodes.len()));
                    ui.separator();
                    ui.label(format!("{} columns ", table.all_columns().len()));
                    ui.separator();
                    ui.label(format!("{} depth level", self.max_depth));
                    if !table.parent_pointer.pointer.is_empty() {
                        ui.separator();
                        ui.label(format!("Start pointer: {}", table.parent_pointer.pointer));
                    }
                    if !table.columns_filter.is_empty() {
                        ui.separator();
                        if ui
                            .label(
                                RichText::new(format!(
                                    "{} active filters",
                                    table.columns_filter.len()
                                ))
                                .underline(),
                            )
                            .on_hover_ui(|ui| {
                                ui.vertical(|ui| {
                                    table.columns_filter.iter().for_each(|(k, _)| {
                                        ui.label(k.as_str());
                                    })
                                });
                            })
                            .hovered()
                        {
                            ui.ctx().set_cursor_icon(CursorIcon::Help);
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let about_button = ui.add(Button::new("About").frame(false));
                    if about_button.clicked() {
                        set_open(&mut self.open, PANEL_ABOUT, true);
                    }
                    if about_button.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                })
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
                let text = ctx.input(|i| {
                    let mut text = "Dropping files:\n".to_owned();
                    for file in &i.raw.hovered_files {
                        if let Some(path) = &file.path {
                            write!(text, "\n{}", path.display()).ok();
                        } else if !file.mime.is_empty() {
                            write!(text, "\n{}", file.mime).ok();
                        } else {
                            text += "\n???";
                        }
                    }
                    text
                });

                let painter =
                    ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

                let screen_rect = ctx.screen_rect();
                painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));
                painter.text(
                    screen_rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    TextStyle::Heading.resolve(&ctx.style()),
                    Color32::WHITE,
                );
            }

            // Collect dropped files:
            ctx.input(|i| {
                if !i.raw.dropped_files.is_empty() {
                    let file = i.raw.dropped_files.clone().pop().unwrap();
                    self.table = None;
                    self.selected_pointer = None;
                    self.should_parse_again = true;
                    self.parsing_invalid = false;
                    self.parsing_invalid_pointers.clear();
                    if let Some(bytes) = file.bytes {
                        self.open_json_content(u8::MAX, bytes.as_ref());
                    } else {
                        self.selected_file = Some(file.path.unwrap());
                    }
                }
            });

            if let Some(ref mut table) = self.table {
                let response1 = table.ui(ui);
                if !response1.edited_value.is_empty() {
                    self.unsaved_changes = true;
                }
            } else if self.selected_file.is_none() {
                let max_rect = ui.max_rect();
                let mut rect = ui.max_rect();
                rect.min.y = rect.max.y / 2.0 - 20.0;
                let mut already_interact = false;

                if !already_interact {
                    let response = ui.interact(max_rect, Id::new("select_file"), Sense::click());
                    if response.clicked() {
                        self.file_picker();
                    }
                }
                ui.allocate_ui_at_rect(rect,
                                       |ui| {
                                           ui.vertical_centered(|ui| {
                                               ui.heading("Select or drop a json file");

                                               #[cfg(target_arch = "wasm32")] {
                                                   if ui.button("Or load sample json file of 1mb").clicked() {
                                                       self.failed_to_load_sample_json = None;
                                                       already_interact = true;
                                                       let request = ehttp::Request::get("https://raw.githubusercontent.com/nmeylan/json-table-editor/master/web/skill.json");
                                                       self.force_repaint = true;
                                                       let sender = self.async_events_channel.0.clone();
                                                       ehttp::fetch(request, move |result: ehttp::Result<ehttp::Response>| {
                                                           if let Ok(result) = result {
                                                               if result.status == 200 {
                                                                   sender.send(AsyncEvent::LoadJson(result.bytes));
                                                               } else {
                                                                   sender.send(AsyncEvent::LoadSampleErr(format!("Failed to load sample file: [{}]", result.status)));
                                                               }
                                                               return;
                                                           } else {
                                                               sender.send(AsyncEvent::LoadSampleErr(format!("Failed to load sample file")));
                                                           }

                                                       });
                                                   }
                                                   if let Some(ref failed_to_load_sample_json) = self.failed_to_load_sample_json {
                                                       ui.colored_label(Color32::RED, failed_to_load_sample_json);
                                                   }
                                                   if ui.hyperlink_to("Sample source available here", "https://raw.githubusercontent.com/nmeylan/json-table-editor/master/web/skill.json").clicked() {
                                                       already_interact = true;
                                                   }
                                               }
                                           });
                                       },
                );
            }
            if self.selected_file.is_some() {
                if self.parsing_invalid {
                    let mut rect = ui.max_rect();
                    rect.min.y = 40.0_f32.max(rect.max.y / 2.0 - (20.0 * self.parsing_invalid_pointers.len() as f32));
                    ui.allocate_ui_at_rect(rect,
                                           |ui| {
                                               ui.vertical_centered(|ui| {
                                                   let scroll_area = ScrollArea::vertical();
                                                   scroll_area.show(ui, |ui| {
                                                       ui.heading("Provided json is not an array but an object");
                                                       ui.heading("Select which array you want to parse");
                                                       self.parsing_invalid_pointers.iter().for_each(|pointer| {
                                                           if self.selected_pointer.is_some() && self.selected_pointer.as_ref().unwrap().eq(pointer) {
                                                               let _ = ui.radio(true, pointer.as_str());
                                                           } else if ui.radio(false, pointer.as_str()).clicked() {
                                                               self.selected_pointer = Some(pointer.clone());
                                                           }
                                                       });
                                                       let sense = if self.selected_pointer.is_none() {
                                                           Sense::hover()
                                                       } else {
                                                           Sense::click()
                                                       };
                                                       if Button::new("Parse again").sense(sense).ui(ui).clicked() {
                                                           self.open_json();
                                                       }
                                                       if Button::new("Select another file").sense(Sense::click()).ui(ui).clicked() {
                                                           self.selected_file = None;
                                                           self.selected_pointer = None;
                                                           self.should_parse_again = true;
                                                           self.parsing_invalid = false;
                                                           self.parsing_invalid_pointers.clear();
                                                       }
                                                   })

                                               });
                                           });
                } else if self.should_parse_again {
                    self.open_json();
                }
            }
        });
        if self.table.is_some() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                ctx.input_mut(|i| {
                    if i.consume_shortcut(&SHORTCUT_SAVE_AS) {
                        self.save_as();
                    }
                    if i.consume_shortcut(&SHORTCUT_SAVE) {
                        self.save();
                    }
                })
            }
        }

        if self.force_repaint {
            ctx.request_repaint();
        }
    }
}
