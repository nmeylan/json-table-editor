#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![feature(trait_upcasting)]


mod table;
mod panels;

use std::{env, fs, io, mem};
use std::collections::{BTreeSet, HashSet};
use std::fmt::Display;
use std::path::Path;
use eframe::egui;
use eframe::Theme::Light;
use egui::{Context, Vec2};
use serde_json::Value;
use crate::panels::{SelectColumnsPanel, SelectColumnsPanel_id};
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

// fn main() {
//     let args: Vec<_> = env::args().collect();
//     if args.len() < 2 {
//         println!("Please provide file to open as 1st program argument");
//     } else {
//         println!("Opening {}", args[1].as_str());
//     }
//
//     let content = fs::read_to_string(Path::new(args[1].as_str())).unwrap();
//     let mut unique_keys: HashSet<String> = HashSet::new();
//     let v: Value = serde_json::from_str(&content).unwrap();
//     let max_depth = 2;
//     let mut depth = 0;
//     print_key(&v, &mut unique_keys, depth, max_depth);
//     for k in unique_keys {
//         println!("{}", k);
//     }
// }

fn main() {
    let options = eframe::NativeOptions {
        default_theme: Light,
        persist_window: false,
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2 {x: 1900.0, y: 1200.0}).with_maximized(true),
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
        let mut unique_keys: BTreeSet<String> = BTreeSet::new();
        let mut v: Value = serde_json::from_str(&content).unwrap();
        let mut max_depth = 0;
        let depth = 0;
        collect_keys(&v, &mut unique_keys, "", depth, &mut max_depth);

        let root_node = mem::take(v.as_object_mut().unwrap().get_mut("skills").unwrap());
        let all_columns = unique_keys.into_iter().collect();
        Self {
            table: Table::new(all_columns, root_node, 1),
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
            ui.label("top panel");
            if ui.button("select columns").clicked() {
                set_open(&mut self.open, SelectColumnsPanel_id, true);
            }
            let slider_response = ui.add(
                egui::Slider::new(&mut self.depth, 1..=self.max_depth).text("Depth"),
            );
            if slider_response.changed() {
                self.table.update_selected_columns(self.depth)
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.table.ui(ui)
        });
    }
}

fn collect_keys(v: &Value, unique_keys: &mut BTreeSet<String>, parent: &str, depth: i32, max_depth: &mut i32) {
    if *max_depth < depth {
        *max_depth = depth;
    }
    if v.is_array() {
        if let Some(array) = v.as_array() {
            for v in array.iter() {
                collect_keys(v, unique_keys, parent, depth + 1, max_depth);
            }
        }
    } else if v.is_object() {
        if let Some(object) = v.as_object() {
            for (k, v) in object.iter() {
                let key = if parent.is_empty() {
                    k.to_string()
                } else {
                    format!("{}.{}", parent, k)
                };
                unique_keys.insert(key.to_string());
                collect_keys(v, unique_keys, key.as_str(), depth + 1, max_depth);
            }
        }
    } else {}
}

