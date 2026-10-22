//! AnyPS5 launcher: native Windows front end for AnyPS5.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


mod app;
mod screens;
mod ui;

fn main() -> eframe::Result {
    tracing_subscriber::fmt().init();
    // Hidden modes for screenshots and trying the flows: `--gallery=<scene>` of the home screen and
    // the onboarding, `--demo`, and the foundation's own `--gallery[=components|panels|...]`.
    if let Some(options) = app::preview::parse_args(std::env::args().skip(1)) {
        return app::preview::run(options);
    }
    if let Some(options) = ui::gallery::parse_args(std::env::args().skip(1)) {
        return ui::gallery::run(options);
    }
    app::run()
}
