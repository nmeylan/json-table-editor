#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![feature(trait_upcasting)]


mod table_demo;

use std::{env, fs, io, mem};
use std::collections::HashSet;
use std::fmt::Display;
use std::path::Path;
use eframe::egui;
use serde_json::Value;
use crate::table_demo::TableDemo;

/// Something to view in the demo windows
pub trait View {
    fn ui(&mut self, ui: &mut egui::Ui);
}

/// Something to view
pub trait Demo {
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
    let options = eframe::NativeOptions::default();
    eframe::run_native("Empty app", options, Box::new(|cc| {
        Box::new(MyApp::new())
    }));
}

struct MyApp {
    table: TableDemo,

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
        let mut unique_keys: HashSet<String> = HashSet::new();
        let mut v: Value = serde_json::from_str(&content).unwrap();
        let max_depth = 2;
        let mut depth = 0;
        print_key(&v, &mut unique_keys, depth, max_depth);

        let root_node = mem::take(v.as_object_mut().unwrap().get_mut("skills").unwrap());
        Self {
            table: TableDemo::new(unique_keys.into_iter().collect(), root_node),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.label("top panel");
        });
        egui::SidePanel::left("left").show(ctx, |ui| {
            ui.label("left panel");
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.table.ui(ui)
        });
    }
}

fn print_key(v: &Value, unique_keys: &mut HashSet<String>, depth: i32, max_depth: i32) {
    if v.is_array() {
        if let Some(array) = v.as_array() {
            for v in array.iter() {
                if depth < max_depth {
                    print_key(v, unique_keys, depth + 1, max_depth);
                }
            }
        }
    } else if v.is_object() {
        if let Some(object) = v.as_object() {
            for (k, v) in object.iter() {
                unique_keys.insert(k.to_string());
                if depth < max_depth {
                    print_key(v, unique_keys, depth + 1, max_depth);
                }
            }
        }
    } else {

    }
}

