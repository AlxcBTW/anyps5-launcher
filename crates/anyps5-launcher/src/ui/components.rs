//! Shared widgets (docs/DESIGN.md section 5 and 2.4): console buttons, icon buttons, switch,
//! progress lines, keycaps and the hint bar.
//!
//! All widgets are drawn with the painter and driven by the *model*: the screen passes
//! `focused` from its focus model (`ui::focus`); egui's own keyboard focus is not used. Each widget
//! animates its focus with `Motion::Focus` and shows hover (mouse only) and press (0.97) states.
//! A disabled widget is drawn at 40 % and takes no input.

use egui::epaint::{Mesh, RectShape, Shadow, Vertex};
use egui::{
    Align2, Color32, CornerRadius, Painter, Pos2, Rect, Response, Role, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2,
};

use crate::ui::anim::{self, AnimExt, Motion, lerp};
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{TextStyle, black, color, fade, mix, radius, shadows, size, ty, white};

/// Forces a visual state so galleries and tests can show hover or press without a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    Hover,
    Press,
}

/// The press scale of buttons (a mouse affordance added on Windows, DESIGN.md 9.2).
pub const PRESS_SCALE: f32 = 0.97;

/// Rectangle scaled about its centre.
pub fn scale_rect(rect: Rect, factor: f32) -> Rect {
    Rect::from_center_size(rect.center(), rect.size() * factor)
}

/// Converts a radius in dp to egui's integer corner radius.
pub fn corner(radius: f32) -> CornerRadius {
    CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8)
}

/// Paints a shadow behind `rect`.
pub fn paint_shadow(painter: &Painter, rect: Rect, radius: f32, shadow: Shadow) {
    if shadow.color.a() > 0 {
        painter.add(shadow.as_shape(rect, corner(radius)));
    }
}

/// The glow of a focused element (white 0.45, SwiftUI radius 10) around its focus ring. Paint it
/// **before** the element so the element covers the glow's interior; then paint the element and
/// [`paint_focus_stroke`] on top.
pub fn paint_focus_glow(painter: &Painter, rect: Rect, radius: f32, width: f32, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t > 0.001 {
        paint_shadow(
            painter,
            rect.expand(width * 1.6),
            radius + width,
            shadows::focus_glow(t),
        );
    }
}

/// The white ring of a focused element (`focusRing`): a stroke of `width` drawn outside the
/// element, outset by `width * 1.6`. `t` is the focus amount; paint after the element.
pub fn paint_focus_stroke(painter: &Painter, rect: Rect, radius: f32, width: f32, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t > 0.001 {
        painter.add(RectShape::stroke(
            rect.expand(width * 1.6),
            corner(radius + width),
            Stroke::new(width, white(0.95 * t)),
            StrokeKind::Inside,
        ));
    }
}

/// 1 px hairline drawn inside the shape (SwiftUI `strokeBorder`).
pub fn paint_hairline(painter: &Painter, rect: Rect, radius: f32, color: Color32) {
    if color.a() > 0 {
        painter.add(RectShape::stroke(
            rect,
            corner(radius),
            Stroke::new(1.0, color),
            StrokeKind::Inside,
        ));
    }
}

/// Fills a capsule.
pub fn paint_capsule(painter: &Painter, rect: Rect, color: Color32) {
    painter.rect_filled(rect, corner(rect.height() * 0.5), color);
}

// ---------------------------------------------------------------------------------------------
// Shared interaction/animation state
// ---------------------------------------------------------------------------------------------

/// What a widget needs to draw itself this frame.
pub struct Interaction {
    pub response: Response,
    /// The widget rectangle at rest (before scale effects).
    pub rect: Rect,
    /// Focus amount 0..=1 (spring).
    pub focus: f32,
    /// Press amount 0..=1.
    pub press: f32,
    pub hovered: bool,
    pub enabled: bool,
}

