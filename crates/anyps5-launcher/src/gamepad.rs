//! Gamepad polling on its own thread. Silent until a button is pressed.

use std::thread;
use std::time::Duration;

use gilrs::{EventType, Gilrs};

/// Starts the poll loop. Failure to open the gamepad backend is logged once and is not fatal.
pub fn spawn(ctx: egui::Context) {
    let spawned = thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || poll(&ctx));
    if let Err(err) = spawned {
        tracing::warn!("gamepad thread not started: {err}");
    }
}

fn poll(ctx: &egui::Context) {
    let mut gilrs = match Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(err) => {
            tracing::warn!("gamepad backend unavailable: {err}");
            return;
        }
    };
    loop {
        while let Some(event) = gilrs.next_event() {
            if let EventType::ButtonPressed(button, _) = event.event {
                println!("gamepad {}: {button:?}", usize::from(event.id));
                ctx.request_repaint();
            }
        }
        thread::sleep(Duration::from_millis(8));
    }
}
