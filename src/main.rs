extern crate core;

mod array_table;
mod components;
mod subtable_window;
pub mod parser;
mod object_table;
pub mod fonts;
mod web;
mod compatibility;

use std::{env, mem};

use std::collections::{BTreeSet};
use std::fs::File;
use std::io::Read;
use std::fmt::Write;

use std::path::{PathBuf};
use std::sync::{Arc, Mutex};
use crate::components::fps::FrameHistory;

use eframe::{CreationContext};
use eframe::Theme::Light;
use egui::{Align2, Button, Color32, ComboBox, Context, CursorIcon, Id, Key, Label, LayerId, Order, RichText, Sense, Separator, TextEdit, TextStyle, Vec2, Widget};

use json_flat_parser::{FlatJsonValue, JSONParser, ParseOptions, ValueType};
use crate::array_table::{ArrayTable, ScrollToRowMode};
use crate::components::icon;
use crate::fonts::{CHEVRON_DOWN, CHEVRON_UP};
use crate::parser::save_to_file;

pub const ACTIVE_COLOR: Color32 = Color32::from_rgb(63, 142, 252);

/// Something to view in the demo windows
pub trait View<R> {
    fn ui(&mut self, ui: &mut egui::Ui) -> R;
}

/// Something to view
pub trait Window {
    /// Is the demo enabled for this integration?
    fn is_enabled(&self, _ctx: &egui::Context) -> bool {
        true
    }

    /// `&'static` so we can also use it as a key to store open/close state.
    fn name(&self) -> &'static str;

    /// Show windows, etc
    fn show(&mut self, ctx: &egui::Context, open: &mut bool);
}

#[derive(Default, Clone)]
struct ArrayResponse {
    pub(crate) edited_value: Option<FlatJsonValue<String>>,
}

impl ArrayResponse {
    pub fn union(&mut self, other: ArrayResponse) -> Self {
        let mut new_response = mem::take(self);
        if new_response.edited_value.is_none() && other.edited_value.is_some() {
            new_response.edited_value = other.edited_value;
        }
        new_response
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let options = eframe::NativeOptions {
            default_theme: Light,
            persist_window: false,
            viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1200.0, y: 900.0 }).with_maximized(true).with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../icons/logo.png")).unwrap()),
            // viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1900.0, y: 1200.0 }).with_maximized(true),
            ..eframe::NativeOptions::default()
        };
        eframe::run_native("JSON table editor", options, Box::new(|cc| {
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
            Box::new(app)
        })).unwrap();
    }
}

struct MyApp {
    frame_history: FrameHistory,
    table: Option<ArrayTable>,
    windows: Vec<Box<dyn Window>>,
    open: BTreeSet<String>,
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
    web_loaded_json: Arc<Mutex<Option<Vec<u8>>>>,
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        let mut fonts = egui::FontDefinitions::default();