/// Allocates `size`, reads the pointer state and runs the focus and press springs.
pub fn interact(
    ui: &mut Ui,
    size: Vec2,
    focused: bool,
    enabled: bool,
    force: Option<Force>,
) -> Interaction {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hovered = enabled
        && (response.hovered() || force == Some(Force::Hover) || force == Some(Force::Press));
    let pressed = enabled && (response.is_pointer_button_down_on() || force == Some(Force::Press));
    let ctx = ui.ctx();
    let focus = ctx.focus_t((response.id, "focus"), focused && enabled);
    let press = ctx.animate_to(
        (response.id, "press"),
        if pressed { 1.0 } else { 0.0 },
        Motion::Focus,
    );
    if hovered && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    Interaction {
        response,
        rect,
        focus,
        press,
        hovered,
        enabled,
    }
}

impl Interaction {
    /// Scale from focus (`focus_scale` at full focus) and press.
    pub fn scale(&self, focus_scale: f32) -> f32 {
        lerp(1.0, focus_scale, self.focus) * lerp(1.0, PRESS_SCALE, self.press)
    }

    /// Multiplier for the whole widget when disabled.
    pub fn alpha(&self) -> f32 {
        if self.enabled { 1.0 } else { 0.4 }
    }
}

// ---------------------------------------------------------------------------------------------
// Console button
// ---------------------------------------------------------------------------------------------

/// A translucent rounded button that turns solid white when focused.
pub struct ConsoleButton<'a> {
    title: &'a str,
    icon: Option<Icon>,
    icon_fill: bool,
    focused: bool,
    prominent: bool,
    destructive: bool,
    min_width: f32,
    enabled: bool,
    force: Option<Force>,
    /// Accessible name when the button shows no text.
    label: Option<String>,
}

