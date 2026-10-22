//! The trophy list (docs/DESIGN.md 5.11): the set's icon, title and progress on top, one row per
//! trophy below with its icon, grade and earned date. Secret trophies stay locked until earned.
//! Up and down move, back and options close; clicking a row focuses it.
//!
//! The data is the core's [`TrophySet`]; trophy icons are PNG bytes inside it, decoded here into
//! textures (greyscale and dimmed until a trophy is earned).

use std::collections::HashMap;

use anyps5_core::trophies::{Trophy, TrophySet};
use egui::{
    Align2, Color32, ColorImage, Context, Rect, Sense, TextureHandle, TextureId, TextureOptions,
    Ui, UiBuilder, pos2, vec2,
};

use crate::screens::clamped;
use crate::screens::convert;
use crate::screens::format;
use crate::screens::scroll::Follow;
use crate::screens::side::commands;
use crate::ui::Shell;
use crate::ui::anim::AnimExt;
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::{corner, paint_focus_stroke, paint_progress};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, Grade, color, fade, mix, radius, size, ty, white};

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrophyAction {
    /// The list closed (back, options).
    Close,
}

/// The set to show, borrowed for one frame.
pub struct TrophyData<'a> {
    /// Identifies the game in the icon cache (the game id).
    pub game_key: &'a str,
    pub game_name: &'a str,
    pub set: &'a TrophySet,
    pub language: Language,
    pub accent: Color32,
}

/// Decodes and keeps trophy icon textures.
#[derive(Default)]
pub struct IconCache {
    textures: HashMap<(String, bool), Option<TextureHandle>>,
}

impl IconCache {
    /// The texture of an icon, decoded on first use. Unearned icons are greyscale.
    pub fn get(
        &mut self,
        ctx: &Context,
        key: &str,
        data: Option<&[u8]>,
        earned: bool,
    ) -> Option<TextureId> {
        self.textures
            .entry((key.to_owned(), earned))
            .or_insert_with(|| {
                let image = decode_icon(data?, earned)?;
                Some(ctx.load_texture(
                    format!("trophy/{key}/{earned}"),
                    image,
                    TextureOptions::LINEAR,
                ))
            })
            .as_ref()
            .map(TextureHandle::id)
    }
}

/// PNG bytes to an image, greyscale when `earned` is false.
pub fn decode_icon(data: &[u8], earned: bool) -> Option<ColorImage> {
    let decoded = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = (decoded.width() as usize, decoded.height() as usize);
    let mut pixels = decoded.into_raw();
    if !earned {
        for px in pixels.as_chunks_mut::<4>().0 {
            let luma =
                (0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2]))
                    .round() as u8;
            px[0] = luma;
            px[1] = luma;
            px[2] = luma;
        }
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &pixels))
}

/// Paints a trophy icon tile: the picture (or a trophy glyph) clipped to a rounded square.
pub fn paint_trophy_icon(ui: &mut Ui, rect: Rect, texture: Option<TextureId>, earned: bool) {
    let side = rect.width();
    let r = radius::trophy_icon(side);
    ui.painter().rect_filled(rect, corner(r), white(0.07));
    match texture {
        Some(id) => {
            let tint = if earned {
                Color32::WHITE
            } else {
                fade(Color32::WHITE, 0.45)
            };
            egui::Image::new(egui::load::SizedTexture::new(id, rect.size()))
                .corner_radius(corner(r))
                .tint(tint)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::TROPHY.glyph(),
                Icon::TROPHY.font(side * 0.38, true),
                white(0.4),
            );
        }
    }
}

/// The accessible name of a trophy row: name, grade and whether it is earned.
fn trophy_label(trophy: &Trophy, language: Language) -> String {
    let secret = trophy.hidden && !trophy.is_unlocked();
    let name = if secret {
        language.text(Key::TrophyHidden)
    } else {
        trophy.name.as_str()
    };
    let grade = convert::grade(trophy.grade).title(language);
    let state = match trophy.unlocked {
        Some(micros) => format::local_date_text(micros, language),
        None => language.text(Key::TrophyLocked).to_owned(),
    };
    format!("{name}, {grade}, {state}")
}

/// The trophy list overlay.
#[derive(Default)]
pub struct TrophyListPanel {
    open: bool,
    closing: bool,
    index: usize,
    icons: IconCache,
}

