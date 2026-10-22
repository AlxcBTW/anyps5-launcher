//! The app settings (docs/DESIGN.md 5.7): categories on the left, their rows on the right. Values
//! live in the core's [`Preferences`]; the panel edits them in place, applies theme, accent,
//! language, motion and sound to the shell right away and reports `changed` so the host saves.
//!
//! Categories: Appearance, Library, Language, Behaviour, Toolkit (Windows addition: update check
//! and CPU conversion), Advanced, About. File and folder choices that only need a native dialog run
//! here; everything that needs the host (rescan, quit, the toolkit download) is an action.

use std::path::PathBuf;

use anyps5_core::settings::{Backdrop, CpuConversion, Preferences};
use egui::{Align2, Color32, Context, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::screens::convert;
use crate::screens::dialogs;
use crate::screens::format::{abbreviate_path, home_dir};
use crate::screens::rows::{self, Change, ListOptions, Row};
use crate::screens::side::{Output, commands};
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion, lerp};
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::corner;
use crate::ui::focus::PanelNav;
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{Accent, Theme, color, mix, pad, radius, size, ty, white};

/// The categories, in the order of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Library,
    Language,
    Behaviour,
    Toolkit,
    Advanced,
    About,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Appearance,
        Category::Library,
        Category::Language,
        Category::Behaviour,
        Category::Toolkit,
        Category::Advanced,
        Category::About,
    ];

    pub fn title(self, language: Language) -> &'static str {
        match self {
            Category::Appearance => language.text(Key::Appearance),
            Category::Library => language.text(Key::Storage),
            Category::Language => language.text(Key::Language),
            Category::Behaviour => language.text(Key::Behaviour),
            Category::Toolkit => Extra::Toolkit.text(language),
            Category::Advanced => language.text(Key::Advanced),
            Category::About => language.text(Key::About),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Category::Appearance => Icon::PAINT_BRUSH,
            Category::Library => Icon::SQUARES_FOUR,
            Category::Language => Icon::GLOBE,
            Category::Behaviour => Icon::SLIDERS_HORIZONTAL,
            Category::Toolkit => Icon::HARD_DRIVE,
            Category::Advanced => Icon::WRENCH,
            Category::About => Icon::INFO,
        }
    }
}

/// Where the toolkit update check stands.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum UpdateState {
    /// Never checked (or the check is off).
    #[default]
    Unknown,
    Checking,
    /// The toolkit download runs.
    Downloading,
    UpToDate,
    /// A newer release exists: its version.
    Available(String),
    Failed,
}

/// What the settings show that is not a preference.
pub struct SettingsEnv<'a> {
    pub games_found: usize,
    pub app_version: &'a str,
    /// The installed toolkit's version, `None` when there is none.
    pub toolkit_version: Option<&'a str>,
    pub update: UpdateState,
    /// The folder screenshots go to when the preference is unset.
    pub default_screenshots: &'a std::path::Path,
}

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Close,
    /// A new games folder was chosen (already stored in the preferences): rescan and watch it.
    GamesFolderChosen(PathBuf),
    Rescan,
    ShowScreenshots,
    /// "Check now" (or the check was switched on).
    CheckForUpdates,
    /// Download and install the latest toolkit (first install or update).
    InstallToolkit,
    /// A toolkit folder was chosen: adopt it.
    AdoptToolkitFolder(PathBuf),
    Quit,
}

/// One category with its rows.
pub struct CategoryRows {
    pub category: Category,
    pub rows: Vec<Row>,
}

const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "webp", "gif"];

fn backdrop_options(prefs: &Preferences, language: Language) -> Vec<String> {
    let t = |key: Key| language.text(key).to_owned();
    let mut options = vec![t(Key::BackgroundTheme), t(Key::BackgroundImage)];
    // A video background cannot be shown yet; a stored choice stays visible instead of vanishing.
    if prefs.backdrop == Backdrop::Video {
        options.push(t(Key::BackgroundVideo));
    }
    options
}

fn path_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "—".to_owned())
}