impl<'a> ConsoleButton<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            label: None,
            icon: None,
            icon_fill: false,
            focused: false,
            prominent: false,
            destructive: false,
            min_width: 0.0,
            enabled: true,
            force: None,
        }
    }

    /// A button with only a glyph (18 dp side padding). `label` is its accessible name.
    pub fn icon_only(icon: Icon, label: impl Into<String>) -> ConsoleButton<'static> {
        let mut button = ConsoleButton::new("").icon(icon);
        button.label = Some(label.into());
        button
    }

    /// The name assistive technology reads when it differs from the visible text.
    pub fn accessible_name(mut self, name: impl Into<String>) -> Self {
        self.label = Some(name.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Use the fill weight of the glyph (play, lock, trophy, ...).
    pub fn icon_fill(mut self, fill: bool) -> Self {
        self.icon_fill = fill;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    /// The primary action of a row: fill white 0.20.
    pub fn prominent(mut self, prominent: bool) -> Self {
        self.prominent = prominent;
        self
    }
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn force(mut self, force: Option<Force>) -> Self {
        self.force = force;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let painter = ui.painter().clone();
        let label = text::layout(
            &painter,
            ty::BUTTON,
            self.title,
            Color32::WHITE,
            f32::INFINITY,
        );
        let has_text = !self.title.is_empty();
        let pad = if has_text {
            size::BUTTON_PADDING
        } else {
            size::BUTTON_PADDING_ICON_ONLY
        };
        let icon_width = if self.icon.is_some() {
            size::BUTTON_ICON
        } else {
            0.0
        };
        let gap = if self.icon.is_some() && has_text {
            size::BUTTON_GAP
        } else {
            0.0
        };
        let text_width = if has_text { label.size().x } else { 0.0 };
        let content = icon_width + gap + text_width;
        let width = (pad * 2.0 + content).max(self.min_width);
        let height = size::BUTTON_MIN_HEIGHT.max(ty::BUTTON.line_height());

        let it = interact(
            ui,
            vec2(width, height),
            self.focused,
            self.enabled,
            self.force,
        );
        let enabled = self.enabled;
        let name = match (&self.label, self.title.is_empty()) {
            (Some(label), _) => label.as_str(),
            (None, true) => "",
            (None, false) => self.title,
        };
        it.response
            .widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
        if !ui.is_rect_visible(it.rect) {
            return it.response;
        }
        let s = it.scale(1.05);
        let rect = scale_rect(it.rect, s);
        let a = it.alpha();
        let rest = if self.prominent {
            color::surface_prominent()
        } else if it.hovered {
            color::surface_hover()
        } else {
            color::surface()
        };
        let fill = mix(rest, color::surface_focused(), it.focus);
        let foreground = mix(
            if self.destructive {
                color::destructive_icon()
            } else {
                Color32::WHITE
            },
            color::text_on_focus(),
            it.focus,
        );
        let r = radius::CONSOLE_BUTTON * s;
        paint_shadow(&painter, rect, r, shadows::console_button(it.focus));
        painter.rect_filled(rect, corner(r), fade(fill, a));
        paint_hairline(
            &painter,
            rect,
            r,
            fade(color::hairline(), (1.0 - it.focus) * a),
        );

        // Content, centred, scaled with the button.
        let text_style = text::scaled(ty::BUTTON, s);
        let mut x = rect.center().x - content * s * 0.5;
        if let Some(icon) = self.icon {
            let glyph = icon_width * s;
            painter.text(
                pos2(x + glyph * 0.5, rect.center().y),
                Align2::CENTER_CENTER,
                icon.glyph(),
                icon.font(text::quantize_size(size::BUTTON_ICON * s), self.icon_fill),
                fade(foreground, a),
            );
            x += glyph + gap * s;
        }
        if has_text {
            text::paint(
                &painter,
                pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                text_style,
                self.title,
                fade(foreground, a),
            );
        }
        it.response
    }
}

// ---------------------------------------------------------------------------------------------
// Icon button (top bar)
// ---------------------------------------------------------------------------------------------

/// A 44 dp circle with a glyph (library, settings). Focused: white fill, black glyph, scale 1.1.
pub struct IconButton {
    icon: Icon,
    label: String,
    fill: bool,
    focused: bool,
    enabled: bool,
    force: Option<Force>,
}

impl IconButton {
    /// `label` is the accessible name (the glyph carries no text).
    pub fn new(icon: Icon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            fill: true,
            focused: false,
            enabled: true,
            force: None,
        }
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn force(mut self, force: Option<Force>) -> Self {
        self.force = force;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let it = interact(
            ui,
            Vec2::splat(size::ICON_BUTTON),
            self.focused,
            self.enabled,
            self.force,
        );
        let (label, enabled) = (self.label.as_str(), self.enabled);
        it.response
            .widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
        if !ui.is_rect_visible(it.rect) {
            return it.response;
        }
        let painter = ui.painter();
        let s = it.scale(1.1);
        let rect = scale_rect(it.rect, s);
        let a = it.alpha();
        let rest = if it.hovered {
            color::surface_hover()
        } else {
            color::surface()
        };
        let fill = mix(rest, color::surface_focused(), it.focus);
        paint_shadow(
            painter,
            rect,
            radius::ICON_BUTTON * s,
            shadows::icon_button(it.focus),
        );
        painter.rect_filled(rect, corner(radius::ICON_BUTTON * s), fade(fill, a));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.icon.glyph(),
            self.icon
                .font(text::quantize_size(size::ICON_BUTTON_GLYPH * s), self.fill),
            fade(mix(Color32::WHITE, Color32::BLACK, it.focus), a),
        );
        it.response
    }
}

// ---------------------------------------------------------------------------------------------
// Switch
// ---------------------------------------------------------------------------------------------

/// The 50 x 28 on/off switch. The caller flips its state when the response is clicked.
pub struct Switch {
    on: bool,
    accent: Color32,
    /// Focus amount of the row the switch sits in (0..=1): the off track turns black 0.18.
    focus: f32,
    label: String,
    enabled: bool,
    /// Expose the switch to assistive technology (off when the row already does).
    announce: bool,
}

impl Switch {
    pub fn new(on: bool, accent: Color32) -> Self {
        Self {
            on,
            accent,
            focus: 0.0,
            label: String::new(),
            enabled: true,
            announce: true,
        }
    }
    /// The row is focused: the off track turns black 0.18.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focus = if focused { 1.0 } else { 0.0 };
        self
    }
    /// Animated focus amount of the surrounding row (0..=1).
    pub fn focus_amount(mut self, focus: f32) -> Self {
        self.focus = focus.clamp(0.0, 1.0);
        self
    }
    /// Accessible name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    /// A disabled switch is drawn at 40 % and takes no input.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Whether the switch is its own accessibility node (default). A row that already announces
    /// its toggle state turns it off so screen readers do not read it twice.
    pub fn announce(mut self, announce: bool) -> Self {
        self.announce = announce;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(size::SWITCH.0, size::SWITCH.1), sense);
        let (on, label, enabled) = (self.on, self.label.as_str(), self.enabled);
        if self.announce {
            response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, on, label));
        }
        let hovered = self.enabled && response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let t = ui.ctx().flag_t((response.id, "on"), self.on, Motion::Focus);
        let rest = if hovered {
            white(0.24)
        } else {
            color::switch_track_off()
        };
        let off = mix(rest, color::switch_track_off_on_focus(), self.focus);
        let a = if self.enabled { 1.0 } else { 0.4 };
        let painter = ui.painter();
        paint_capsule(painter, rect, fade(mix(off, self.accent, t), a));
        let knob = size::SWITCH.1 - 2.0 * size::SWITCH_INSET;
        let travel = size::SWITCH.0 - 2.0 * size::SWITCH_INSET - knob;
        let centre = pos2(
            rect.min.x + size::SWITCH_INSET + knob * 0.5 + travel * t,
            rect.center().y,
        );
        let knob_rect = Rect::from_center_size(centre, Vec2::splat(knob));
        if self.enabled {
            paint_shadow(painter, knob_rect, knob * 0.5, shadows::switch_knob());
        }
        painter.circle_filled(centre, knob * 0.5, fade(Color32::WHITE, a));
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------------------------