impl TrophyListPanel {
    /// Opens on the first trophy (the original keeps the index of the last visit; `open_at` does too).
    pub fn open(&mut self) {
        self.open_at(self.index);
    }

    pub fn open_at(&mut self, index: usize) {
        self.open = true;
        self.closing = false;
        self.index = index;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &TrophyData<'_>,
    ) -> Vec<TrophyAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        let count = data.set.trophies.len();
        self.index = self.index.min(count.saturating_sub(1));
        if self.open {
            for command in commands(events) {
                match command {
                    Command::Up | Command::Down => {
                        let next = if command == Command::Up {
                            self.index.saturating_sub(1)
                        } else {
                            (self.index + 1).min(count.saturating_sub(1))
                        };
                        if next != self.index {
                            self.index = next;
                            shell.play(Cue::Move);
                        }
                    }
                    Command::Back | Command::Options => {
                        shell.play(Cue::Back);
                        self.close();
                        actions.push(TrophyAction::Close);
                    }
                    Command::Left | Command::Right | Command::Select => {}
                }
            }
        }
        let open = self.open;
        let mut clicked = None;
        let drawn = fullscreen_panel(
            ctx,
            "trophy-list",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| self.draw(ui, data, &mut clicked),
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if let Some(i) = clicked.filter(|i| open && *i != self.index) {
            self.index = i;
            shell.play(Cue::Move);
        }
        actions
    }

