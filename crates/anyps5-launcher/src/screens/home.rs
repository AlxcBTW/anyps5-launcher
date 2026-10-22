//! The home screen (docs/DESIGN.md 5.3): top bar, the tile row with the growing selected tile, the
//! hero (title, action row, facts line), the hint bar, and the empty library.
//!
//! The model decides what is selected and focused; this module paints it and reports presses back.

use anyps5_core::library::Game;
use anyps5_core::paths::GameId;
use egui::{
    Align, Align2, Color32, Context, CursorIcon, Id, Layout, Rect, Role, Sense, Ui, UiBuilder,
    WidgetInfo, pos2, vec2,
};

use crate::app::model::{GameState, Model, SizeState};
use crate::screens::{
    ScreenCx, StateLabel, paint_glass_card, paint_gradient_circle, paint_label, paint_spinner,
    place, state_label, wrapped,
};
use crate::ui::anim::{AnimExt, Motion, lerp};
use crate::ui::chrome::{self, TileImage, TileOverlay, TileStyle};
use crate::ui::components::{
    ConsoleButton, IconButton, IndeterminateLine, ProgressLine, hint_bar, key_string_width,
};
use crate::ui::focus::HomeFocus;
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Key, Language, byte_text};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{self, Weight, color, pad, size, ty, white};

/// Hero content slides in from this far below.
const HERO_SWAP_OFFSET: f32 = 14.0;
/// Gap between the stacked parts of the hero.
const HERO_GAP: f32 = 22.0;
/// Height of the action row (the console button).
const ACTION_ROW: f32 = size::BUTTON_MIN_HEIGHT;
/// Longest a failure or diagnosis text gets.
const MESSAGE_WIDTH: f32 = 520.0;

/// A press the model has to hear about, collected while painting.
#[derive(Clone, Copy, Debug)]
enum Press {
    Tile { index: usize, double: bool },
    Focus(HomeFocus),
    ChooseFolder,
}

/// The hero that is on screen and the ones still fading out.
#[derive(Clone, Default)]
struct HeroTrack {
    current: Option<GameId>,
    leaving: Vec<GameId>,
}

/// Paints the whole home screen and applies the presses.
pub fn show(ui: &mut Ui, cx: &mut ScreenCx<'_>) {
    let screen = cx.ctx.content_rect();
    let mut presses = Vec::new();
    top_bar(ui, cx, screen, &mut presses);
    if cx.model.games.is_empty() {
        empty_library(ui, cx, screen, &mut presses);
    } else {
        tile_row(ui, cx, screen, &mut presses);
        hero(ui, cx, screen, &mut presses);
        hints(ui, cx.language(), screen);
    }
    for press in presses {
        match press {
            Press::Tile { index, double } => cx.model.click_tile(index, double),
            Press::Focus(focus) => cx.model.click(focus),
            Press::ChooseFolder => cx.model.request_folder_dialog(),
        }
    }
}

/// Focus is only drawn while no overlay covers the home screen.
fn focus_visible(model: &Model) -> bool {
    model.overlay.is_none() && model.starting.is_none()
}

// ---------------------------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------------------------