/// All categories and rows for these preferences.
pub fn build_categories(
    prefs: &Preferences,
    env: &SettingsEnv<'_>,
    language: Language,
) -> Vec<CategoryRows> {
    let t = |key: Key| language.text(key).to_owned();
    let home = home_dir();
    let abbreviated = |p: &std::path::Path| abbreviate_path(p, home.as_deref());

    // Appearance
    let accent = convert::effective_accent(prefs);
    let mut appearance = vec![
        Row::choice(
            "theme",
            t(Key::Theme),
            Theme::ALL
                .iter()
                .map(|x| x.title(language).to_owned())
                .collect(),
            Theme::ALL
                .iter()
                .position(|x| *x == convert::theme(prefs.theme))
                .unwrap_or(0),
        )
        .icon(Icon::PALETTE),
        Row::choice(
            "accent",
            t(Key::Accent),
            Accent::ALL
                .iter()
                .map(|x| x.title(language).to_owned())
                .collect(),
            Accent::ALL.iter().position(|x| *x == accent).unwrap_or(0),
        )
        .icon(Icon::CIRCLE_HALF),
        Row::choice(
            "backdrop",
            t(Key::Background),
            backdrop_options(prefs, language),
            match prefs.backdrop {
                Backdrop::Theme => 0,
                Backdrop::Image => 1,
                Backdrop::Video => 2,
            },
        )
        .icon(Icon::IMAGES),
    ];
    if prefs.backdrop != Backdrop::Theme {
        let mut row = Row::action("file", t(Key::ChooseFile)).icon(Icon::FOLDER);
        if let Some(name) = prefs
            .backdrop_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
        {
            row = row.detail(name);
        }
        appearance.push(row);
    }
    appearance
        .push(Row::toggle("artwork", t(Key::GameArtwork), prefs.game_artwork).icon(Icon::IMAGE));
    appearance.push(
        Row::choice(
            "motion",
            t(Key::Motion),
            vec![t(Key::MotionFull), t(Key::MotionReduced)],
            usize::from(prefs.reduced_motion),
        )
        .icon(Icon::WIND),
    );

    // Library
    let library = vec![
        Row::action("folder", t(Key::Folder))
            .detail(path_text(&prefs.games_folder))
            .icon(Icon::FOLDER),
        Row::action("rescan", t(Key::Rescan))
            .detail(language.format(Key::GamesFound, &[Arg::Int(env.games_found as i64)]))
            .icon(Icon::ARROW_CLOCKWISE),
        Row::info("clones", t(Key::InstalledSize), None),
    ];

    // Language
    let languages = vec![
        Row::choice(
            "language",
            t(Key::Language),
            Language::ALL
                .iter()
                .map(|l| l.display_name().to_owned())
                .collect(),
            Language::ALL
                .iter()
                .position(|l| *l == convert::language(prefs.language))
                .unwrap_or(0),
        )
        .icon(Icon::GLOBE),
    ];

    // Behaviour
    let behaviour = vec![
        Row::text("name", t(Key::ProfileName), prefs.profile_name.clone()).icon(Icon::USER_CIRCLE),
        Row::toggle("sounds", t(Key::Sounds), prefs.sounds).icon(Icon::SPEAKER_HIGH),
        Row::toggle("music", Extra::HomeMusic.text(language), prefs.music)
            .detail(Extra::HomeMusicDetail.text(language))
            .icon(Icon::MUSIC_NOTES),
        Row::toggle(
            "discord",
            Extra::DiscordPresence.text(language),
            prefs.discord_presence,
        )
        .detail(Extra::DiscordPresenceDetail.text(language))
        .icon(Icon::DISCORD_LOGO),
        Row::toggle("hide", t(Key::HideWhilePlaying), prefs.hide_while_playing)
            .icon(Icon::EYE_SLASH),
    ];

    // Toolkit
    let toolkit_value = env.toolkit_version.map_or_else(
        || Extra::ToolkitMissing.text(language).to_owned(),
        str::to_owned,
    );
    let update_value = match &env.update {
        UpdateState::Unknown => None,
        UpdateState::Checking => Some(Extra::Checking.text(language).to_owned()),
        UpdateState::Downloading => Some(Extra::ToolkitDownloading.text(language).to_owned()),
        UpdateState::UpToDate => Some(Extra::UpToDate.text(language).to_owned()),
        UpdateState::Available(v) => Some(Extra::UpdateAvailable.format(language, &[Arg::Str(v)])),
        UpdateState::Failed => Some(Extra::CheckFailed.text(language).to_owned()),
    };
    let last_checked = prefs
        .toolkit_checked_at
        .clone()
        .unwrap_or_else(|| Extra::NeverChecked.text(language).to_owned());
    let mut toolkit = vec![
        Row::info("toolkit", t(Key::ToolkitVersion), Some(toolkit_value)),
        Row::info(
            "lastChecked",
            Extra::LastChecked.text(language),
            Some(last_checked),
        ),
    ];
    if let Some(value) = update_value {
        toolkit.push(Row::info("updateState", t(Key::Status), Some(value)));
    }
    toolkit.push(
        Row::toggle(
            "updateCheck",
            Extra::UpdateCheck.text(language),
            prefs.check_for_updates,
        )
        .detail(Extra::UpdateCheckDetail.text(language))
        .icon(Icon::ARROWS_CLOCKWISE),
    );
    toolkit
        .push(Row::action("checkNow", Extra::CheckNow.text(language)).icon(Icon::ARROW_CLOCKWISE));
    match (&env.toolkit_version, &env.update) {
        (None, _) => toolkit.push(
            Row::action("install", Extra::ToolkitInstall.text(language))
                .icon(Icon::DOWNLOAD_SIMPLE),
        ),
        (Some(_), UpdateState::Available(_)) => toolkit.push(
            Row::action("install", Extra::ToolkitInstallUpdate.text(language))
                .icon(Icon::DOWNLOAD_SIMPLE),
        ),
        _ => {}
    }
    toolkit.push(
        Row::action("toolkitFolder", Extra::ToolkitChooseFolder.text(language)).icon(Icon::FOLDER),
    );
    toolkit.push(
        Row::choice(
            "cpu",
            Extra::CpuConversion.text(language),
            vec![
                Extra::CpuAuto.text(language).to_owned(),
                Extra::CpuNone.text(language).to_owned(),
                Extra::CpuIntel.text(language).to_owned(),
            ],
            match prefs.cpu_conversion {
                CpuConversion::Auto => 0,
                CpuConversion::None => 1,
                CpuConversion::Intel => 2,
            },
        )
        .detail(Extra::CpuConversionDetail.text(language))
        .icon(Icon::GAUGE),
    );

    // Advanced
    let screenshots = prefs
        .screenshots_folder
        .as_deref()
        .unwrap_or(env.default_screenshots);
    let mut advanced = vec![
        Row::action("screenshots", t(Key::ScreenshotsFolder))
            .detail(abbreviated(screenshots))
            .icon(Icon::CAMERA),
        Row::action("showScreenshots", t(Key::ShowScreenshots)).icon(Icon::IMAGES),
        Row::action("fonts", t(Key::FontsFolder))
            .detail(
                prefs
                    .fonts_folder
                    .as_deref()
                    .map_or_else(|| t(Key::FontsNone), abbreviated),
            )
            .icon(Icon::TEXT_AA),
    ];
    if prefs.fonts_folder.is_some() {
        advanced.push(Row::action("fontsReset", t(Key::FontsReset)).icon(Icon::X_CIRCLE));
    }
    advanced.push(Row::info("fontsHint", t(Key::FontsHint), None));

    // About
    let about = vec![
        Row::info("text", t(Key::AboutText), None),
        // The toolkit version lives on the Toolkit page only.
        Row::info("version", t(Key::Version), Some(env.app_version.to_owned())),
        Row::action("quit", t(Key::Quit))
            .icon(Icon::POWER)
            .destructive(),
    ];

    [
        appearance, library, languages, behaviour, toolkit, advanced, about,
    ]
    .into_iter()
    .zip(Category::ALL)
    .map(|(rows, category)| CategoryRows { category, rows })
    .collect()
}

