//! Text layout helpers on top of egui's galleys: type-scale roles, line height, tracking, middle
//! truncation, shrink-to-fit and tabular figures (egui has no OpenType `tnum`, so digits are laid
//! out one by one in cells as wide as the widest digit of the font).

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, FontId, Galley, Painter, Pos2, Rect, Vec2, pos2, vec2};

use crate::ui::theme::TextStyle;

/// Lays out `text` in a role of the type scale. `wrap_width` of `f32::INFINITY` means one line.
pub fn layout(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    color: Color32,
    wrap_width: f32,
) -> Arc<Galley> {
    layout_font(
        painter,
        style.font(),
        style.line_height(),
        style.tracking,
        text,
        color,
        wrap_width,
    )
}

/// Like [`layout`] with an explicit font (for scaled text).
pub fn layout_font(
    painter: &Painter,
    font: FontId,
    line_height: f32,
    tracking: f32,
    text: &str,
    color: Color32,
    wrap_width: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, wrap_width);
    for section in &mut job.sections {
        section.format.line_height = Some(line_height);
        section.format.extra_letter_spacing = tracking;
    }
    painter.layout_job(job)
}

/// Uppercase for overlines. Turkish keeps its dotted and dotless i: `i` becomes `İ` and `ı` stays `I`.
pub fn to_upper(text: &str, language: crate::ui::i18n::Language) -> String {
    if language == crate::ui::i18n::Language::Turkish {
        text.chars()
            .flat_map(|c| match c {
                'i' => vec!['İ'],
                'ı' => vec!['I'],
                other => other.to_uppercase().collect(),
            })
            .collect()
    } else {
        text.to_uppercase()
    }
}

/// Quantises a scaled size so animated scale effects reuse a bounded set of glyph rasters.
pub fn quantize_size(size: f32) -> f32 {
    (size * 4.0).round() / 4.0
}

/// A type-scale role scaled by `factor` (the SwiftUI `scaleEffect` of a focused element).
pub fn scaled(style: TextStyle, factor: f32) -> TextStyle {
    TextStyle {
        size: quantize_size(style.size * factor),
        ..style
    }
}

/// Paints one line of text anchored at `anchor` of the rectangle spanned around `pos`
/// (`Align2::LEFT_TOP` puts `pos` at the top-left corner of the text). Returns the text rect.
pub fn paint(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    style: TextStyle,
    text: &str,
    color: Color32,
) -> Rect {
    let galley = layout(painter, style, text, color, f32::INFINITY);
    let rect = anchor.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// The width a line of text takes.
pub fn width(painter: &Painter, style: TextStyle, text: &str) -> f32 {
    layout(painter, style, text, Color32::WHITE, f32::INFINITY)
        .size()
        .x
}

/// Shortens `text` in the middle with an ellipsis until it fits `max_width` (paths).
pub fn middle_truncate(painter: &Painter, style: TextStyle, text: &str, max_width: f32) -> String {
    if width(painter, style, text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    // Largest number of kept characters (split evenly around the ellipsis) that still fits.
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(painter, style, &ellipsize_middle(&chars, mid)) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    ellipsize_middle(&chars, lo)
}

/// `keep` characters of `chars`, half from each end, joined by an ellipsis.
pub fn ellipsize_middle(chars: &[char], keep: usize) -> String {
    if keep >= chars.len() {
        return chars.iter().collect();
    }
    let head = keep.div_ceil(2);
    let tail = keep / 2;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// Factor (<= 1) by which `style`'s size must shrink so the text fits `max_width` on one line,
/// but never below `minimum` (SwiftUI `minimumScaleFactor`).
pub fn shrink_factor(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    max_width: f32,
    minimum: f32,
) -> f32 {
    let natural = width(painter, style, text);
    if natural <= max_width || natural <= 0.0 {
        1.0
    } else {
        (max_width / natural).clamp(minimum, 1.0)
    }
}

// ---------------------------------------------------------------------------------------------
// Tabular figures
// ---------------------------------------------------------------------------------------------

/// Width of the widest digit of `style`'s font: the cell every digit is laid out in.
pub fn digit_cell(painter: &Painter, style: TextStyle) -> f32 {
    ('0'..='9')
        .map(|c| width(painter, style, &c.to_string()))
        .fold(0.0, f32::max)
}

/// Total width of `text` with tabular digits.
pub fn tabular_width(painter: &Painter, style: TextStyle, text: &str) -> f32 {
    let cell = digit_cell(painter, style);
    text.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                cell
            } else {
                width(painter, style, &c.to_string())
            }
        })
        .sum()
}