fn top_bar(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let language = cx.language();
    let accent = cx.accent();
    let painter = ui.painter().clone();
    let top = screen.min.y + pad::HOME_TOP;
    let mid = top + size::ICON_BUTTON * 0.5;
    let gap = 22.0;

    let title = text::paint(
        &painter,
        pos2(screen.min.x + pad::HOME_X, mid),
        Align2::LEFT_CENTER,
        ty::TOP_BAR_TITLE,
        language.text(Key::Games),
        Color32::WHITE,
    );
    if cx.model.scanning {
        paint_spinner(
            &painter,
            pos2(title.max.x + gap + 8.0, mid),
            7.0,
            cx.ctx.clock(),
            Color32::WHITE,
        );
    }

    // From the right: clock, profile, settings, library.
    let mut x = screen.max.x - pad::HOME_X;
    let clock = crate::app::clock::local_time().text();
    let clock_width = text::tabular_width(&painter, ty::CLOCK, &clock);
    x -= clock_width;
    text::paint_tabular(
        &painter,
        pos2(x, mid - ty::CLOCK.line_height() * 0.5),
        ty::CLOCK,
        &clock,
        white(0.92),
    );
    x -= gap;

    let name = cx.model.prefs.profile_name.as_str();
    let name_width = text::width(&painter, ty::PROFILE, name);
    let profile_width = size::AVATAR + 10.0 + name_width;
    x -= profile_width;
    let avatar = pos2(x + size::AVATAR * 0.5, mid);
    paint_gradient_circle(
        &painter,
        avatar,
        size::AVATAR * 0.5,
        accent,
        theme::fade(accent, 0.55),
    );
    let initial: String = name.chars().take(1).flat_map(char::to_uppercase).collect();
    text::paint(
        &painter,
        avatar,
        Align2::CENTER_CENTER,
        theme::TextStyle::new(16.0, Weight::Bold),
        &initial,
        Color32::WHITE,
    );
    text::paint(
        &painter,
        pos2(x + size::AVATAR + 10.0, mid),
        Align2::LEFT_CENTER,
        ty::PROFILE,
        name,
        white(0.9),
    );
    x -= gap;

    let visible = focus_visible(cx.model);
    let buttons = [
        (1u8, Icon::GEAR_SIX, language.text(Key::Settings)),
        (0u8, Icon::DOTS_NINE, language.text(Key::Library)),
    ];
    for (index, icon, label) in buttons {
        x -= size::ICON_BUTTON;
        let rect = Rect::from_min_size(pos2(x, top), vec2(size::ICON_BUTTON, size::ICON_BUTTON));
        let focused = visible && cx.model.focus == HomeFocus::Top(index);
        let response = place(ui, rect, |ui| {
            IconButton::new(icon, label).focused(focused).show(ui)
        });
        if response.clicked() {
            presses.push(Press::Focus(HomeFocus::Top(index)));
        }
        x -= gap;
    }
}

// ---------------------------------------------------------------------------------------------
// Tile row
// ---------------------------------------------------------------------------------------------

/// The overlay a tile shows for a state (DESIGN.md 5.1).
pub fn tile_overlay(state: &GameState) -> TileOverlay {
    match state {
        GameState::Locked => TileOverlay::Locked,
        GameState::Unreadable => TileOverlay::Unreadable,
        GameState::Queued => TileOverlay::Queued,
        GameState::Installing { step, fraction } => {
            TileOverlay::Installing(step.overall(*fraction))
        }
        GameState::Failed(_) => TileOverlay::Failed,
        GameState::Installed { .. } => TileOverlay::None,
        GameState::Starting => TileOverlay::Starting,
        GameState::Running => TileOverlay::Running,
    }
}

/// Decoded icon size to ask for: small tiles take the smaller texture.
pub fn icon_resolution(ctx: &Context, side: f32) -> u32 {
    if side * ctx.pixels_per_point() > 256.0 {
        400
    } else {
        256
    }
}

/// The tile style of a game: icon, state overlay, time.
pub fn tile_style(cx: &mut ScreenCx<'_>, game: &Game, side: f32, accent: Color32) -> TileStyle {
    let mut style = TileStyle::new(accent);
    style.time = cx.ctx.clock();
    style.overlay = tile_overlay(&cx.model.state_of(game));
    if let Some(icon) = &game.icon {
        let max = icon_resolution(cx.ctx, side);
        style.image = cx.textures.get(icon, max).map(|l| TileImage {
            texture: l.id,
            aspect: l.aspect,
        });
    }
    style
}