/// A thin capsule progress bar; the fill width eases with easeOut 0.35 s per change.
pub struct ProgressLine {
    fraction: f32,
    color: Color32,
    height: f32,
    width: Option<f32>,
    label: String,
}

impl ProgressLine {
    pub fn new(fraction: f32, color: Color32) -> Self {
        Self {
            fraction,
            color,
            height: size::PROGRESS_HEIGHT,
            width: None,
            label: String::new(),
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// Fixed width; otherwise the bar fills the available width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Accessible name (the current step, for example).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, response) = ui.allocate_exact_size(vec2(width, self.height), Sense::hover());
        let fraction = self.fraction.clamp(0.0, 1.0);
        response.widget_info(|| {
            let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, self.label.as_str());
            info.value = Some(f64::from(fraction));
            info
        });
        if ui.is_rect_visible(rect) {
            let eased = ui
                .ctx()
                .animate_to((response.id, "fill"), fraction, Motion::Progress);
            paint_progress(ui.painter(), rect, eased, self.color);
        }
        response
    }
}

/// Track plus fill (with the accent glow) for an already eased fraction.
pub fn paint_progress(painter: &Painter, rect: Rect, fraction: f32, accent: Color32) {
    paint_capsule(painter, rect, color::track());
    let fill_width = (rect.width() * fraction.clamp(0.0, 1.0)).max(rect.height());
    let fill = Rect::from_min_size(rect.min, vec2(fill_width, rect.height()));
    paint_shadow(
        painter,
        fill,
        rect.height() * 0.5,
        shadows::progress_glow(accent),
    );
    paint_capsule(painter, fill, accent);
}

/// An endless sweep for steps of unknown length: a 35 %-wide gradient (clear, accent, clear)
/// slides from -0.4 w to 1.05 w every 1.3 s, clipped to the capsule.
pub struct IndeterminateLine {
    color: Color32,
    height: f32,
    width: Option<f32>,
}

impl IndeterminateLine {
    pub fn new(color: Color32) -> Self {
        Self {
            color,
            height: size::PROGRESS_HEIGHT,
            width: None,
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, response) = ui.allocate_exact_size(vec2(width, self.height), Sense::hover());
        response.widget_info(|| WidgetInfo::new(Role::ProgressIndicator));
        if ui.is_rect_visible(rect) {
            let time = ui.ctx().clock();
            paint_indeterminate(ui.painter(), rect, time, self.color);
            ui.ctx().request_repaint();
        }
        response
    }
}