/// Paints `text` with tabular digits starting at `pos` (top-left). Each digit is centred in its
/// cell so columns of numbers line up and a changing value does not shift its neighbours.
/// Returns the rect used.
pub fn paint_tabular(
    painter: &Painter,
    pos: Pos2,
    style: TextStyle,
    text: &str,
    color: Color32,
) -> Rect {
    let cell = digit_cell(painter, style);
    let mut x = pos.x;
    let mut height: f32 = 0.0;
    for c in text.chars() {
        let galley = layout(painter, style, &c.to_string(), color, f32::INFINITY);
        let w = galley.size().x;
        let advance = if c.is_ascii_digit() { cell } else { w };
        let offset = if c.is_ascii_digit() {
            (cell - w) * 0.5
        } else {
            0.0
        };
        painter.galley(pos2(x + offset, pos.y), galley.clone(), color);
        height = height.max(galley.size().y);
        x += advance;
    }
    Rect::from_min_size(pos, vec2(x - pos.x, height))
}

/// Paints tabular text aligned inside `rect` (horizontal `align`, vertically centred).
pub fn paint_tabular_aligned(
    painter: &Painter,
    rect: Rect,
    align: Align,
    style: TextStyle,
    text: &str,
    color: Color32,
) {
    let w = tabular_width(painter, style, text);
    let h = style.line_height();
    let x = match align {
        Align::Min => rect.min.x,
        Align::Center => rect.center().x - w * 0.5,
        Align::Max => rect.max.x - w,
    };
    paint_tabular(
        painter,
        pos2(x, rect.center().y - h * 0.5),
        style,
        text,
        color,
    );
}

/// A [`TextFormat`] for a role, for building multi-section jobs.
pub fn format(style: TextStyle, color: Color32) -> TextFormat {
    TextFormat {
        font_id: style.font(),
        color,
        line_height: Some(style.line_height()),
        extra_letter_spacing: style.tracking,
        ..TextFormat::default()
    }
}

/// Size of `text` in a role without painting.
pub fn measure(painter: &Painter, style: TextStyle, text: &str, wrap_width: f32) -> Vec2 {
    layout(painter, style, text, Color32::WHITE, wrap_width).size()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_ellipsis_keeps_both_ends() {
        let chars: Vec<char> = "C:/Users/alec/Games".chars().collect();
        assert_eq!(ellipsize_middle(&chars, 8), "C:/U…ames");
        assert_eq!(ellipsize_middle(&chars, 100), "C:/Users/alec/Games");
        assert_eq!(ellipsize_middle(&chars, 0), "…");
    }

    #[test]
    fn turkish_uppercase_keeps_the_dotted_i() {
        use crate::ui::i18n::Language;
        assert_eq!(to_upper("kimliği", Language::Turkish), "KİMLİĞİ");
        assert_eq!(to_upper("ışık", Language::Turkish), "IŞIK");
        assert_eq!(to_upper("title id", Language::English), "TITLE ID");
    }

    #[test]
    fn quantised_sizes_form_a_small_set() {
        let mut sizes: Vec<f32> = (0..=100)
            .map(|i| quantize_size(19.0 * (1.0 + 0.05 * i as f32 / 100.0)))
            .collect();
        sizes.dedup();
        assert!(sizes.len() <= 5, "{sizes:?}");
    }
}