fn tile_row(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let ctx = cx.ctx.clone();
    let painter = ui.painter().clone();
    let accent = cx.accent();
    let language = cx.language();
    let (small, large, gap, label_column) = (
        size::TILE_SMALL,
        size::TILE_LARGE,
        size::TILE_GAP,
        size::TILE_LABEL_COLUMN,
    );
    let leading = screen.min.x + pad::HOME_X;
    let row_top = screen.min.y + pad::HOME_TOP + size::ICON_BUTTON + 34.0;
    let selected = cx.model.selected;
    let ring_on = focus_visible(cx.model) && cx.model.focus == HomeFocus::Tiles;
    let interactive = focus_visible(cx.model);
    let count = cx.model.games.len();
    // Tiles further than a screen away are not worth animating.
    let first = selected.saturating_sub(3);
    let last = (selected + ((screen.width() / (small + gap)).ceil() as usize) + 3).min(count);

    for index in first..last {
        let game = cx.model.games[index].clone();
        let is_selected = index == selected;
        let offset = index as isize - selected as isize;
        let target_x = if offset < 0 {
            leading + offset as f32 * (small + gap)
        } else if offset == 0 {
            leading
        } else {
            leading + large + gap + label_column + (offset - 1) as f32 * (small + gap)
        };
        let key = |what: &'static str| (what, game.id.as_str().to_owned());
        let t = ctx.animate_to(
            key("tile-size"),
            if is_selected { 1.0 } else { 0.0 },
            Motion::Layout,
        );
        let x = ctx.animate_to(key("tile-x"), target_x, Motion::Layout);
        let opacity = ctx.animate_to(
            key("tile-opacity"),
            if offset < 0 { 0.0 } else { 1.0 },
            Motion::Layout,
        );
        let side = lerp(small, large, t);
        let y = row_top + (large - small) * 0.18 * (1.0 - t);
        let rect = Rect::from_min_size(pos2(x, y), vec2(side, side));
        if rect.max.x < screen.min.x || rect.min.x > screen.max.x || opacity <= 0.003 {
            continue;
        }

        let ring_t = ctx.focus_t(key("tile-ring"), is_selected && ring_on);
        let mut style = tile_style(cx, &game, side, accent);
        style.focus = ring_t;
        style.ring = ring_t > 0.002;
        style.opacity = opacity.clamp(0.0, 1.0);
        chrome::paint_tile(&painter, rect, &style);

        if interactive && opacity > 0.5 {
            let state = cx.model.state_of(&game);
            let label = format!(
                "{}, {}",
                game.name,
                state_label(&state, language, accent).text
            );
            let response =
                ui.interact(rect, Id::unique(("tile", game.id.as_str())), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label.as_str()));
            if response.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if response.double_clicked() {
                presses.push(Press::Tile {
                    index,
                    double: true,
                });
            } else if response.clicked() {
                presses.push(Press::Tile {
                    index,
                    double: false,
                });
            }
        }
    }

    // The name and state beside the selected tile fade with the selection.
    for index in first..last {
        let game = cx.model.games[index].clone();
        let is_selected = index == selected;
        let id = ("tile-label", game.id.as_str().to_owned());
        if !is_selected && ctx.animation_value(id.clone()).is_none() {
            continue;
        }
        let alpha = ctx
            .animate_from_to(id, 0.0, if is_selected { 1.0 } else { 0.0 }, Motion::Layout)
            .clamp(0.0, 1.0);
        if alpha <= 0.003 {
            continue;
        }
        let state = cx.model.state_of(&game);
        let label = state_label(&state, language, accent);
        let x = leading + large + gap * 1.4;
        let y = row_top + large * 0.32;
        let width = label_column - gap;
        let name = wrapped(
            &painter,
            ty::TILE_NAME,
            &game.name,
            theme::fade(Color32::WHITE, alpha),
            width,
            2,
        );
        let name_height = name.size().y;
        painter.galley(pos2(x, y), name, Color32::WHITE);
        paint_label(
            &painter,
            pos2(
                x,
                y + name_height + 6.0 + ty::STATE_LABEL.line_height() * 0.5,
            ),
            &label,
            ty::STATE_LABEL,
            width,
            2,
            alpha,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------------------------

fn hints(ui: &mut Ui, language: Language, screen: Rect) {
    let painter = ui.painter().clone();
    let (width, height) = hint_bar_size(&painter, language);
    let rect = Rect::from_min_size(
        pos2(
            screen.max.x - pad::HOME_X - width,
            screen.max.y - pad::HINT_Y - height,
        ),
        vec2(width, height),
    );
    place(ui, rect, |ui| hint_bar(ui, language, true));
}

/// Size of the hint bar (the components lay it out themselves; the screen needs it to right-align).
fn hint_bar_size(painter: &egui::Painter, language: Language) -> (f32, f32) {
    let cap_height = ty::HINT_KEY.line_height() + 6.0;
    let height = cap_height.max(ty::HINT_LABEL.line_height());
    let hints = [
        ("← → ↑ ↓", Key::HintMove),
        ("↩ / ✕", Key::HintSelect),
        ("esc / ○", Key::HintBack),
        ("O / △", Key::HintOptions),
    ];
    let mut width = 0.0;
    for (i, (keys, key)) in hints.iter().enumerate() {
        let cap = key_string_width(painter, ty::HINT_KEY, keys) + 14.0;
        width += cap + size::HINT_GAP + text::width(painter, ty::HINT_LABEL, language.text(*key));
        if i + 1 < hints.len() {
            width += size::HINT_BAR_GAP;
        }
    }
    (width, height)
}

fn hero(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let ctx = cx.ctx.clone();
    let Some(selected) = cx.model.selected_game().map(|g| g.id.clone()) else {
        return;
    };
    let track_id = Id::unique("home-hero-track");
    let mut track: HeroTrack = ctx.data_mut(|d| d.get_temp(track_id)).unwrap_or_default();
    if track.current.as_ref() != Some(&selected) {
        if let Some(previous) = track.current.replace(selected.clone()) {
            track.leaving.retain(|id| *id != previous);
            track.leaving.push(previous);
            let keep = track.leaving.len().saturating_sub(2);
            track.leaving.drain(..keep);
        }
        track.leaving.retain(|id| *id != selected);
    }
    // The ones on their way out are drawn first, so the incoming hero sits on top.
    let mut still_leaving = Vec::new();
    for id in track.leaving.clone() {
        let alpha = ctx.animate_to(("hero", id.as_str().to_owned()), 0.0, Motion::Layout);
        if alpha > 0.01
            && let Some(game) = cx.model.game(&id).cloned()
        {
            paint_hero(ui, cx, screen, &game, alpha, 0.0, false, presses);
            still_leaving.push(id);
        }
    }
    track.leaving = still_leaving;
    let t = ctx.animate_from_to(
        ("hero", selected.as_str().to_owned()),
        0.0,
        1.0,
        Motion::Layout,
    );
    if let Some(game) = cx.model.game(&selected).cloned() {
        paint_hero(
            ui,
            cx,
            screen,
            &game,
            t,
            if ctx.reduced_motion() {
                0.0
            } else {
                (1.0 - t) * HERO_SWAP_OFFSET
            },
            true,
            presses,
        );
    }
    ctx.data_mut(|d| d.insert_temp(track_id, track));
}

/// Where the parts of the hero sit, from the bottom up.
struct HeroGeometry {
    x: f32,
    title_bottom: f32,
    actions_top: f32,
    cards_top: f32,
}

fn hero_geometry(screen: Rect, hint_height: f32, dy: f32) -> HeroGeometry {
    let bottom = screen.max.y - pad::HINT_Y - hint_height - pad::HINT_Y + dy;
    let cards_top = bottom - size::INFO_CARD.1;
    let actions_top = cards_top - HERO_GAP - ACTION_ROW;
    HeroGeometry {
        x: screen.min.x + pad::HERO_X,
        title_bottom: actions_top - HERO_GAP,
        actions_top,
        cards_top,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_hero(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    screen: Rect,
    game: &Game,
    alpha: f32,
    dy: f32,
    live: bool,
    presses: &mut Vec<Press>,
) {
    let language = cx.language();
    let accent = cx.accent();
    let state = cx.model.state_of(game);
    let mut child = ui.new_child(UiBuilder::new().max_rect(screen));
    child.set_opacity(alpha.clamp(0.0, 1.0));
    let ui = &mut child;
    let painter = ui.painter().clone();
    let (_, hint_height) = hint_bar_size(&painter, language);
    let geometry = hero_geometry(screen, hint_height, dy);
    let x = geometry.x;
    let width = screen.width() - 2.0 * pad::HERO_X;
    let visible = live && focus_visible(cx.model);

    // 1. Title: 58 light, two lines at most, shrinks to 60 %.
    let mut title = wrapped(
        &painter,
        ty::HERO_TITLE,
        &game.name,
        Color32::WHITE,
        width,
        2,
    );
    for step in 1..=4 {
        if !title.elided {
            break;
        }
        let style = text::scaled(ty::HERO_TITLE, 1.0 - 0.1 * step as f32);
        title = wrapped(&painter, style, &game.name, Color32::WHITE, width, 2);
    }
    // A low window must not push a two-line title over the tile row.
    let tiles_bottom =
        screen.min.y + pad::HOME_TOP + size::ICON_BUTTON + 34.0 + size::TILE_LARGE + 24.0;
    if geometry.title_bottom - title.size().y < tiles_bottom {
        title = wrapped(
            &painter,
            ty::HERO_TITLE,
            &game.name,
            Color32::WHITE,
            width,
            1,
        );
    }
    let title_height = title.size().y;
    painter.galley(
        pos2(x, geometry.title_bottom - title_height),
        title,
        Color32::WHITE,
    );

    // 2. Action row.
    let row = Rect::from_min_size(pos2(x, geometry.actions_top), vec2(width, ACTION_ROW));
    let has_trophies = cx.model.trophies.get(&game.id);
    let trophy_text =
        has_trophies.map(|set| format!("{}/{}", set.unlocked_count(), set.trophies.len()));
    let settings_index = if trophy_text.is_some() { 2 } else { 1 };
    let (primary_title, primary_icon, primary_fill) = primary_look(&state, language);
    let mut pressed = None;
    let after_buttons = place(ui, row, |ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        let focused = |i: usize| visible && cx.model.focus == HomeFocus::Actions(i);
        let primary = ConsoleButton::new(primary_title)
            .icon(primary_icon)
            .icon_fill(primary_fill)
            .prominent(true)
            .min_width(size::PLAY_MIN_WIDTH)
            .focused(focused(0))
            .show(ui);
        if primary.clicked() {
            pressed = Some(HomeFocus::Actions(0));
        }
        if let Some(text) = &trophy_text {
            let response = ConsoleButton::new(text)
                .accessible_name(format!("{}, {}", language.text(Key::Trophies), text))
                .icon(Icon::TROPHY)
                .icon_fill(true)
                .focused(focused(1))
                .show(ui);
            if response.clicked() {
                pressed = Some(HomeFocus::Actions(1));
            }
        }
        let more = ConsoleButton::icon_only(Icon::DOTS_THREE, language.text(Key::Options))
            .focused(focused(settings_index))
            .show(ui);
        if more.clicked() {
            pressed = Some(HomeFocus::Actions(settings_index));
        }
        ui.cursor().min
    });
    if live && let Some(focus) = pressed {
        presses.push(Press::Focus(focus));
    }
    action_row_status(ui, cx, game, &state, after_buttons.x, geometry.actions_top);

    // 3. Four info cards.
    let status = state_label(&state, language, accent);
    let size_text = match cx.model.sizes.get(&game.id) {
        Some(SizeState::Known(bytes)) => byte_text(*bytes as i64, language),
        _ => byte_text(-1, language),
    };
    let cards = [
        (Icon::CIRCLE_DASHED, Key::Status),
        (Icon::HARD_DRIVE, Key::Size),
        (Icon::HASH, Key::TitleID),
        (Icon::TAG, Key::Version),
    ];
    let values = [
        String::new(),
        size_text,
        game.title_id.clone(),
        game.version.clone(),
    ];
    for (i, ((icon, key), value)) in cards.iter().zip(values.iter()).enumerate() {
        let rect = Rect::from_min_size(
            pos2(
                x + i as f32 * (size::INFO_CARD.0 + size::INFO_CARD_GAP),
                geometry.cards_top,
            ),
            vec2(size::INFO_CARD.0, size::INFO_CARD.1),
        );
        place(ui, rect, |ui| {
            let value = if i == 0 {
                CardValue::State(&status)
            } else {
                CardValue::Text(value)
            };
            fact_card(ui, *icon, language.text(*key), value, language);
        });
    }
}

fn primary_look(state: &GameState, language: Language) -> (&'static str, Icon, bool) {
    match state {
        GameState::Installed { .. } => (language.text(Key::Play), Icon::PLAY, true),
        GameState::Locked | GameState::Unreadable => {
            (language.text(Key::Unlock), Icon::LOCK_OPEN, true)
        }
        GameState::Failed(_) => (language.text(Key::Retry), Icon::ARROW_CLOCKWISE, false),
        GameState::Installing { .. } | GameState::Queued => {
            (language.text(Key::Cancel), Icon::X, false)
        }
        GameState::Starting => (language.text(Key::Starting), Icon::HOURGLASS, false),
        GameState::Running => (language.text(Key::Running), Icon::PLAY_CIRCLE, false),
    }
}

/// To the right of the buttons: the unlock progress, the failure or the diagnosis.
fn action_row_status(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    game: &Game,
    state: &GameState,
    x: f32,
    top: f32,
) {
    let language = cx.language();
    let accent = cx.accent();
    let painter = ui.painter().clone();
    let mid = top + ACTION_ROW * 0.5;
    match state {
        GameState::Installing { step, fraction } => {
            let left = x + 10.0;
            let overall = step.overall(*fraction);
            let block = size::HERO_PROGRESS_WIDTH;
            let line_height = ty::PROGRESS_LABEL.line_height();
            let block_height = line_height + 8.0 + size::PROGRESS_HEIGHT;
            let y = mid - block_height * 0.5;
            text::paint(
                &painter,
                pos2(left, y),
                Align2::LEFT_TOP,
                ty::PROGRESS_LABEL,
                language.text(step.title_key()),
                white(0.85),
            );
            let percent = format!("{}%", (overall * 100.0) as u32);
            let w = text::tabular_width(&painter, ty::PROGRESS_PERCENT, &percent);
            text::paint_tabular(
                &painter,
                pos2(left + block - w, y),
                ty::PROGRESS_PERCENT,
                &percent,
                white(0.85),
            );
            let bar = Rect::from_min_size(
                pos2(left, y + line_height + 8.0),
                vec2(block, size::PROGRESS_HEIGHT),
            );
            place(ui, bar, |ui| {
                if *step == crate::app::pipeline::UnlockStep::Relink && *fraction == 0.0 {
                    IndeterminateLine::new(accent).width(block).show(ui);
                } else {
                    ProgressLine::new(overall, accent)
                        .width(block)
                        .label(language.text(step.title_key()))
                        .show(ui);
                }
            });
        }
        GameState::Failed(message) => {
            let galley = wrapped(
                &painter,
                ty::DETAIL_LARGE,
                message,
                color::error_text(),
                MESSAGE_WIDTH,
                2,
            );
            let pos = pos2(x, mid - galley.size().y * 0.5);
            painter.galley(pos, galley, color::error_text());
        }
        _ => {
            if let Some(summary) = cx.model.diagnoses.get(&game.id) {
                paint_label(
                    &painter,
                    pos2(x, mid),
                    &StateLabel {
                        icon: Icon::WARNING,
                        fill: true,
                        tint: color::warning_text(),
                        text: summary.clone(),
                    },
                    ty::DETAIL_LARGE,
                    MESSAGE_WIDTH,
                    2,
                    1.0,
                );
            }
        }
    }
}

/// What an info card shows under its overline.
enum CardValue<'a> {
    Text(&'a str),
    State(&'a StateLabel),
}

/// One 236 x 96 info card: overline with a glyph over the value, on glass over the artwork.
fn fact_card(ui: &mut Ui, icon: Icon, title: &str, value: CardValue<'_>, language: Language) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(size::INFO_CARD.0, size::INFO_CARD.1), Sense::hover());
    let shown = match &value {
        CardValue::Text(text) => (*text).to_owned(),
        CardValue::State(label) => label.text.clone(),
    };
    let name = format!("{title}: {shown}");
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, name.as_str()));
    let painter = ui.painter();
    paint_glass_card(ui.ctx(), painter, rect, GlassStyle::card());
    let pad = size::INFO_CARD_PADDING;
    let overline_y = rect.min.y + pad;
    painter.text(
        pos2(
            rect.min.x + pad,
            overline_y + ty::INFO_OVERLINE.line_height() * 0.5,
        ),
        Align2::LEFT_CENTER,
        icon.glyph(),
        icon.font(12.0, false),
        color::text_overline(),
    );
    text::paint(
        painter,
        pos2(rect.min.x + pad + 18.0, overline_y),
        Align2::LEFT_TOP,
        ty::INFO_OVERLINE,
        &text::to_upper(title, language),
        color::text_overline(),
    );
    let value_y = overline_y + ty::INFO_OVERLINE.line_height() + 10.0;
    let max = size::INFO_CARD.0 - 2.0 * pad;
    match value {
        CardValue::Text(text) => {
            let truncated = text::middle_truncate(painter, ty::INFO_VALUE, text, max);
            text::paint(
                painter,
                pos2(rect.min.x + pad, value_y),
                Align2::LEFT_TOP,
                ty::INFO_VALUE,
                &truncated,
                Color32::WHITE,
            );
        }
        CardValue::State(label) => {
            paint_label(
                painter,
                pos2(
                    rect.min.x + pad,
                    value_y + ty::STATE_LABEL.line_height() * 0.5,
                ),
                label,
                ty::STATE_LABEL,
                max,
                1,
                1.0,
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Empty library
// ---------------------------------------------------------------------------------------------

fn empty_library(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let language = cx.language();
    let painter = ui.painter().clone();
    let scanning = cx.model.scanning;
    let folder = cx.model.prefs.games_folder.clone();
    let title = language.text(if scanning {
        Key::Scanning
    } else {
        Key::EmptyLibraryTitle
    });
    let body = language.text(Key::EmptyLibraryText);

    let icon_size = 54.0;
    let rows = [
        icon_size,
        ty::EMPTY_TITLE.line_height(),
        ty::ROW_TITLE.line_height().max(17.0 * 1.35),
    ];
    let path_height = folder
        .as_ref()
        .map_or(0.0, |_| ty::DETAIL.line_height() + 18.0);
    let total =
        rows.iter().sum::<f32>() + 18.0 * 2.0 + path_height + 18.0 + 10.0 + size::BUTTON_MIN_HEIGHT;
    let region_top = screen.min.y + pad::HOME_TOP + size::ICON_BUTTON;
    let mut y = region_top + (screen.max.y - region_top - total) * 0.5;
    let cx_mid = screen.center().x;

    let icon = if scanning {
        Icon::MAGNIFYING_GLASS
    } else {
        Icon::STACK
    };
    let centre = pos2(cx_mid, y + icon_size * 0.5);
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(icon_size, false),
        white(0.8),
    );
    if !scanning {
        // "square.stack.3d.up.slash": the stack struck through.
        let d = icon_size * 0.36;
        painter.line_segment(
            [centre + vec2(-d, d), centre + vec2(d, -d)],
            egui::Stroke::new(3.0, white(0.8)),
        );
    }
    y += icon_size + 18.0;
    text::paint(
        &painter,
        pos2(cx_mid, y),
        Align2::CENTER_TOP,
        ty::EMPTY_TITLE,
        title,
        Color32::WHITE,
    );
    y += ty::EMPTY_TITLE.line_height() + 18.0;
    let body_galley = crate::screens::wrapped_centered(
        &painter,
        theme::TextStyle::new(17.0, Weight::Regular),
        body,
        white(0.65),
        (screen.width() - 2.0 * pad::HOME_X).min(720.0),
        3,
    );
    let body_height = body_galley.size().y;
    painter.galley(pos2(cx_mid, y), body_galley, white(0.65));
    y += body_height.max(rows[2]) + 18.0;
    if let Some(folder) = folder {
        let shown = text::middle_truncate(
            &painter,
            ty::DETAIL,
            &folder.to_string_lossy(),
            screen.width() * 0.6,
        );
        text::paint(
            &painter,
            pos2(cx_mid, y),
            Align2::CENTER_TOP,
            ty::DETAIL,
            &shown,
            white(0.6),
        );
        y += ty::DETAIL.line_height() + 18.0;
    }
    y += 10.0;
    let focused = cx.model.overlay.is_none();
    let button_width = 260.0;
    let rect = Rect::from_min_size(
        pos2(cx_mid - button_width * 0.5, y),
        vec2(button_width, size::BUTTON_MIN_HEIGHT),
    );
    let response = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Center)),
            |ui| {
                ConsoleButton::new(language.text(Key::ChangeFolder))
                    .icon(Icon::FOLDER)
                    .focused(focused)
                    .show(ui)
            },
        )
        .inner;
    if response.clicked() {
        presses.push(Press::ChooseFolder);
    }
}
