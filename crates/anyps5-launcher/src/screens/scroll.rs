//! Scroll-to-focus (docs/DESIGN.md animation 31): a list scrolls so that the focused item sits in
//! the middle, with the focus spring (`scrollTo(anchor: .center)` inside `Motion.focus`). The mouse
//! wheel still scrolls freely; the next focus change takes over again.
//!
//! Usage per frame: [`Follow::begin`] before the scroll area (it yields the offset to apply), draw
//! the items, then [`Follow::end`] with where they ended up.

use egui::{Context, Id};

use crate::ui::anim::{AnimExt, Motion};

/// Scroll offset that centres an item, never past the ends.
pub fn centred_offset(top: f32, height: f32, viewport: f32, content: f32) -> f32 {
    let max = (content - viewport).max(0.0);
    (top + height * 0.5 - viewport * 0.5).clamp(0.0, max)
}

/// What the follower remembers between frames.
#[derive(Clone, Default)]
struct Memory {
    /// `(top, height)` of every item in content coordinates, from the last frame.
    items: Vec<(f32, f32)>,
    content_height: f32,
    viewport_height: f32,
    focus: Option<usize>,
    /// The offset egui used last frame.
    offset: f32,
    /// The offset this follower last asked for.
    set: f32,
    session: u32,
    following: bool,
}

/// One frame of following.
pub struct Follow {
    memory_id: Id,
    memory: Memory,
}

impl Follow {
    /// Starts the frame. Returns the follower and the offset to give the scroll area (`None` leaves
    /// the area alone, so the wheel works).
    pub fn begin(ctx: &Context, id: Id, focus: Option<usize>) -> (Follow, Option<f32>) {
        let memory_id = id.with("follow-memory");
        let mut mem: Memory = ctx.data(|d| d.get_temp(memory_id)).unwrap_or_default();
        if focus != mem.focus {
            // A wheel scroll since the last follow leaves the spring's value stale: start a new one
            // from where the list really is.
            let dirty = (mem.offset - mem.set).abs() > 1.0;
            if dirty || mem.items.is_empty() {
                mem.session += 1;
            }
            mem.following = focus.is_some();
            mem.focus = focus;
        }
        let mut offset = None;
        if mem.following
            && let Some(&(top, height)) = focus.and_then(|f| mem.items.get(f))
        {
            let target = centred_offset(top, height, mem.viewport_height, mem.content_height);
            let value = ctx.animate_from_to(
                (id, "follow", mem.session),
                mem.offset,
                target,
                Motion::Focus,
            );
            mem.set = value;
            offset = Some(value);
            if (value - target).abs() < 0.5 {
                mem.following = false;
            }
        }
        (
            Follow {
                memory_id,
                memory: mem,
            },
            offset,
        )
    }

    /// Ends the frame with the geometry the items were drawn at.
    pub fn end(
        mut self,
        ctx: &Context,
        items: Vec<(f32, f32)>,
        content_height: f32,
        viewport_height: f32,
        offset: f32,
    ) {
        self.memory.items = items;
        self.memory.content_height = content_height;
        self.memory.viewport_height = viewport_height;
        self.memory.offset = offset;
        ctx.data_mut(|d| d.insert_temp(self.memory_id, self.memory));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_centred_but_never_past_the_ends() {
        assert_eq!(centred_offset(0.0, 58.0, 400.0, 1000.0), 0.0);
        assert_eq!(centred_offset(500.0, 58.0, 400.0, 1000.0), 329.0);
        assert_eq!(centred_offset(980.0, 58.0, 400.0, 1000.0), 600.0);
        assert_eq!(
            centred_offset(10.0, 58.0, 400.0, 300.0),
            0.0,
            "short content"
        );
    }
}