/// Paints the indeterminate bar for clock time `time`.
pub fn paint_indeterminate(painter: &Painter, rect: Rect, time: f64, accent: Color32) {
    paint_capsule(painter, rect, color::track());
    let phase = anim::indeterminate_phase(time);
    let w = rect.width();
    let x0 = rect.min.x + w * phase;
    let band = Rect::from_min_max(pos2(x0, rect.min.y), pos2(x0 + w * 0.35, rect.max.y));
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mid = band.center().x;
    let clear = Color32::TRANSPARENT;
    let accent = accent.to_srgba_unmultiplied();
    let accent = Color32::from_rgba_unmultiplied(accent[0], accent[1], accent[2], 255);
    let mut mesh = Mesh::default();
    let vertex = |pos: Pos2, color: Color32| Vertex {
        pos,
        uv: egui::epaint::WHITE_UV,
        color,
    };
    mesh.vertices = vec![
        vertex(band.left_top(), clear),
        vertex(pos2(mid, band.min.y), accent),
        vertex(pos2(mid, band.max.y), accent),
        vertex(band.left_bottom(), clear),
        vertex(band.right_top(), clear),
        vertex(band.right_bottom(), clear),
    ];
    mesh.indices = vec![0, 1, 2, 0, 2, 3, 1, 4, 5, 1, 5, 2];
    clip.add(Shape::mesh(mesh));
}

// ---------------------------------------------------------------------------------------------
// Keycaps, hints, controller glyphs
// ---------------------------------------------------------------------------------------------

/// The controller face buttons, painted as vector shapes (the font glyphs are not relied on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadGlyph {
    Cross,
    Circle,
    Triangle,
    Square,
}

impl PadGlyph {
    /// The text characters the original uses for them.
    pub fn from_char(c: char) -> Option<PadGlyph> {
        match c {
            '✕' => Some(PadGlyph::Cross),
            '○' => Some(PadGlyph::Circle),
            '△' => Some(PadGlyph::Triangle),
            '□' => Some(PadGlyph::Square),
            _ => None,
        }
    }

    /// Paints the glyph centred at `centre`, fitting a `size` x `size` box, strokes of 1.5 dp.
    pub fn paint(self, painter: &Painter, centre: Pos2, size: f32, color: Color32) {
        let stroke = Stroke::new(1.5, color);
        let h = size * 0.5;
        match self {
            PadGlyph::Cross => {
                let d = h * 0.78;
                painter.line_segment([centre + vec2(-d, -d), centre + vec2(d, d)], stroke);
                painter.line_segment([centre + vec2(-d, d), centre + vec2(d, -d)], stroke);
            }
            PadGlyph::Circle => {
                painter.circle_stroke(centre, h * 0.85, stroke);
            }
            PadGlyph::Triangle => {
                let r = h * 0.95;
                let points = vec![
                    centre + vec2(0.0, -r),
                    centre + vec2(r * 0.9, r * 0.7),
                    centre + vec2(-r * 0.9, r * 0.7),
                ];
                painter.add(Shape::closed_line(points, stroke));
            }
            PadGlyph::Square => {
                let d = h * 0.75;
                painter.rect_stroke(
                    Rect::from_center_size(centre, Vec2::splat(d * 2.0)),
                    corner(1.0),
                    stroke,
                    StrokeKind::Middle,
                );
            }
        }
    }
}

/// Paints a key string such as `↩ / ✕` in a role: text runs as text, controller glyphs as shapes.
/// Returns the width used. `origin` is the left-centre of the string.
pub fn paint_key_string(
    painter: &Painter,
    origin: Pos2,
    style: TextStyle,
    keys: &str,
    color: Color32,
) -> f32 {
    let mut x = origin.x;
    let mut run = String::new();
    let flush = |run: &mut String, x: &mut f32| {
        if !run.is_empty() {
            let rect = text::paint(
                painter,
                pos2(*x, origin.y),
                Align2::LEFT_CENTER,
                style,
                run,
                color,
            );
            *x += rect.width();
            run.clear();
        }
    };
    for c in keys.chars() {
        if let Some(glyph) = PadGlyph::from_char(c) {
            flush(&mut run, &mut x);
            let side = style.size * 0.95;
            glyph.paint(painter, pos2(x + side * 0.5, origin.y), side, color);
            x += side;
        } else {
            run.push(c);
        }
    }
    flush(&mut run, &mut x);
    x - origin.x
}

