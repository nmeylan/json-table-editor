extern crate core;

mod array_table;
mod panels;
mod components;
mod subtable_window;
pub mod parser;
mod object_table;
pub mod fonts;

use std::{env, mem};

use std::collections::{BTreeSet};
use std::fs::File;
use std::io::Read;
use std::fmt::Write;

use std::path::{PathBuf};
use crate::components::fps::FrameHistory;
use std::time::{Instant};
use eframe::{CreationContext, NativeOptions};
use eframe::Theme::Light;
use egui::{Align2, Button, Color32, ComboBox, Context, IconData, Id, Label, LayerId, Order, RichText, Sense, Separator, TextEdit, TextStyle, Vec2, Widget};
use json_flat_parser::{FlatJsonValue, JSONParser, ParseOptions, ValueType};
use crate::panels::{SelectColumnsPanel};
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

#[derive(Default, Debug, Clone)]
struct Pos<T> {
    x: T,
    y: T,
}


fn main() {
    let options = NativeOptions {
        default_theme: Light,
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1200.0, y: 900.0 }).with_maximized(true).with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../icons/logo.png")).unwrap()),
        // viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1900.0, y: 1200.0 }).with_maximized(true),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native("JSON table editor", options, Box::new(|cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
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
    show_fps: bool
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
            windows: vec![
                Box::<SelectColumnsPanel>::default()
            ],
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
        let start = Instant::now();
        let mut content = String::with_capacity(metadata1.len() as usize);
        // let mut reader = LfToCrlfReader::new(file);
        // reader.read_to_string(&mut content);
        file.read_to_string(&mut content);
        println!("Read file took {}ms", start.elapsed().as_millis());
        let mut found_array = false;
        let mut found_object = false;
        for byte in content.as_bytes() {
            if *byte == b'[' {
                found_array = true;
                break;
            }
            if *byte == b'{' {
                found_object = true;
                break;
            }
        }
        if found_array || self.selected_pointer.is_some() {
            let start = Instant::now();
            let mut options = ParseOptions::default().parse_array(false).max_depth(max_depth);
            if let Some(ref start_at) = self.selected_pointer {
                options = options.start_parse_at(start_at.clone());
            }
            let result = JSONParser::parse(content.as_mut_str(), options).unwrap().to_owned();
            let parsing_max_depth = result.parsing_max_depth;
            println!("Custom parser took {}ms for a {}mb file, max depth {}, {}", start.elapsed().as_millis(), size, parsing_max_depth, result.json.len());
            let parse_result = result.clone_except_json();

            let start = Instant::now();
            let (result1, columns) = crate::parser::as_array(result).unwrap();
            println!("Transformation to array took {}ms, root array len {}, columns {}", start.elapsed().as_millis(), result1.len(), columns.len());

            let max_depth = parse_result.max_json_depth;
            let depth = (parse_result.depth_after_start_at + 1).min(parsing_max_depth);
            let mut prefix = "".to_owned();
            if let Some(ref start_at) = self.selected_pointer {
                prefix = start_at.clone();
            }
            let table = ArrayTable::new(Some(parse_result), result1, columns, depth, prefix, ValueType::Array(0));
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
            let result = JSONParser::parse(content.as_mut_str(), options.clone()).unwrap();
            self.should_parse_again = true;
            self.parsing_invalid = true;
            self.unsaved_changes = false;
            self.parsing_invalid_pointers = result.json.iter()
                .filter(|entry| matches!(entry.pointer.value_type, ValueType::Array(_)))
                .map(|entry| entry.pointer.pointer.clone()).collect();
        }
    }

    fn file_picker(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.selected_file = Some(path);
            self.should_parse_again = true;
            self.table = None;
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
        let mut title = format!("json table editor - {}{}",
                                self.selected_file.as_ref().map(|p| p.display().to_string()).unwrap_or("No file selected".to_string()),
                                if self.unsaved_changes { " *" } else { "" }
        );

        if self.show_fps {
            self.frame_history
                .on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);
            title = format!("{} - {:.2}", title, self.frame_history.fps())
        }

        ctx.send_viewport_cmd_to(ctx.parent_viewport_id(), egui::ViewportCommand::Title(title), );

        self.windows(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.table.is_some() {
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
                if let Some(ref mut table) = self.table {
                    ui.separator();
                    let slider_response = ui.add(
                        egui::Slider::new(&mut self.depth, self.min_depth..=self.max_depth).text("Depth"),
                    );
                    ui.add(Separator::default().vertical());
                    let scroll_to_column_response = ui.allocate_ui(Vec2::new(180.0, ui.spacing().interact_size.y), |ui| {
                        ui.add(Label::new("Scroll to column: ").wrap(false));
                        let text_edit = TextEdit::singleline(&mut table.scroll_to_column).hint_text("named");
                        ui.add(text_edit)
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
                                    if table.matching_row_selected == table.matching_rows.len() - 1 {
                                        table.matching_row_selected = 0;
                                    } else {
                                        table.matching_row_selected += 1;
                                    }
                                    table.changed_matching_row_selected = true;
                                }
                            }
                            (scroll_to_row_mode_response, scroll_to_row_response)
                        }).inner
                    }).inner;


                    // interaction handling
                    if scroll_to_column_response.changed() {
                        table.changed_scroll_to_column_value = true;
                    }
                    if scroll_to_row_response.changed() {
                        table.changed_scroll_to_row_value = Some(Instant::now());
                        if table.scroll_to_row.is_empty() {
                            table.reset_search();
                        }
                    }
                    if scroll_to_row_mode_response.inner.is_some() && scroll_to_row_mode_response.inner.unwrap() {
                        table.reset_search();
                    }
                    if slider_response.changed() {
                        if let Some(new_max_depth) = table.update_max_depth(self.depth) {
                            self.max_depth = new_max_depth as u8;
                        }
                    }
                }
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
                    self.selected_file = Some(i.raw.dropped_files.clone().pop().unwrap().path.unwrap());
                    self.table = None;
                    self.selected_pointer = None;
                    self.should_parse_again = true;
                    self.parsing_invalid = false;
                    self.parsing_invalid_pointers.clear();
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
                                           if response.inner.clicked() {
                                               self.file_picker();
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
                                ui.radio(true, pointer.as_str());
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
                    });
                } else if self.should_parse_again {
                    self.open_json();
                }
                // });
            }
        });
    }
}

