#![cfg_attr(
    not(debug_assertions),
    windows_subsystem = "windows"
)] // hide console window on Windows in release
#![feature(trait_upcasting)]


extern crate core;

mod table;
mod panels;
mod components;
mod flatten;
mod subtable_window;
mod parser;

use std::{env, fs, io, mem};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::exit;
use std::time::{Duration, Instant};
use eframe::egui;
use eframe::Theme::Light;
use egui::{Color32, Context, Separator, TextEdit, Vec2};
use serde_json::Value;
use crate::flatten::ValueType;
use crate::panels::{SelectColumnsPanel, SelectColumnsPanel_id};
use crate::parser::{JSONParser, my_lexer, ParseOptions};
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
    let options = eframe::NativeOptions {
        default_theme: Light,
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 { x: 1900.0, y: 1200.0 }).with_maximized(true),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native("Empty app", options, Box::new(|cc| {
        Box::new(MyApp::new())
    }));
}

struct MyApp {
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

        let content = fs::read_to_string(Path::new(args[1].as_str())).unwrap();

        // let mut lexer = Lexer::new(content.as_str());
        // let tokens = lexer.lex().unwrap();
        // // 55034885 tokens
        // println!("Custom Lexer took {}ms, {} tokens",start.elapsed().as_millis(), tokens.len());
        // let start = Instant::now();
        // let mut lexer = my_lexer::Lexer::new(content.as_bytes());
        // let tokens = lexer.lex();
        // 55034885 tokens
        // 50565637
        // println!("{:?}", tokens);
        // println!("Custom Lexer took {}ms, {} tokens",start.elapsed().as_millis(), tokens.len());
        let start = Instant::now();
        let mut parser = JSONParser::new(content.as_str());
        let vec1 = parser.parse(ParseOptions::default().start_parse_at("/skills").parse_array(false)).unwrap();
        // for (k, v) in &vec1 {
        //     let val = v.as_ref().unwrap();
        //     println!("{:?} {}", k.pointer, &(val[0..(val.len().min(10))]));
        // }
        println!("Custom parser took {}ms, max depth {}", start.elapsed().as_millis(), parser.parser.max_depth);
        exit(0);
        let start = Instant::now();
        let mut v: Value = serde_json::from_str(&content).unwrap();
        let mut max_depth = 0;
        let depth = 1;
        let mut count = 0usize;

        let mut root_node = mem::take(v.as_object_mut().unwrap().get_mut("skills").unwrap());

        println!("Parse took {}ms", start.elapsed().as_millis());
        let start = Instant::now();
        for node in root_node.as_array().unwrap().iter() {
            collect_keys(&node, "", depth, &mut max_depth, &mut count);
        }
        println!("Collect max depth {}ms", start.elapsed().as_millis());

        // println!("{:?}", all_columns);
        Self {
            table: Table::new(mem::take(root_node.as_array_mut().unwrap()), 1, "/".to_string(), ValueType::Array),
            windows: vec![
                Box::new(SelectColumnsPanel::default())
            ],
            max_depth: max_depth as u8,
            open: Default::default(),
            depth: 1,
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

fn collect_keys(node: &Value, parent: &str, depth: i32, max_depth: &mut i32, count: &mut usize) {
    if *max_depth < depth {
        *max_depth = depth;
    }
    if let Some(object) = node.as_object() {
        for (k, v) in object.iter() {
            let key = if parent.is_empty() {
                k.to_string()
            } else {
                format!("{}.{}", parent, k)
            };
            collect_keys(v, key.as_str(), depth + 1, max_depth, count);
        }
    }
}

