//! The launch intro (docs/DESIGN.md 3.7): the window opens black, the backdrop comes up out of it
//! while the boot cue swells, then the top bar, the tile row, the hero and the hints rise into
//! place one after another. It runs once per launch and takes 1.6 s; with reduced motion only a
//! short fade from black remains.
//!
//! [`start`] begins it; screens ask [`stage`] how far their part is and move it with [`lift`];
//! the host paints [`paint_curtain`] last. Without a start (previews, tests) everything is done.

use egui::{Color32, Context, Id, LayerId, Order, Rect, vec2};

use crate::ui::anim::{AnimExt, cubic_bezier};

/// Black before anything shows, then the fade from black.
const HOLD: f64 = 0.15;
const CURTAIN: f64 = 1.1;
const CURTAIN_REDUCED: f64 = 0.45;
/// Whole intro: the last stage ends here.
const LENGTH: f64 = 1.6;
/// How far a part rises while it fades in.
const RISE: f32 = 18.0;

/// When the parts of a screen arrive, as (delay, duration) in seconds from the start.
pub const TOP_BAR: (f32, f32) = (0.45, 0.7);
pub const TILES: (f32, f32) = (0.55, 0.75);
pub const HERO: (f32, f32) = (0.7, 0.8);
pub const HINTS: (f32, f32) = (0.9, 0.6);

fn start_id() -> Id {
    Id::unique("intro-start")
}

/// Starts the intro now. Call once, on the first frame of a launch.
pub fn start(ctx: &Context) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(start_id(), now));
}

/// Seconds since [`start`], or `None` when no intro runs (or it is over).
fn elapsed(ctx: &Context) -> Option<f64> {
    let started: f64 = ctx.data(|d| d.get_temp(start_id()))?;
    let elapsed = ctx.input(|i| i.time) - started;
    (elapsed < LENGTH).then_some(elapsed.max(0.0))
}

/// Progress of one part (0 = not there yet, 1 = in place) on a strong ease-out. Always 1 without
/// an intro or with reduced motion.
pub fn stage(ctx: &Context, (delay, duration): (f32, f32)) -> f32 {
    match elapsed(ctx) {
        Some(t) if !ctx.reduced_motion() => {
            ctx.request_repaint();
            let linear = ((t as f32 - delay) / duration).clamp(0.0, 1.0);
            cubic_bezier(0.23, 1.0, 0.32, 1.0, linear)
        }
        _ => 1.0,
    }
}

/// `screen` moved down by what is left of the rise, so a part drawn into it comes up from below.
pub fn lift(screen: Rect, t: f32) -> Rect {
    screen.translate(vec2(0.0, (1.0 - t) * RISE))
}

/// Black over everything while the intro begins: fully black for the hold, then fading out.
/// Paint it after everything else.
pub fn paint_curtain(ctx: &Context) {
    let Some(t) = elapsed(ctx) else {
        return;
    };
    ctx.request_repaint();
    let (hold, fade) = if ctx.reduced_motion() {
        (0.0, CURTAIN_REDUCED)
    } else {
        (HOLD, CURTAIN)
    };
    let linear = (((t - hold) / fade) as f32).clamp(0.0, 1.0);
    let alpha = 1.0 - cubic_bezier(0.42, 0.0, 0.58, 1.0, linear);
    if alpha <= 0.0 {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("intro-curtain")));
    painter.rect_filled(
        ctx.content_rect(),
        0.0,
        Color32::from_black_alpha((alpha * 255.0).round() as u8),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_start_everything_is_in_place() {
        let ctx = Context::default();
        assert_eq!(stage(&ctx, HERO), 1.0);
        assert!(elapsed(&ctx).is_none());
    }

    #[test]
    fn a_fresh_start_holds_every_part_back() {
        let ctx = Context::default();
        start(&ctx);
        assert_eq!(stage(&ctx, TOP_BAR), 0.0);
        assert_eq!(lift(Rect::ZERO, 0.0).min.y, RISE);
        assert_eq!(lift(Rect::ZERO, 1.0).min.y, 0.0);
    }

    #[test]
    fn the_parts_arrive_in_order_and_within_the_intro() {
        let stages = [TOP_BAR, TILES, HERO, HINTS];
        for pair in stages.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for (delay, duration) in stages {
            assert!(f64::from(delay + duration) <= LENGTH + 1e-6);
        }
    }
}