        let font_data = egui::FontData::from_static(include_bytes!("../icons/fa-solid-900.ttf"));
        fonts.font_data.insert(
            "fa".into(),
            font_data,
        );
        fonts.families.insert(
            egui::FontFamily::Name("fa".into()),
            vec!["Ubuntu-Light".into(), "fa".into()],
        );
        cc.egui_ctx.set_fonts(fonts);
        // let path = Path::new(args[1].as_str());
        Self {
            frame_history: FrameHistory::default(),
            table: None,
            windows: vec![],
            max_depth: 0,
            open: Default::default(),
            depth: 0,
            selected_file: None,
            parsing_invalid: false,
            should_parse_again: false,
            parsing_invalid_pointers: vec![],
            selected_pointer: None,
            min_depth: 0,
            unsaved_changes: false,
            show_fps: true,
            web_loaded_json: Arc::new(Mutex::new(None)),
        }
    }
    pub fn windows(&mut self, ctx: &Context) {
        let Self { windows, open, .. } = self;
        for window in windows {
            let mut is_open = open.contains(window.name());
            window.show(ctx, &mut is_open);
            set_open(open, window.name(), is_open);
        }
    }

    pub fn open_json(&mut self) {
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

    fn open_json_content(&mut self, max_depth: u8, json: &[u8]) {
        let mut found_array = false;
        let size = json.len() / 1024 / 1024;
        log!("open_json_content with size {}mb, found array {}", size, found_array);
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
            let mut options = ParseOptions::default().parse_array(false).max_depth(max_depth);
            if let Some(ref start_at) = self.selected_pointer {
                options = options.start_parse_at(start_at.clone());
            }
            let parse_result = JSONParser::parse_bytes(json, options);

            let result = parse_result.unwrap().to_owned();
            let parsing_max_depth = result.parsing_max_depth;
            log!("Custom parser took {}ms for a {}mb file, max depth {}, {}", start.elapsed().as_millis(), size, parsing_max_depth, result.json.len());
            let parse_result = result.clone_except_json();

            let start = crate::compatibility::now();
            let (result1, columns) = crate::parser::as_array(result).unwrap();
            log!("Transformation to array took {}ms, root array len {}, columns {}", start.elapsed().as_millis(), result1.len(), columns.len());

            let max_depth = parse_result.max_json_depth;
            let depth = (parse_result.depth_after_start_at + 1).min(parsing_max_depth);
            let mut prefix = "".to_owned();
            if let Some(ref start_at) = self.selected_pointer {
                prefix = start_at.clone();
            }
            let table = ArrayTable::new(Some(parse_result), result1, columns, depth, prefix);
            self.table = Some(table);
            self.depth = depth;
            self.max_depth = max_depth as u8;
            self.min_depth = depth;
            self.parsing_invalid_pointers.clear();
            self.should_parse_again = false;
            self.parsing_invalid = false;
            self.selected_pointer = None;
            self.unsaved_changes = false;
        } else {
            let options = ParseOptions::default().parse_array(false).max_depth(max_depth);
            let result = JSONParser::parse_bytes(json, options.clone()).unwrap();
            self.should_parse_again = true;
            self.parsing_invalid = true;
            self.unsaved_changes = false;
            #[cfg(target_arch = "wasm32")]
            {
                let mut json_guard = self.web_loaded_json.lock().unwrap();
                *json_guard = Some(json.to_vec());
            }
            self.parsing_invalid_pointers = result.json.iter()
                .filter(|entry| matches!(entry.pointer.value_type, ValueType::Array(_)))
                .map(|entry| entry.pointer.pointer.clone()).collect();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn file_picker(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.selected_file = Some(path);
            self.should_parse_again = true;
            self.table = None;
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

    #[cfg(target_arch = "wasm32")]
    fn web_try_open_json_bytes(&mut self) {
        let mut json_guard = self.web_loaded_json.try_lock();
        let has_json = json_guard.is_ok();
        if has_json {
            let mut json_guard = json_guard.unwrap();
            let has_json = json_guard.is_some();
            if has_json {
                let option = mem::take(&mut *json_guard);
                drop(json_guard);
                self.open_json_content(u8::MAX, option.unwrap().as_slice());
                self.selected_file = Some(PathBuf::default());
            }
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

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(not(target_arch = "wasm32"))] {
            let mut title = format!("json table editor - {}{}",
                                    self.selected_file.as_ref().map(|p| p.display().to_string()).unwrap_or("No file selected".to_string()),
                                    if self.unsaved_changes { " *" } else { "" }
            );

            if self.show_fps {
                self.frame_history
                    .on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);
                title = format!("{} - {:.2}", title, self.frame_history.fps())
            }

            ctx.send_viewport_cmd_to(ctx.parent_viewport_id(), egui::ViewportCommand::Title(title));
        }
        self.windows(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.table.is_some() {
                    #[cfg(not(target_arch = "wasm32"))] {
                        ui.menu_button("File", |ui| {
                            ui.set_min_width(220.0);
                            ui.style_mut().wrap = Some(false);
                            if ui.button("Open json file").clicked() {
                                ui.close_menu();
                                self.file_picker();
                            }
                            ui.separator();
                            if ui.button("Save").clicked() {
                                ui.close_menu();
                                let table = self.table.as_ref().unwrap();
                                save_to_file(table.parent_pointer.as_str(), table.nodes(), self.selected_file.as_ref().unwrap()).unwrap();
                                self.unsaved_changes = false;
                            }
                            ui.separator();
                            if ui.button("Save as").clicked() {
                                ui.close_menu();
                                if let Some(path) = rfd::FileDialog::new().save_file() {
                                    self.selected_file = Some(path);
                                    let table = self.table.as_ref().unwrap();
                                    save_to_file(table.parent_pointer.as_str(), table.nodes(), self.selected_file.as_ref().unwrap()).unwrap();
                                    self.unsaved_changes = false;
                                }
                            }
                        });
                    }
                }
                if let Some(ref mut table) = self.table {
                    ui.separator();
                    let change_depth_slider_response = ui.add(
                        egui::Slider::new(&mut self.depth, self.min_depth..=self.max_depth).text("Depth"),
                    );
                    ui.add(Separator::default().vertical());
                    let scroll_to_column_response = ui.allocate_ui(Vec2::new(180.0, ui.spacing().interact_size.y), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(Label::new("Scroll to column: ").wrap(false));
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
                            ui.add(Label::new("Scroll to row: ").wrap(false));
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
                    ui.colored_label(Color32::RED, "Web version is only here as a demo, performances are better on desktop version.");
                    ui.hyperlink_to("Download desktop version on GitHub for a better experience", "https://github.com/nmeylan/json-table-editor/releases");
                });
            }
        });

        if self.table.is_some() {
            let table = self.table.as_ref().unwrap();
            egui::TopBottomPanel::bottom("bottom-panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} rows ", table.nodes.len()));
                    ui.separator();
                    ui.label(format!("{} columns ", table.all_columns().len()));
                    ui.separator();
                    ui.label(format!("{} depth level", self.max_depth));
                    if !table.parent_pointer.is_empty() {
                        ui.separator();
                        ui.label(format!("Start pointer: {}", table.parent_pointer));
                    }
                    if !table.columns_filter.is_empty() {
                        ui.separator();
                        if ui.label(RichText::new(format!("{} active filters", table.columns_filter.len())).underline())
                            .on_hover_ui(|ui| {
                                ui.vertical(|ui| {
                                    table.columns_filter.iter().for_each(|(k, _)| { ui.label(k); })
                                });
                            }).hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::Help);
                        }
                    }
                });
            });
        }
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
                if response1.edited_value.is_some() {
                    self.unsaved_changes = true;
                }
            } else if self.selected_file.is_none() {
                ui.allocate_ui_at_rect(ui.max_rect(),
                                       |ui| {
                                           let response = ui.centered_and_justified(|ui| {
                                               ui.heading("Select or drop a json file")
                                           });
                                           #[cfg(not(target_arch = "wasm32"))] {
                                               if response.inner.clicked() {
                                                   self.file_picker();
                                               }
                                           }

                                           #[cfg(target_arch = "wasm32")]
                                           {
                                               let mut json = self.web_loaded_json.clone();
                                               let future = async move {
                                                   if response.inner.clicked() {
                                                       if let Some(file_handle) = rfd::AsyncFileDialog::new().pick_file().await {
                                                           let mut json = json.lock().unwrap();
                                                           *json = Some(file_handle.read().await);
                                                       }
                                                   }
                                               };
                                               wasm_bindgen_futures::spawn_local(future);
                                               self.web_try_open_json_bytes();
                                           }
                                       },
                );
            }
            if self.selected_file.is_some() {
                if self.parsing_invalid {
                    ui.vertical_centered(|ui| {
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
                            #[cfg(not(target_arch = "wasm32"))] {
                                self.open_json();
                            }
                            #[cfg(target_arch = "wasm32")] {
                                self.web_try_open_json_bytes();
                            }
                        }
                        if Button::new("Select another file").sense(Sense::click()).ui(ui).clicked() {
                            self.selected_file = None;
                            self.selected_pointer = None;
                            self.should_parse_again = true;
                            self.parsing_invalid = false;
                            self.parsing_invalid_pointers.clear();
                        }
                    });
                } else if self.should_parse_again {
                    #[cfg(not(target_arch = "wasm32"))] {
                        self.open_json();
                    }
                    #[cfg(target_arch = "wasm32")] {
                        self.web_try_open_json_bytes();
                    }
                }
                // });
            }
        });
    }
}