/// Applies the preferences that the shell shows: theme and accent, language, motion, sounds. The
/// host calls it once after loading the preferences; the panel calls it after every change.
pub fn apply_preferences(shell: &mut Shell, ctx: &Context, prefs: &Preferences) {
    shell.set_theme(
        convert::theme(prefs.theme),
        prefs.accent.map(convert::accent),
    );
    shell.language = convert::language(prefs.language);
    shell.set_reduced_motion(ctx, prefs.reduced_motion);
    shell.sounds.enabled = prefs.sounds;
}

/// Writes one change into the preferences. Dialogs (a picture, a folder) are the caller's
/// business: this covers the plain values. Returns whether anything changed.
pub fn apply_value(prefs: &mut Preferences, change: &Change) -> bool {
    let before = prefs.clone();
    match change {
        Change::Choice {
            row: "theme",
            index,
        } => {
            if let Some(theme) = Theme::ALL.get(*index) {
                prefs.theme = convert::theme_to_core(*theme);
            }
        }
        Change::Choice {
            row: "accent",
            index,
        } => {
            if let Some(accent) = Accent::ALL.get(*index) {
                prefs.accent = Some(convert::accent_to_core(*accent));
            }
        }
        Change::Toggle { row: "artwork", on } => prefs.game_artwork = *on,
        Change::Choice {
            row: "motion",
            index,
        } => prefs.reduced_motion = *index == 1,
        Change::Choice {
            row: "language",
            index,
        } => {
            if let Some(language) = Language::ALL.get(*index) {
                prefs.language = convert::language_to_core(*language);
            }
        }
        Change::Toggle { row: "sounds", on } => prefs.sounds = *on,
        Change::Toggle { row: "music", on } => prefs.music = *on,
        Change::Toggle { row: "discord", on } => prefs.discord_presence = *on,
        Change::Toggle { row: "hide", on } => prefs.hide_while_playing = *on,
        Change::Toggle {
            row: "updateCheck",
            on,
        } => prefs.check_for_updates = *on,
        Change::Choice { row: "cpu", index } => {
            prefs.cpu_conversion = match index {
                1 => CpuConversion::None,
                2 => CpuConversion::Intel,
                _ => CpuConversion::Auto,
            };
        }
        Change::Choice {
            row: "backdrop",
            index: 0,
        } => prefs.backdrop = Backdrop::Theme,
        Change::Run { row: "fontsReset" } => prefs.fonts_folder = None,
        _ => {}
    }
    *prefs != before
}

