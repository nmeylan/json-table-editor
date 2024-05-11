extern crate core;

mod table;
mod panels;
mod components;
mod subtable_window;
pub mod parser;

use std::{env, fs};

use std::collections::{BTreeSet};

use std::path::Path;
use std::process::exit;
use crate::components::fps::FrameHistory;
use std::time::{Instant};
use eframe::NativeOptions;
use eframe::Theme::Light;
use egui::{Context, Separator, TextEdit, Vec2};
use crate::panels::{SelectColumnsPanel, SelectColumnsPanel_id};
use crate::parser::{JSONParser, ParseOptions, ValueType};
use crate::table::Table;

/// Something to view in the demo windows
pub trait View {
    fn ui(&mut self, ui: &mut egui::Ui);
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

#[derive(Default, Debug, Clone)]
struct Pos<T> {
    x: T,
    y: T,
}


fn main() {
    let options = NativeOptions {
        default_theme: Light,
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1900.0, y: 1200.0 }).with_maximized(true),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native("JSON table editor", options, Box::new(|_cc| {
        Box::new(MyApp::new())
    }));
}

struct MyApp {
    frame_history: FrameHistory,
    table: Table,
    windows: Vec<Box<dyn Window>>,
    open: BTreeSet<String>,
    max_depth: u8,
    depth: u8,
}


impl MyApp {
    fn new() -> Self {
        let args: Vec<_> = env::args().collect();
        if args.len() < 2 {
            println!("Please provide file to open as 1st program argument");
        } else {
            println!("Opening {}", args[1].as_str());
        }

        let path = Path::new(args[1].as_str());
        let mut content = fs::read_to_string(path).unwrap();
        let start = Instant::now();
        content = content.replace('\n', "");
        println!("took {}ms to replace LF", start.elapsed().as_millis());

        let metadata1 = fs::metadata(path).unwrap();

        let size = metadata1.len() / 1024 / 1024;
        let start = Instant::now();
        let mut parser = JSONParser::new(content.as_mut_str());
        let max_depth =if size < 10 {
            100
        } else if size < 50 {
            10
        } else {
            3
        };
        let options = ParseOptions::default().start_parse_at("/skills".to_string()).parse_array(false).max_depth(max_depth);
        let result = parser.parse(options.clone()).unwrap();

        let parse_result = result.clone_except_json();
        let max_depth = result.max_json_depth;
        println!("Custom parser took {}ms for a {}mb file, max depth {}, {}, root array len {}", start.elapsed().as_millis(), size, max_depth, result.json.len(), result.root_array_len);
        let start = Instant::now();
        let (result1, columns) = JSONParser::as_array(result).unwrap();
        println!("Transformation to array took {}ms, root array len {}, columns {}", start.elapsed().as_millis(), result1.len(), columns.len());
        // JSONParser::change_depth_array(parse_result, result1, 2);
        // exit(0);
        let parsing_max_depth = parse_result.parsing_max_depth;
        let depth = (parser.parser.depth_after_start_at as u8).min(parsing_max_depth as u8);
        Self {
            frame_history: FrameHistory::default(),
            table: Table::new(Some(parse_result), result1, columns, depth, "/skills".to_string(), ValueType::Array),
            windows: vec![
                Box::<SelectColumnsPanel>::default()
            ],
            max_depth: max_depth as u8,
            open: Default::default(),
            depth,
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
        self.frame_history
            .on_new_frame(ctx.input(|i| i.time), frame.info().cpu_usage);
        ctx.send_viewport_cmd_to(
            ctx.parent_viewport_id(),
            egui::ViewportCommand::Title(self.frame_history.fps().to_string()),
        );
        self.windows(ctx);
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("select columns").clicked() {
                    set_open(&mut self.open, SelectColumnsPanel_id, true);
                }
                ui.add(Separator::default().vertical());
                let slider_response = ui.add(
                    egui::Slider::new(&mut self.depth, 1..=self.max_depth).text("Depth"),
                );
                ui.add(Separator::default().vertical());
                ui.label("Scroll to column: ");
                let text_edit = TextEdit::singleline(&mut self.table.scroll_to_column).hint_text("Type name contains in column");
                let response = ui.add(text_edit);
                if response.changed() {
                    self.table.next_frame_scroll_to_column = true;
                }
                if slider_response.changed() {
                    self.table.update_max_depth(self.depth);
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.table.ui(ui)
        });
    }
}

