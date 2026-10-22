//! AnyPS5 launcher: native Windows front end for AnyPS5.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

use eframe::egui;

use crate::ui::Shell;
use crate::ui::theme;

/// Placeholder host until the screens land: the animated backdrop and the product name.
struct App {
    shell: Shell,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            shell: Shell::new(cc),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _frame_input = self.shell.begin_frame(&ctx);
        let screen = ctx.content_rect();
        let painter = ui.painter().clone();
        self.shell.paint_backdrop(&ctx, &painter, screen);
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            "AnyPS5",
            theme::ty::HERO_TITLE.font(),
            egui::Color32::WHITE,
        );
    }
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt().init();
    if let Some(options) = ui::gallery::parse_args(std::env::args().skip(1)) {
        return ui::gallery::run(options);
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyPS5")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([theme::MIN_WINDOW.0, theme::MIN_WINDOW.1])
            .with_maximized(true),
        ..Default::default()
    };
    ui::restrict_backends(&mut options);
    eframe::run_native("AnyPS5", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