/// Width of a key string (see [`paint_key_string`]).
pub fn key_string_width(painter: &Painter, style: TextStyle, keys: &str) -> f32 {
    let mut width = 0.0;
    let mut run = String::new();
    for c in keys.chars() {
        if PadGlyph::from_char(c).is_some() {
            if !run.is_empty() {
                width += text::width(painter, style, &run);
                run.clear();
            }
            width += style.size * 0.95;
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        width += text::width(painter, style, &run);
    }
    width
}

/// One hint: a keycap and what it does (`← → ↑ ↓` "Move").
pub struct Hint<'a> {
    pub keys: &'a str,
    pub label: &'a str,
}

impl Hint<'_> {
    pub fn show(&self, ui: &mut Ui) -> Response {
        let painter = ui.painter().clone();
        let keys_w = key_string_width(&painter, ty::HINT_KEY, self.keys);
        let cap_w = keys_w + 14.0;
        let cap_h = ty::HINT_KEY.line_height() + 6.0;
        let label = text::layout(
            &painter,
            ty::HINT_LABEL,
            self.label,
            Color32::PLACEHOLDER,
            f32::INFINITY,
        );
        let width = cap_w + size::HINT_GAP + label.size().x;
        let height = cap_h.max(label.size().y);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if ui.is_rect_visible(rect) {
            let cap = Rect::from_min_size(
                pos2(rect.min.x, rect.center().y - cap_h * 0.5),
                vec2(cap_w, cap_h),
            );
            painter.rect_filled(cap, corner(radius::KEYCAP_HINT), color::keycap());
            paint_key_string(
                &painter,
                pos2(cap.min.x + 7.0, cap.center().y),
                ty::HINT_KEY,
                self.keys,
                color::text_hint(),
            );
            painter.galley(
                pos2(
                    cap.max.x + size::HINT_GAP,
                    rect.center().y - label.size().y * 0.5,
                ),
                label,
                color::text_hint(),
            );
        }
        response
    }
}

/// The hint bar: `← → ↑ ↓` Move, `↩ / ✕` Select, `esc / ○` Back and, optionally, `O / △` Options.
/// The key texts are fixed (PlayStation naming), not adapted to the controller in use.
pub fn hint_bar(ui: &mut Ui, language: Language, options: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::HINT_BAR_GAP;
        Hint {
            keys: "← → ↑ ↓",
            label: language.text(Key::HintMove),
        }
        .show(ui);
        Hint {
            keys: "↩ / ✕",
            label: language.text(Key::HintSelect),
        }
        .show(ui);
        Hint {
            keys: "esc / ○",
            label: language.text(Key::HintBack),
        }
        .show(ui);
        if options {
            Hint {
                keys: "O / △",
                label: language.text(Key::HintOptions),
            }
            .show(ui);
        }
    });
}

/// A key cap of the controls panel: 30 x 28 minimum, radius 7, 13.5 semibold, 1 px stroke at 0.18.
/// `keyboard` keys fill at 0.12, mouse and wheel sources at 0.06; `focus` (0..=1, the row's focus amount) inverts to black.
pub fn key_cap(ui: &mut Ui, label: &str, keyboard: bool, focus: f32) -> Response {
    let painter = ui.painter().clone();
    let galley = text::layout(
        &painter,
        ty::KEY_CAP,
        label,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let width = (galley.size().x + 2.0 * size::KEYCAP_PADDING_X).max(size::KEYCAP_MIN.0);
    let height = size::KEYCAP_MIN.1.max(galley.size().y);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let ink = mix(Color32::WHITE, Color32::BLACK, focus);
        let fill = fade(ink, if keyboard { 0.12 } else { 0.06 });
        painter.rect_filled(rect, corner(radius::KEYCAP), fill);
        paint_hairline(&painter, rect, radius::KEYCAP, fade(ink, 0.18));
        painter.galley(rect.center() - galley.size() * 0.5, galley, ink);
    }
    response
}

/// Convenience: fills `rect` with black alpha, used for scrims.
pub fn paint_scrim(painter: &Painter, rect: Rect, alpha: f32) {
    painter.rect_filled(rect, 0.0, black(alpha));
}