/// The settings screen.
pub struct SettingsPanel {
    open: bool,
    closing: bool,
    pub nav: PanelNav,
    /// Counts category changes: each one fades the new list in.
    switches: u32,
    last_category: usize,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            nav: PanelNav::settings(Category::ALL.len()),
            switches: 0,
            last_category: 0,
        }
    }
}

impl SettingsPanel {
    /// Opens at the category level on the first category.
    pub fn open(&mut self) {
        self.open_at(0, false);
    }

    /// Opens on a category, optionally already in its rows (previews).
    pub fn open_at(&mut self, category: usize, in_rows: bool) {
        self.open = true;
        self.closing = false;
        self.nav = PanelNav::settings(Category::ALL.len());
        self.nav.category = category.min(Category::ALL.len() - 1);
        self.nav.in_rows = in_rows;
        if in_rows {
            self.nav.row = 0;
        }
        self.last_category = self.nav.category;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        prefs: &mut Preferences,
        env: &SettingsEnv<'_>,
    ) -> Output<SettingsAction> {
        let mut out = Output::default();
        if !self.is_active() {
            return out;
        }
        if self.open {
            for command in commands(events) {
                let cats = build_categories(prefs, env, convert::language(prefs.language));
                let rows = &cats[self.nav.category.min(cats.len() - 1)].rows;
                let driven = rows::drive(shell, &mut self.nav, rows, &[command]);
                for change in &driven.changes {
                    self.apply(shell, ctx, prefs, change, &mut out);
                }
                if driven.close {
                    self.close();
                    out.actions.push(SettingsAction::Close);
                    break;
                }
            }
        }
        if self.nav.category != self.last_category {
            self.last_category = self.nav.category;
            self.switches += 1;
        }
        let language = convert::language(prefs.language);
        let mut cats = build_categories(prefs, env, language);
        let open = self.open;
        let mut mouse = None;
        let mut picked_category = None;
        let nav = self.nav;
        let switches = self.switches;
        let accent = shell.accent_color();
        let drawn = fullscreen_panel(
            ctx,
            "settings-panel",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| {
                draw(
                    ui,
                    &mut cats,
                    &nav,
                    switches,
                    language,
                    accent,
                    &mut mouse,
                    &mut picked_category,
                )
            },
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if open {
            if let Some(index) = picked_category {
                self.nav.category = index;
                self.nav.row = 0;
                self.nav.in_rows = false;
                shell.play(Cue::Move);
            }
            if let Some(events) = mouse {
                let rows = &cats[self.nav.category.min(cats.len() - 1)].rows;
                let (changes, texts) = rows::apply_mouse(shell, &mut self.nav, rows, events);
                for change in &changes {
                    self.apply(shell, ctx, prefs, change, &mut out);
                }
                for (id, value) in texts {
                    if id == "name" && prefs.profile_name != value {
                        prefs.profile_name = value;
                        out.changed = true;
                    }
                }
            }
        }
        out
    }

    fn apply(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        prefs: &mut Preferences,
        change: &Change,
        out: &mut Output<SettingsAction>,
    ) {
        let language = convert::language(prefs.language);
        let mut changed = apply_value(prefs, change);
        match change {
            Change::Choice {
                row: "backdrop",
                index,
            } if *index >= 1 => {
                let kind = if *index == 2 {
                    Backdrop::Video
                } else {
                    Backdrop::Image
                };
                // Choosing a picture asks for one unless it is already the choice with a file.
                if prefs.backdrop_file.is_none() || prefs.backdrop != kind {
                    changed |= choose_backdrop(shell, prefs, language);
                }
            }
            Change::Run { row: "file" } => changed |= choose_backdrop(shell, prefs, language),
            Change::Run { row: "folder" } => {
                let start = prefs.games_folder.clone();
                if let Some(folder) =
                    dialogs::pick_folder(shell, language.text(Key::PickFolder), start.as_deref())
                {
                    prefs.games_folder = Some(folder.clone());
                    changed = true;
                    out.actions.push(SettingsAction::GamesFolderChosen(folder));
                }
            }
            Change::Edit { .. } => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel-text-focus"), true));
                ctx.request_repaint();
            }
            Change::Run { row: "rescan" } => out.actions.push(SettingsAction::Rescan),
            Change::Run { row: "screenshots" } => {
                let start = prefs.screenshots_folder.clone();
                if let Some(folder) = dialogs::pick_folder(
                    shell,
                    language.text(Key::ScreenshotsFolder),
                    start.as_deref(),
                ) {
                    prefs.screenshots_folder = Some(folder);
                    changed = true;
                }
            }
            Change::Run {
                row: "showScreenshots",
            } => out.actions.push(SettingsAction::ShowScreenshots),
            Change::Run { row: "fonts" } => {
                let start = prefs.fonts_folder.clone();
                if let Some(folder) =
                    dialogs::pick_folder(shell, language.text(Key::FontsFolder), start.as_deref())
                {
                    prefs.fonts_folder = Some(folder);
                    changed = true;
                }
            }
            Change::Run { row: "checkNow" } => out.actions.push(SettingsAction::CheckForUpdates),
            Change::Toggle {
                row: "updateCheck",
                on: true,
            } => {
                out.actions.push(SettingsAction::CheckForUpdates);
            }
            Change::Run { row: "install" } => out.actions.push(SettingsAction::InstallToolkit),
            Change::Run {
                row: "toolkitFolder",
            } => {
                if let Some(folder) =
                    dialogs::pick_folder(shell, Extra::ToolkitChooseFolder.text(language), None)
                {
                    out.actions.push(SettingsAction::AdoptToolkitFolder(folder));
                }
            }
            Change::Run { row: "quit" } => out.actions.push(SettingsAction::Quit),
            _ => {}
        }
        if changed {
            out.changed = true;
            apply_preferences(shell, ctx, prefs);
        }
    }
}

