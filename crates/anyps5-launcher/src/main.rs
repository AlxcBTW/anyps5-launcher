//! AnyPS5 launcher: native Windows front end for AnyPS5.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod background;
mod gamepad;

use eframe::egui;

struct App;

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        }
        gamepad::spawn(cc.egui_ctx.clone());
        App
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                background::paint(ui, ui.max_rect());
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("AnyPS5")
                            .size(48.0)
                            .color(egui::Color32::from_rgb(0xE8, 0xEC, 0xF4)),
                    );
                });
            });
    }
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt().init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyPS5")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([960.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native("AnyPS5", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