    fn draw(&mut self, ui: &mut Ui, data: &TrophyData<'_>, clicked: &mut Option<usize>) {
        let area = ui
            .max_rect()
            .shrink2(vec2(theme::pad::TROPHIES.0, theme::pad::TROPHIES.1));
        let header = Rect::from_min_size(area.min, vec2(area.width(), size::TROPHY_HEADER_ICON));
        self.draw_header(ui, header, data);
        let list = Rect::from_min_max(pos2(area.min.x, header.max.y + 28.0), area.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        self.draw_rows(&mut child, data, clicked);
    }

    fn draw_header(&mut self, ui: &mut Ui, header: Rect, data: &TrophyData<'_>) {
        let ctx = ui.ctx().clone();
        let language = data.language;
        let set = data.set;
        let painter = ui.painter().clone();
        let icon_rect = Rect::from_min_size(header.min, vec2(header.height(), header.height()));
        let texture = self.icons.get(
            &ctx,
            &format!("{}/set", data.game_key),
            set.icon.as_deref(),
            true,
        );
        paint_trophy_icon(ui, icon_rect, texture, true);

        // Overline, title and the progress row, centred against the icon.
        let overline = ty::TROPHIES_OVERLINE;
        let title_style = ty::TROPHY_LIST_TITLE;
        let row_h = ty::PERCENT.line_height();
        let total = overline.line_height() + 12.0 + title_style.line_height() + 12.0 + row_h;
        let x = icon_rect.max.x + 26.0;
        let top = header.center().y - total * 0.5;
        text::paint(
            &painter,
            pos2(x, top),
            Align2::LEFT_TOP,
            overline,
            &text::to_upper(language.text(Key::Trophies), language),
            color::text_overline(),
        );
        let title = if set.title.is_empty() {
            data.game_name
        } else {
            set.title.as_str()
        };
        let hint_w = 520.0;
        let title_galley = clamped(
            &painter,
            title_style,
            title,
            Color32::PLACEHOLDER,
            (header.max.x - x - hint_w).max(200.0),
            1,
        );
        let title_y = top + overline.line_height() + 12.0;
        painter.galley(pos2(x, title_y), title_galley, Color32::WHITE);

        let row_y = title_y + title_style.line_height() + 12.0;
        let percent = set.percent();
        let mut cx = x;
        let percent_text = format!("{percent}%");
        let w = text::tabular_width(&painter, ty::PERCENT, &percent_text);
        text::paint_tabular(
            &painter,
            pos2(cx, row_y),
            ty::PERCENT,
            &percent_text,
            Color32::WHITE,
        );
        cx += w + 16.0;
        let bar = Rect::from_center_size(pos2(cx + 130.0, row_y + row_h * 0.5), vec2(260.0, 4.0));
        paint_progress(&painter, bar, percent as f32 / 100.0, data.accent);
        cx = bar.max.x + 16.0;
        let count_style = theme::TextStyle::new(15.0, theme::Weight::SemiBold);
        for grade in anyps5_core::trophies::Grade::ALL {
            let (unlocked, total) = set.count(grade);
            if total == 0 {
                continue;
            }
            let ui_grade = convert::grade(grade);
            let mid = row_y + row_h * 0.5;
            painter.text(
                pos2(cx + 8.0, mid),
                Align2::CENTER_CENTER,
                Icon::TROPHY.glyph(),
                Icon::TROPHY.font(15.0, true),
                ui_grade.color(),
            );
            cx += 22.0;
            let label = format!("{unlocked}/{total}");
            let w = text::tabular_width(&painter, count_style, &label);
            text::paint_tabular(
                &painter,
                pos2(cx, mid - count_style.line_height() * 0.5),
                count_style,
                &label,
                fade(Color32::WHITE, 0.85),
            );
            cx += w + 16.0;
        }

        // The hint bar, right aligned and centred on the header.
        crate::screens::right_aligned_hints(ui, header, language);
    }

    fn draw_rows(&mut self, ui: &mut Ui, data: &TrophyData<'_>, clicked: &mut Option<usize>) {
        let ctx = ui.ctx().clone();
        let id = ui.make_persistent_id("trophy-rows");
        let (follow, scroll_to) = Follow::begin(&ctx, id, Some(self.index));
        let mut area = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
            );
        if let Some(offset) = scroll_to {
            area = area.vertical_scroll_offset(offset);
        }
        let mut geometry = Vec::with_capacity(data.set.trophies.len());
        let output = area.show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let origin = ui.cursor().min.y;
                    for (i, trophy) in data.set.trophies.iter().enumerate() {
                        let rect = self.draw_row(ui, data, trophy, i, i == self.index);
                        geometry.push((rect.min.y - origin, rect.height()));
                        // Hit-test after drawing so the click reads the row's own response.
                        let response =
                            ui.interact(rect, id.with(("trophy-row", i)), Sense::click());
                        let label = trophy_label(trophy, data.language);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, label.as_str())
                        });
                        if response.clicked() {
                            *clicked = Some(i);
                        }
                    }
                });
        });
        follow.end(
            &ctx,
            geometry,
            output.content_size.y,
            output.inner_rect.height(),
            output.state.clamped_offset().y,
        );
    }

    fn draw_row(
        &mut self,
        ui: &mut Ui,
        data: &TrophyData<'_>,
        trophy: &Trophy,
        index: usize,
        focused: bool,
    ) -> Rect {
        let ctx = ui.ctx().clone();
        let language = data.language;
        let earned = trophy.is_unlocked();
        let secret = trophy.hidden && !earned;
        let width = ui.available_width();
        let painter = ui.painter().clone();
        let grade: Grade = convert::grade(trophy.grade);

        // Right column first: it decides how much room the text has.
        let grade_style = theme::TextStyle::new(14.0, theme::Weight::SemiBold);
        let grade_text = grade.title(language);
        let date_text = match trophy.unlocked {
            Some(micros) => format::local_date_text(micros, language),
            None => language.text(Key::TrophyLocked).to_owned(),
        };
        let date_style = ty::DETAIL;
        let right_w = (text::width(&painter, grade_style, grade_text) + 22.0)
            .max(text::width(&painter, date_style, &date_text));

        let icon_size = size::TROPHY_ICON;
        let text_x = 18.0 + icon_size + 20.0;
        let text_w = (width - text_x - 18.0 - right_w - 12.0).max(60.0);
        let name = if secret {
            language.text(Key::TrophyHidden)
        } else {
            trophy.name.as_str()
        };
        let detail = if secret {
            language.text(Key::TrophyHiddenDetail)
        } else {
            trophy.detail.as_str()
        };
        let name_g = clamped(
            &painter,
            ty::TROPHY_NAME,
            name,
            Color32::PLACEHOLDER,
            text_w,
            1,
        );
        let detail_g = clamped(
            &painter,
            ty::DETAIL_LARGE,
            detail,
            Color32::PLACEHOLDER,
            text_w,
            2,
        );
        let progress = (!earned && !secret).then_some(trophy.progress).flatten();
        let mut block = name_g.size().y + 6.0 + detail_g.size().y;
        if progress.is_some() {
            block += 6.0 + 14.4;
        }
        let height = icon_size.max(block) + 28.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return rect;
        }
        let t = ctx.focus_t(("trophy-row-focus", index), focused);
        let r = radius::TROPHY_ROW;
        // Ring only: a glow would show through the translucent row.
        painter.rect_filled(
            rect,
            corner(r),
            mix(
                color::surface_trophy_row(),
                color::surface_trophy_row_focused(),
                t,
            ),
        );
        paint_focus_stroke(&painter, rect, r, 2.0, t);

        // Icon or the secret's lock tile.
        let icon_rect = Rect::from_center_size(
            pos2(rect.min.x + 18.0 + icon_size * 0.5, rect.center().y),
            vec2(icon_size, icon_size),
        );
        if secret {
            painter.rect_filled(icon_rect, corner(12.0), white(0.07));
            painter.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                Icon::LOCK.glyph(),
                Icon::LOCK.font(22.0, true),
                white(0.5),
            );
        } else {
            let key = format!("{}/{}", data.game_key, trophy.id());
            let texture = self.icons.get(&ctx, &key, trophy.icon.as_deref(), earned);
            paint_trophy_icon(ui, icon_rect, texture, earned);
        }

        // Name, detail and the progress line.
        let x = rect.min.x + text_x;
        let top = rect.center().y - block * 0.5;
        let name_color = if earned {
            Color32::WHITE
        } else {
            fade(Color32::WHITE, 0.8)
        };
        let name_h = name_g.size().y;
        painter.galley(pos2(x, top), name_g, name_color);
        let detail_y = top + name_h + 6.0;
        let detail_h = detail_g.size().y;
        painter.galley(pos2(x, detail_y), detail_g, white(0.6));
        if let Some((value, target)) = progress {
            let y = detail_y + detail_h + 6.0;
            let bar = Rect::from_center_size(pos2(x + 90.0, y + 7.2), vec2(180.0, 4.0));
            let fraction = if target == 0 {
                0.0
            } else {
                value as f32 / target as f32
            };
            paint_progress(&painter, bar, fraction.clamp(0.0, 1.0), data.accent);
            text::paint_tabular(
                &painter,
                pos2(bar.max.x + 10.0, y),
                theme::TextStyle::new(12.0, theme::Weight::Medium),
                &format!("{value}/{target}"),
                white(0.6),
            );
        }

        // Grade and date, right aligned.
        let right = rect.max.x - 18.0;
        let grade_color = if earned {
            grade.color()
        } else {
            fade(grade.color(), 0.55)
        };
        let rows_h = grade_style.line_height() + 6.0 + date_style.line_height();
        let gy = rect.center().y - rows_h * 0.5;
        let label = text::paint(
            &painter,
            pos2(right, gy),
            Align2::RIGHT_TOP,
            grade_style,
            grade_text,
            grade_color,
        );
        painter.text(
            pos2(label.min.x - 14.0, gy + grade_style.line_height() * 0.5),
            Align2::CENTER_CENTER,
            Icon::TROPHY.glyph(),
            Icon::TROPHY.font(14.0, true),
            grade_color,
        );
        text::paint(
            &painter,
            pos2(right, gy + grade_style.line_height() + 6.0),
            Align2::RIGHT_TOP,
            date_style,
            &date_text,
            white(if earned { 0.7 } else { 0.4 }),
        );
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba));
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn earned_icons_keep_their_colour_and_unearned_go_grey() {
        let data = png([200, 40, 40, 255]);
        let colour = decode_icon(&data, true).unwrap();
        assert_eq!(colour.pixels[0], Color32::from_rgb(200, 40, 40));
        let grey = decode_icon(&data, false).unwrap();
        let px = grey.pixels[0];
        assert_eq!(px.r(), px.g());
        assert_eq!(px.g(), px.b());
        assert!(px.r() > 40 && px.r() < 200);
    }

    #[test]
    fn a_broken_icon_is_none_not_a_panic() {
        assert!(decode_icon(b"not a png", true).is_none());
        assert!(decode_icon(&[], false).is_none());
    }

    #[test]
    fn the_index_stays_inside_the_set() {
        let mut panel = TrophyListPanel::default();
        panel.open_at(40);
        assert!(panel.is_active());
        panel.close();
        assert!(panel.is_active(), "still fading out");
    }
}