/// Asks for a picture and stores it as the backdrop. Returns whether the preferences changed.
fn choose_backdrop(shell: &mut Shell, prefs: &mut Preferences, language: Language) -> bool {
    let start = prefs
        .backdrop_file
        .as_deref()
        .and_then(|p| p.parent())
        .map(|p| p.to_owned());
    match dialogs::pick_file(
        shell,
        language.text(Key::ChooseFile),
        ("Images", &IMAGE_EXTENSIONS),
        start.as_deref(),
    ) {
        Some(file) => {
            prefs.backdrop_file = Some(file);
            prefs.backdrop = Backdrop::Image;
            true
        }
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut Ui,
    cats: &mut [CategoryRows],
    nav: &PanelNav,
    switches: u32,
    language: Language,
    accent: Color32,
    mouse: &mut Option<Vec<rows::ListEvent>>,
    picked: &mut Option<usize>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let area = Rect::from_min_max(
        pos2(
            ui.max_rect().min.x + pad::SETTINGS.0,
            ui.max_rect().min.y + pad::SETTINGS.1,
        ),
        pos2(
            ui.max_rect().max.x - pad::SETTINGS.0,
            ui.max_rect().max.y - pad::SETTINGS.2,
        ),
    );

    // Title and the category list.
    let title = text::paint(
        &painter,
        area.min,
        Align2::LEFT_TOP,
        ty::SCREEN_TITLE,
        language.text(Key::Settings),
        Color32::WHITE,
    );
    let mut y = title.max.y + 31.0;
    let column_w = size::SETTINGS_CATEGORY_WIDTH;
    for (index, cat) in cats.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(area.min.x, y),
            vec2(column_w, size::SETTINGS_CATEGORY_ROW),
        );
        y += size::SETTINGS_CATEGORY_ROW + 6.0;
        let selected = index == nav.category;
        let focused = selected && !nav.in_rows;
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("category", index)),
            Sense::click(),
        );
        let title_text = cat.category.title(language);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Tab, true, selected, title_text)
        });
        if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            *picked = Some(index);
        }
        let t_focus = ctx.focus_t(("category-focus", index), focused);
        let t_sel = ctx.focus_t(("category-selected", index), selected);
        let rest = if response.hovered() && !selected {
            white(0.06)
        } else {
            Color32::TRANSPARENT
        };
        let fill = mix(
            mix(rest, color::selected_category(), t_sel),
            Color32::WHITE,
            t_focus,
        );
        painter.rect_filled(rect, corner(radius::ROW), fill);
        let ink = mix(white(lerp(0.7, 1.0, t_sel)), Color32::BLACK, t_focus);
        painter.text(
            pos2(rect.min.x + 18.0 + 13.0, rect.center().y),
            Align2::CENTER_CENTER,
            cat.category.icon().glyph(),
            cat.category.icon().font(18.0, false),
            ink,
        );
        let style = if selected {
            ty::CATEGORY_SELECTED
        } else {
            ty::CATEGORY
        };
        text::paint(
            &painter,
            pos2(rect.min.x + 18.0 + 26.0 + 14.0, rect.center().y),
            Align2::LEFT_CENTER,
            style,
            title_text,
            ink,
        );
    }

    // The rows of the current category, fading in from the side when the category changes.
    let current = nav.category.min(cats.len() - 1);
    let fade_in = ctx.animate_from_to(("settings-switch", switches), 0.0, 1.0, Motion::Focus);
    let x = area.min.x + column_w + 36.0;
    let list_rect = Rect::from_min_max(pos2(x + 18.0 * (1.0 - fade_in), area.min.y), area.max);
    let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
    child.set_opacity(fade_in.clamp(0.0, 1.0));
    let options = ListOptions {
        accent,
        language,
        focus: nav.in_rows.then_some(nav.row),
        force: None,
    };
    let events = rows::list(
        &mut child,
        ("settings-rows", current),
        &mut cats[current].rows,
        &options,
    );
    *mouse = Some(events);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(toolkit: Option<&'a str>, update: UpdateState) -> SettingsEnv<'a> {
        SettingsEnv {
            games_found: 9,
            app_version: "0.1.0",
            toolkit_version: toolkit,
            update,
            default_screenshots: std::path::Path::new("C:\\Users\\me\\Pictures\\AnyPS5"),
        }
    }

    fn ids(cats: &[CategoryRows], category: Category) -> Vec<&'static str> {
        cats.iter()
            .find(|c| c.category == category)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id)
            .collect()
    }

    #[test]
    fn the_categories_come_in_the_order_of_the_list() {
        let prefs = Preferences::first_run();
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let order: Vec<_> = cats.iter().map(|c| c.category).collect();
        assert_eq!(order, Category::ALL);
    }

    #[test]
    fn appearance_rows_follow_the_backdrop_choice() {
        let mut prefs = Preferences::first_run();
        let e = env(None, UpdateState::Unknown);
        let cats = build_categories(&prefs, &e, Language::English);
        assert_eq!(
            ids(&cats, Category::Appearance),
            ["theme", "accent", "backdrop", "artwork", "motion"]
        );
        prefs.backdrop = Backdrop::Image;
        let cats = build_categories(&prefs, &e, Language::English);
        assert_eq!(
            ids(&cats, Category::Appearance),
            ["theme", "accent", "backdrop", "file", "artwork", "motion"],
            "the file row appears with a picture backdrop"
        );
    }

    #[test]
    fn a_stored_video_choice_stays_visible() {
        let mut prefs = Preferences::first_run();
        let rows_of = |prefs: &Preferences| {
            build_categories(prefs, &env(None, UpdateState::Unknown), Language::English)
                .remove(0)
                .rows
                .into_iter()
                .find(|r| r.id == "backdrop")
                .unwrap()
        };
        let rows::Control::Choice { options, .. } = rows_of(&prefs).control else {
            panic!()
        };
        assert_eq!(options.len(), 2, "no video choice without one stored");
        prefs.backdrop = Backdrop::Video;
        let rows::Control::Choice { options, selected } = rows_of(&prefs).control else {
            panic!()
        };
        assert_eq!((options.len(), selected), (3, 2));
    }

    #[test]
    fn the_toolkit_rows_depend_on_what_is_installed() {
        let prefs = Preferences::first_run();
        let none = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        assert!(
            ids(&none, Category::Toolkit).contains(&"install"),
            "nothing installed: offer it"
        );
        let current = build_categories(
            &prefs,
            &env(Some("0.1.3-abc"), UpdateState::UpToDate),
            Language::English,
        );
        assert!(!ids(&current, Category::Toolkit).contains(&"install"));
        let outdated = build_categories(
            &prefs,
            &env(Some("0.1.3-abc"), UpdateState::Available("0.1.4".into())),
            Language::English,
        );
        assert!(
            ids(&outdated, Category::Toolkit).contains(&"install"),
            "an update is offered"
        );
    }

    #[test]
    fn advanced_offers_the_reset_only_with_a_fonts_folder() {
        let mut prefs = Preferences::first_run();
        let e = env(None, UpdateState::Unknown);
        let without = build_categories(&prefs, &e, Language::English);
        assert!(!ids(&without, Category::Advanced).contains(&"fontsReset"));
        prefs.fonts_folder = Some("D:\\Fonts".into());
        let with = build_categories(&prefs, &e, Language::English);
        assert!(ids(&with, Category::Advanced).contains(&"fontsReset"));
    }

    #[test]
    fn about_ends_with_the_destructive_quit() {
        let prefs = Preferences::first_run();
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let about = &cats.last().unwrap().rows;
        let quit = about.last().unwrap();
        assert_eq!(quit.id, "quit");
        assert!(quit.destructive);
    }

    #[test]
    fn values_land_in_the_preferences() {
        let mut prefs = Preferences::first_run();
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "theme",
                index: 2
            }
        ));
        assert_eq!(prefs.theme, anyps5_core::settings::Theme::Sunset);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "accent",
                index: 3
            }
        ));
        assert_eq!(prefs.accent, Some(anyps5_core::settings::Accent::Violet));
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "sounds",
                on: false
            }
        ));
        assert!(!prefs.sounds);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "music",
                on: false
            }
        ));
        assert!(!prefs.music);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "discord",
                on: false
            }
        ));
        assert!(!prefs.discord_presence);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "motion",
                index: 1
            }
        ));
        assert!(prefs.reduced_motion);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "language",
                index: 0
            }
        ));
        assert_eq!(prefs.language, anyps5_core::settings::Language::Turkish);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "cpu",
                index: 2
            }
        ));
        assert_eq!(prefs.cpu_conversion, CpuConversion::Intel);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "updateCheck",
                on: false
            }
        ));
        assert!(!prefs.check_for_updates);
        assert!(!apply_value(&mut prefs, &Change::Run { row: "rescan" }));
    }

    #[test]
    fn every_row_has_text_in_both_languages() {
        let prefs = Preferences::first_run();
        for language in Language::ALL {
            let cats = build_categories(
                &prefs,
                &env(Some("0.1.3"), UpdateState::Available("0.1.4".into())),
                language,
            );
            for cat in &cats {
                assert!(!cat.category.title(language).is_empty());
                for row in &cat.rows {
                    assert!(!row.title.is_empty(), "{:?} {}", cat.category, row.id);
                }
            }
        }
    }
}
