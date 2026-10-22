//! The host of the panels: opens every panel where the original does, hands each its data, and
//! turns what the panel reports back into model changes, pipeline work and saved settings.
//!
//! | Overlay | Panel (module `screens`) | Opened from |
//! |---|---|---|
//! | `Settings` | `settings::SettingsPanel` | options button, `Ctrl+,` |
//! | `Library` | `library::LibraryGrid` | library button |
//! | `GameSettings` | `game_settings::GameSettingsPanel` | the "..." button of the hero |
//! | `ConfirmUninstall` | `game_settings::UninstallPanel` | "Uninstall" row of the game settings |
//! | `Controls`, `Advanced` | `controls::ControlsPanel`, `advanced::AdvancedPanel` | rows of the game settings |
//! | `Trophies` | `trophies::TrophyListPanel` | trophy button, row of the game settings |
//! | `Log` | `log::LogView` | "Open log" row of the game settings |
//!
//! The panels animate themselves; the host only keeps them in step with `Model::overlay`: a panel
//! opens when its overlay becomes the open one and closes (slides out) when another one takes over.
//! The four side panels share one frame, so changing between them swaps the content without a
//! slide. Commands the model routes here are queued and handed to the panel in `ui`; panels that
//! edit settings get a copy, and the host stores it back and marks the preferences for saving when
//! the panel reports `changed`. Work with I/O (reports, uninstall, toolkit) runs on worker threads
//! and answers through channels polled in [`OverlayHost::frame`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::diagnosis::{self, Diagnosis};
use anyps5_core::library::Game;
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::report::{self, ReportInput, SystemInfo};
use anyps5_core::settings::GameSettings;
use anyps5_core::unlock;
use egui::{Context, Ui, ViewportCommand};

use crate::app::clock;
use crate::app::model::{Effect, GameState, Model};
use crate::app::overlay::{Overlay, OverlayCx, OverlayHost};
use crate::app::toolkit::{ToolkitEvent, ToolkitService};
use crate::screens::advanced::{AdvancedAction, AdvancedData, AdvancedPanel};
use crate::screens::banner::{BannerHost, BannerTrophy};
use crate::screens::controls::{ControlsAction, ControlsData, ControlsPanel};
use crate::screens::game_settings::{
    GameSettingsAction, GameSettingsData, GameSettingsPanel, TrophyProgress, UninstallAction,
    UninstallPanel,
};
use crate::screens::home::tile_overlay;
use crate::screens::library::{LibraryAction, LibraryData, LibraryEntry, LibraryGrid, StateText};
use crate::screens::log::{LogAction, LogData, LogView};
use crate::screens::rows::RowId;
use crate::screens::settings::{SettingsAction, SettingsEnv, SettingsPanel};
use crate::screens::strings::Extra;
use crate::screens::trophies::{TrophyAction, TrophyData, TrophyListPanel};
use crate::screens::{ToastRequest, state_label};
use crate::ui::chrome::TileImage;
use crate::ui::focus::HomeFocus;
use crate::ui::i18n::{Arg, Key};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};

/// The log view keeps this much of the end of a game's log file (DESIGN.md 5.12).
const LOG_TAIL_BYTES: u64 = 60_000;
/// Size of the icons in the library grid.
const LIBRARY_ICON: u32 = 256;

/// What a worker thread of the host reports.
enum Work {
    Report(Result<PathBuf, String>),
    Uninstalled {
        game: GameId,
        result: Result<(), String>,
    },
}

pub struct PanelHost {
    settings: SettingsPanel,
    game_settings: GameSettingsPanel,
    uninstall: UninstallPanel,
    controls: ControlsPanel,
    advanced: AdvancedPanel,
    trophies: TrophyListPanel,
    library: LibraryGrid,
    log: LogView,
    banners: BannerHost,
    toolkit: ToolkitService,
    /// Commands and raw keys routed here by the model, in order.
    events: Vec<InputEvent>,
    /// The game settings row to open on when coming back from the controls or advanced page.
    return_row: Option<RowId>,
    log_text: String,
    log_diagnosis: Option<Diagnosis>,
    /// Resolutions of a title's patch by title id (reading the patch file once per title).
    resolutions: HashMap<String, Vec<String>>,
    work_tx: Sender<Work>,
    work_rx: Receiver<Work>,
    ctx: Context,
    checked_at_start: bool,
    /// The first-start download of the toolkit was tried (once per session; the settings retry).
    toolkit_auto_tried: bool,
}

impl PanelHost {
    /// `simulated`: the demo and the tests; the toolkit service then never goes online.
    pub fn new(ctx: &Context, simulated: bool) -> Self {
        let (work_tx, work_rx) = channel();
        Self {
            settings: SettingsPanel::default(),
            game_settings: GameSettingsPanel::default(),
            uninstall: UninstallPanel::default(),
            controls: ControlsPanel::default(),
            advanced: AdvancedPanel::default(),
            trophies: TrophyListPanel::default(),
            library: LibraryGrid::default(),
            log: LogView::default(),
            banners: BannerHost::default(),
            toolkit: ToolkitService::new(ctx, simulated),
            events: Vec::new(),
            return_row: None,
            log_text: String::new(),
            log_diagnosis: None,
            resolutions: HashMap::new(),
            work_tx,
            work_rx,
            ctx: ctx.clone(),
            checked_at_start: simulated,
            toolkit_auto_tried: simulated,
        }
    }

    /// The events of this frame for the overlay that is open (and none for the others).
    fn take_events(&mut self, wanted: bool) -> Vec<InputEvent> {
        if wanted {
            std::mem::take(&mut self.events)
        } else {
            Vec::new()
        }
    }

    fn resolutions_for(&mut self, game: &Game, patches: Option<&std::path::Path>) -> Vec<String> {
        self.resolutions
            .entry(game.title_id.clone())
            .or_insert_with(|| {
                patches
                    .map(|dir| anyps5_core::patches::resolution_choices(&game.title_id, dir))
                    .unwrap_or_default()
            })
            .clone()
    }

    // --- settings --------------------------------------------------------------------------------

    fn settings_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.settings.is_open() {
            self.settings.open();
        } else if !wanted && self.settings.is_open() {
            self.settings.close();
        }
        let events = self.take_events(wanted);
        let toolkit_version = cx.model.toolkit.clone();
        let default_screenshots = cx.model.paths.default_screenshots();
        let env = SettingsEnv {
            games_found: cx.model.games.len(),
            app_version: env!("CARGO_PKG_VERSION"),
            toolkit_version: toolkit_version.as_deref(),
            update: self.toolkit.state.clone(),
            default_screenshots: &default_screenshots,
        };
        // Theme, accent, language, motion and sounds reach the shell inside the panel
        // (`apply_preferences`), so the backdrop cross-fades while the choice is made.
        let out = self
            .settings
            .show(cx.shell, cx.ctx, &events, &mut cx.model.prefs, &env);
        if out.changed {
            cx.model.mark_prefs_dirty();
        }
        for action in out.actions {
            self.settings_action(cx, action);
        }
    }

    fn settings_action(&mut self, cx: &mut OverlayCx<'_>, action: SettingsAction) {
        let model = &mut *cx.model;
        match action {
            SettingsAction::Close => model.dismiss_overlay(),
            SettingsAction::GamesFolderChosen(_) => {
                model.selected = 0;
                model.rescan();
            }
            SettingsAction::Rescan => model.rescan(),
            SettingsAction::ShowScreenshots => {
                let folder = model.prefs.screenshots_dir(&model.paths);
                let _ = std::fs::create_dir_all(&folder);
                open_folder(&folder);
            }
            SettingsAction::CheckForUpdates => self.check_toolkit(model),
            SettingsAction::InstallToolkit => {
                // The row of the settings shows the download; the toast only confirms the start.
                if self
                    .toolkit
                    .install(&model.paths, &model.prefs.toolkit_repository)
                {
                    let text = Extra::ToolkitDownloading.text(model.language());
                    model.toast(text, Icon::DOWNLOAD_SIMPLE);
                }
            }
            SettingsAction::AdoptToolkitFolder(folder) => self.toolkit.adopt(folder),
            SettingsAction::Quit => cx.ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    fn check_toolkit(&mut self, model: &Model) {
        self.toolkit
            .check(&model.paths, &model.prefs.toolkit_repository);
    }

    // --- game settings and its pages -----------------------------------------------------------

    fn game_settings_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let Some(game) = cx.model.selected_game().cloned() else {
            self.game_settings.dismiss();
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let id = game.id.clone();
        let mut settings = cx.model.prefs.game_settings(&id);
        let resolutions = self.resolutions_for(&game, cx.model.patches_dir.as_deref());
        let trophies = cx.model.trophies.get(&id).map(TrophyProgress::from);
        let installed = matches!(cx.model.state_of(&game), GameState::Installed { .. });
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let mut data = GameSettingsData {
            title: &game.name,
            settings: &mut settings,
            resolutions: &resolutions,
            trophies,
            installed,
            language,
            accent,
        };
        if wanted && !self.game_settings.is_open() {
            match self.return_row.take() {
                Some(row) => self.game_settings.open_on(&data, row),
                None => self.game_settings.open(),
            }
        } else if !wanted && self.game_settings.is_open() {
            if side_next {
                self.game_settings.dismiss();
            } else {
                self.game_settings.close();
            }
        }
        let out = self
            .game_settings
            .show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &id, settings);
        }
        for action in out.actions {
            self.game_settings_action(cx, &game, action);
        }
    }

    fn game_settings_action(
        &mut self,
        cx: &mut OverlayCx<'_>,
        game: &Game,
        action: GameSettingsAction,
    ) {
        let model = &mut *cx.model;
        match action {
            GameSettingsAction::Close => model.dismiss_overlay(),
            GameSettingsAction::OpenControls => model.switch_overlay(Overlay::Controls),
            GameSettingsAction::OpenAdvanced => model.switch_overlay(Overlay::Advanced),
            GameSettingsAction::OpenTrophies => model.switch_overlay(Overlay::Trophies),
            GameSettingsAction::AskUninstall => model.switch_overlay(Overlay::ConfirmUninstall),
            GameSettingsAction::Reinstall => {
                model.dismiss_overlay();
                model.unlock_again(&game.id);
            }
            GameSettingsAction::ShowInExplorer => {
                let target = model
                    .prefs
                    .games_folder
                    .as_deref()
                    .map(|root| {
                        anyps5_core::paths::unlocked_copy_dir(root, &game.title_id, &game.id)
                    })
                    .filter(|copy| copy.exists())
                    .unwrap_or_else(|| game.folder.clone());
                if let Err(error) = report::reveal_in_explorer(&target) {
                    tracing::warn!("cannot show {} in Explorer: {error}", target.display());
                }
            }
            GameSettingsAction::OpenLog => {
                self.prepare_log(model, game);
                model.switch_overlay(Overlay::Log);
            }
            GameSettingsAction::MakeReport => self.make_report(model, game),
            GameSettingsAction::ClearShaderCache => {
                let cache = model.paths.game_cache_dir(&game.title_id);
                let _ = anyps5_core::fsx::remove_tree(&cache);
                let text = model.language().text(Key::CacheCleared);
                model.toast(text, Icon::SPARKLE);
            }
        }
    }

    fn uninstall_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.uninstall.is_active() {
            self.uninstall.open();
        } else if !wanted && self.uninstall.is_active() {
            if cx.model.overlay.is_some_and(Overlay::is_side_panel) {
                self.uninstall.dismiss();
            } else {
                self.uninstall.close();
            }
        }
        let events = self.take_events(wanted);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        for action in self
            .uninstall
            .show(cx.shell, cx.ctx, &events, language, accent)
        {
            let model = &mut *cx.model;
            match action {
                // The original closes everything after either answer.
                UninstallAction::Cancel => model.dismiss_overlay(),
                UninstallAction::Confirm => {
                    model.dismiss_overlay();
                    if let (Some(game), Some(root)) = (
                        model.selected_game().cloned(),
                        model.prefs.games_folder.clone(),
                    ) {
                        self.start_uninstall(&game, root);
                    }
                }
            }
        }
    }

    fn controls_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        if !wanted {
            if self.controls.is_active() {
                if side_next {
                    self.controls.dismiss(cx.shell);
                } else {
                    self.controls.close(cx.shell);
                }
            }
        } else if !self.controls.is_active() {
            self.controls.open();
        }
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let mut settings = cx.model.prefs.game_settings(&game.id);
        let mut data = ControlsData {
            title: &game.name,
            settings: &mut settings,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
        };
        let out = self.controls.show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &game.id, settings);
        }
        for action in out.actions {
            match action {
                ControlsAction::Back => self.back_to_game_settings(cx.model, "controls"),
                ControlsAction::Close => cx.model.dismiss_overlay(),
            }
        }
    }

    fn advanced_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        if !wanted {
            if self.advanced.is_active() {
                if side_next {
                    self.advanced.dismiss();
                } else {
                    self.advanced.close();
                }
            }
        } else if !self.advanced.is_active() {
            self.advanced.open();
        }
        let Some(game) = cx.model.selected_game().cloned() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            return;
        };
        let mut settings = cx.model.prefs.game_settings(&game.id);
        let resolutions = self.resolutions_for(&game, cx.model.patches_dir.as_deref());
        let mut data = AdvancedData {
            game: &game,
            settings: &mut settings,
            resolutions: &resolutions,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
        };
        let out = self.advanced.show(cx.shell, cx.ctx, &events, &mut data);
        if out.changed {
            store_game_settings(cx.model, &game.id, settings);
        }
        for action in out.actions {
            match action {
                AdvancedAction::Back => self.back_to_game_settings(cx.model, "advanced"),
                AdvancedAction::Close => cx.model.dismiss_overlay(),
                AdvancedAction::Toast(ToastRequest { icon, text }) => cx.model.toast(text, icon),
            }
        }
    }

    /// Controls and advanced go back to the game settings on the row that opened them.
    fn back_to_game_settings(&mut self, model: &mut Model, row: RowId) {
        self.return_row = Some(row);
        model.switch_overlay(Overlay::GameSettings);
    }

    // --- full-screen panels --------------------------------------------------------------------

    fn trophies_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let game = cx.model.selected_game().cloned();
        let set = game.as_ref().and_then(|g| cx.model.trophies.get(&g.id));
        let (Some(game), Some(set)) = (game, set) else {
            // Nothing to list: never open, and leave a panel that is already open.
            if wanted {
                cx.model.dismiss_overlay();
            }
            if self.trophies.is_active() {
                self.trophies.close();
            }
            return;
        };
        if wanted && !self.trophies.is_active() {
            self.trophies.open();
        } else if !wanted && self.trophies.is_active() {
            self.trophies.close();
        }
        let data = TrophyData {
            game_key: game.id.as_str(),
            game_name: &game.name,
            set,
            language: cx.model.language(),
            accent: cx.shell.accent_color(),
        };
        let actions = self.trophies.show(cx.shell, cx.ctx, &events, &data);
        for TrophyAction::Close in actions {
            cx.model.dismiss_overlay();
        }
    }

    fn library_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.library.is_active() {
            self.library.open(cx.model.selected);
        } else if !wanted && self.library.is_active() {
            self.library.close();
        }
        let events = self.take_events(wanted);
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let entries: Vec<LibraryEntry> = cx
            .model
            .games
            .iter()
            .map(|game| {
                let state = cx.model.state_of(game);
                let label = state_label(&state, language, accent);
                let icon = game.icon.as_ref().and_then(|path| {
                    cx.textures.get(path, LIBRARY_ICON).map(|loaded| TileImage {
                        texture: loaded.id,
                        aspect: loaded.aspect,
                    })
                });
                LibraryEntry {
                    name: game.name.clone(),
                    icon,
                    overlay: tile_overlay(&state),
                    state: StateText {
                        icon: label.icon,
                        text: label.text,
                        tint: label.tint,
                    },
                }
            })
            .collect();
        let data = LibraryData {
            entries: &entries,
            language,
            accent,
        };
        for action in self.library.show(cx.shell, cx.ctx, &events, &data) {
            let model = &mut *cx.model;
            match action {
                LibraryAction::Pick(index) => {
                    if index < model.games.len() {
                        model.selected = index;
                        model.focus = HomeFocus::Tiles;
                        model.measure_selected();
                    }
                    model.dismiss_overlay();
                }
                LibraryAction::Close => model.dismiss_overlay(),
            }
        }
    }

    fn log_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        if wanted && !self.log.is_active() {
            self.log.open();
        } else if !wanted && self.log.is_active() {
            self.log.close();
        }
        let events = self.take_events(wanted);
        let game = cx.model.selected_game().cloned();
        let data = LogData {
            game_name: game.as_ref().map(|g| g.name.as_str()),
            diagnosis: self.log_diagnosis.as_ref(),
            text: &self.log_text,
            language: cx.model.language(),
        };
        let actions = self.log.show(cx.shell, cx.ctx, &events, &data);
        for action in actions {
            match action {
                LogAction::Close => cx.model.dismiss_overlay(),
                LogAction::MakeReport => {
                    if let Some(game) = &game {
                        self.make_report(cx.model, game);
                    }
                }
            }
        }
    }

    // --- log and report ------------------------------------------------------------------------

    /// The text of the log view (DESIGN.md 5.12): the failure message, then the end of the log file.
    fn prepare_log(&mut self, model: &Model, game: &Game) {
        let mut text = String::new();
        if let Some(failure) = model.failures.get(&game.id) {
            text.push_str(failure);
            text.push_str("\n\n");
        }
        let file = model.paths.log_file(&game.title_id);
        if let Ok(bytes) = diagnosis::read_tail(&file, LOG_TAIL_BYTES) {
            text.push_str(&String::from_utf8_lossy(&bytes));
        }
        self.log_text = if text.trim().is_empty() {
            "—".to_owned()
        } else {
            text
        };
        // The sentence above the log only when the game ended on an error.
        self.log_diagnosis = model
            .diagnoses
            .contains_key(&game.id)
            .then(|| Diagnosis::read(&diagnosis::read_last_session(&file), None))
            .flatten();
    }

    /// Writes the report on a worker thread, then shows it in Explorer (DESIGN.md 5.13).
    fn make_report(&mut self, model: &Model, game: &Game) {
        let paths: AppPaths = model.paths.clone();
        let settings =
            serde_json::to_value(model.prefs.game_settings(&game.id)).unwrap_or_default();
        let (name, title_id, version) = (
            game.name.clone(),
            game.title_id.clone(),
            game.version.clone(),
        );
        let toolkit_version = model.toolkit.clone();
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        spawn("report", move || {
            let session = diagnosis::read_last_session(&paths.log_file(&title_id));
            let input = ReportInput {
                game_name: name,
                title_id,
                version,
                system: SystemInfo::collect(),
                launcher_version: env!("CARGO_PKG_VERSION").to_owned(),
                toolkit_version,
                reason: Diagnosis::read(&session, None).map(|d| d.summary()),
                settings,
                crash: Diagnosis::crash_name(&session, None),
                session,
                now: clock::rfc3339_utc(clock::unix_seconds()),
            };
            let result = report::write_report(&paths, &input).map_err(|e| e.to_string());
            let _ = tx.send(Work::Report(result));
            ctx.request_repaint();
        });
    }

    /// Deletes the unlocked copy on a worker thread (a game can be tens of gigabytes).
    fn start_uninstall(&mut self, game: &Game, root: PathBuf) {
        let (tx, ctx) = (self.work_tx.clone(), self.ctx.clone());
        let (title_id, id) = (game.title_id.clone(), game.id.clone());
        spawn("uninstall", move || {
            let result = unlock::remove_unlocked(&title_id, &id, &root).map_err(|e| e.to_string());
            let _ = tx.send(Work::Uninstalled { game: id, result });
            ctx.request_repaint();
        });
    }

    // --- per frame -----------------------------------------------------------------------------

    fn poll_work(&mut self, model: &mut Model) {
        while let Ok(work) = self.work_rx.try_recv() {
            let language = model.language();
            match work {
                Work::Report(Ok(path)) => {
                    if let Err(error) = report::reveal_in_explorer(&path) {
                        tracing::warn!("cannot show the report: {error}");
                    }
                    model.toast(language.text(Key::ReportSaved), Icon::FILE_TEXT);
                }
                Work::Report(Err(error)) => {
                    tracing::warn!("cannot write the report: {error}");
                    model.toast(language.text(Key::ReportFailed), Icon::WARNING);
                }
                Work::Uninstalled { game, result } => match result {
                    Ok(()) => {
                        if let Some(g) = model.games.iter_mut().find(|g| g.id == game) {
                            g.unlocked_with = None;
                        }
                        model.failures.remove(&game);
                        model.trophies.remove(&game);
                        model.toast(language.text(Key::UninstallDone), Icon::TRASH);
                    }
                    Err(error) => {
                        tracing::warn!("cannot uninstall: {error}");
                        model.toast(error, Icon::WARNING);
                    }
                },
            }
        }
    }

    fn poll_toolkit(&mut self, model: &mut Model) {
        for event in self.toolkit.poll() {
            let language = model.language();
            match event {
                ToolkitEvent::Checked(Ok(_)) => {
                    let stamp = clock::rfc3339_utc(clock::unix_seconds());
                    model.edit_prefs(|p| p.toolkit_checked_at = Some(stamp));
                }
                ToolkitEvent::Checked(Err(error)) => tracing::warn!("update check: {error}"),
                ToolkitEvent::Installed(Ok(version)) => {
                    self.resolutions.clear();
                    model.edit_prefs(|p| p.toolkit = None);
                    model.effects.push(Effect::ReloadToolkit);
                    let text = Extra::ToolkitReady.format(language, &[Arg::Str(&version)]);
                    model.toast(text, Icon::CHECK_CIRCLE);
                }
                ToolkitEvent::Adopted(Ok((folder, version))) => {
                    self.resolutions.clear();
                    model.edit_prefs(|p| p.toolkit = Some(folder));
                    model.effects.push(Effect::ReloadToolkit);
                    let text = Extra::ToolkitReady.format(language, &[Arg::Str(&version)]);
                    model.toast(text, Icon::CHECK_CIRCLE);
                }
                ToolkitEvent::Installed(Err(error)) => {
                    tracing::warn!("toolkit install: {error}");
                    model.toast(Extra::ToolkitFailed.text(language), Icon::WARNING);
                }
                ToolkitEvent::Adopted(Err(error)) => {
                    tracing::warn!("toolkit folder: {error}");
                    model.toast(Extra::ToolkitFolderInvalid.text(language), Icon::WARNING);
                }
            }
        }
    }

    /// Trophies earned by the running game raise banners (own viewport) and refresh the list.
    fn run_banners(&mut self, cx: &mut OverlayCx<'_>) {
        let earned = std::mem::take(&mut cx.model.trophy_banners);
        let mut refresh: Vec<GameId> = Vec::new();
        for (game, trophy) in earned {
            self.banners.push(BannerTrophy::from_core(&trophy));
            if !refresh.contains(&game) {
                refresh.push(game);
            }
        }
        for game in refresh {
            cx.model.reload_trophies(&game);
        }
        let monitor_width = cx
            .ctx
            .input(|i| i.viewport().monitor_size)
            .map_or(1920.0, |size| size.x);
        let reduced = cx.model.prefs.reduced_motion;
        let language = cx.model.language();
        self.banners
            .show(cx.shell, cx.ctx, reduced, monitor_width, language);
    }
}

impl OverlayHost for PanelHost {
    fn command(&mut self, cx: &mut OverlayCx<'_>, _overlay: Overlay, command: Command) {
        self.events.push(InputEvent::Command(command));
        cx.ctx.request_repaint();
    }

    fn ui(&mut self, _ui: &mut Ui, cx: &mut OverlayCx<'_>, overlay: Overlay, _openness: f32) {
        let wanted = cx.model.overlay == Some(overlay);
        match overlay {
            Overlay::Settings => self.settings_ui(cx, wanted),
            Overlay::GameSettings => self.game_settings_ui(cx, wanted),
            Overlay::ConfirmUninstall => self.uninstall_ui(cx, wanted),
            Overlay::Controls => self.controls_ui(cx, wanted),
            Overlay::Advanced => self.advanced_ui(cx, wanted),
            Overlay::Trophies => self.trophies_ui(cx, wanted),
            Overlay::Library => self.library_ui(cx, wanted),
            Overlay::Log => self.log_ui(cx, wanted),
        }
    }

    fn captured_key(
        &mut self,
        cx: &mut OverlayCx<'_>,
        key: egui::Key,
        physical_key: Option<egui::Key>,
        modifiers: egui::Modifiers,
    ) {
        self.events.push(InputEvent::CapturedKey {
            key,
            physical_key,
            modifiers,
        });
        cx.ctx.request_repaint();
    }

    fn frame(&mut self, cx: &mut OverlayCx<'_>) {
        // Keys that arrived while no overlay was open belong to nobody.
        if cx.model.overlay.is_none() {
            self.events.clear();
        }
        self.poll_work(cx.model);
        self.poll_toolkit(cx.model);
        // No toolkit on this PC: fetch it now, already during the onboarding, so the first unlock
        // needs no visit to the settings. The result shows as a toast (see `poll_toolkit`).
        if !self.toolkit_auto_tried && cx.model.toolkit.is_none() {
            self.toolkit_auto_tried = true;
            let model = &*cx.model;
            self.toolkit
                .install(&model.paths, &model.prefs.toolkit_repository);
        }
        if !self.checked_at_start && cx.model.onboarded() {
            self.checked_at_start = true;
            if cx.model.prefs.check_for_updates {
                self.check_toolkit(cx.model);
            }
        }
        self.run_banners(cx);
    }
}

/// Stores a game's edited settings and marks the preferences for saving.
fn store_game_settings(model: &mut Model, id: &GameId, settings: GameSettings) {
    model.prefs.set_game_settings(id, settings);
    model.mark_prefs_dirty();
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}

/// Opens a folder in Explorer.
fn open_folder(folder: &std::path::Path) {
    #[cfg(windows)]
    {
        if let Err(error) = std::process::Command::new("explorer.exe")
            .arg(folder)
            .spawn()
        {
            tracing::warn!("cannot open {}: {error}", folder.display());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = folder;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pipeline::SimPipeline;
    use crate::app::preview::{Scene, apply_scene, make_games};
    use crate::app::textures::Textures;
    use crate::app::{Config, LauncherApp};
    use crate::ui::Shell;
    use crate::ui::focus::HomeIntent;
    use crate::ui::theme::Theme;
    use anyps5_core::settings::{Preferences, Theme as CoreTheme};
    use egui::{Pos2, RawInput, Rect, vec2};

    /// A launcher app on a temp folder with the panel host, driven frame by frame.
    struct Rig {
        ctx: Context,
        app: LauncherApp,
        dir: PathBuf,
        time: f64,
    }

    impl Rig {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("anyps5-panels-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let ctx = Context::default();
            let config = Config {
                prefs: Preferences::default(),
                paths: AppPaths::under(&dir.join("appdata")),
                persist: true,
                pipeline: Box::new(SimPipeline::new("0.1.0")),
                panels: Box::new(PanelHost::new(&ctx, true)),
                scan_on_start: false,
            };
            let shell = Shell::for_test(&ctx, Theme::Cosmic);
            let mut app = LauncherApp::from_shell(shell, &ctx, config);
            app.offline = true;
            apply_scene(&mut app, Scene::Home, make_games(&dir.join("art")));
            // The scene touched the preferences; start from a clean file.
            app.model.take_prefs_dirty();
            let mut rig = Self {
                ctx,
                app,
                dir,
                time: 1.0,
            };
            rig.frames(3);
            rig
        }

        fn frames(&mut self, count: usize) {
            for _ in 0..count {
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))),
                    time: Some(self.time),
                    ..Default::default()
                };
                self.time += 0.1;
                let ctx = self.ctx.clone();
                let app = &mut self.app;
                let mut output = ctx.run_ui(input, |ui| {
                    app.step(&ctx);
                    app.paint(ui);
                });
                output.textures_delta.clear();
            }
        }

        fn press(&mut self, command: Command) {
            let ctx = self.ctx.clone();
            self.app.handle_event(&ctx, InputEvent::Command(command));
            self.frames(2);
        }

        fn saved(&self) -> Preferences {
            Preferences::load(&self.app.model.paths)
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn cx<'a>(rig: &'a mut Rig, textures: &'a mut Textures) -> OverlayCx<'a> {
        OverlayCx {
            model: &mut rig.app.model,
            shell: &mut rig.app.shell,
            textures,
            ctx: &rig.ctx,
        }
    }

    #[test]
    fn every_overlay_opens_and_closes_with_back() {
        let mut rig = Rig::new("overlays");
        for overlay in [
            Overlay::Settings,
            Overlay::Library,
            Overlay::GameSettings,
            Overlay::Controls,
            Overlay::Advanced,
            Overlay::Trophies,
            Overlay::Log,
            Overlay::ConfirmUninstall,
        ] {
            rig.app.model.open(overlay);
            rig.frames(3);
            assert_eq!(rig.app.model.overlay, Some(overlay));
            rig.press(Command::Back);
            // Controls and advanced step back to the game settings, the rest close.
            let expected = match overlay {
                Overlay::Controls | Overlay::Advanced => Some(Overlay::GameSettings),
                _ => None,
            };
            assert_eq!(rig.app.model.overlay, expected, "back from {overlay:?}");
            if expected.is_some() {
                rig.press(Command::Back);
                assert_eq!(rig.app.model.overlay, None);
            }
        }
    }

    #[test]
    fn home_buttons_open_the_panels() {
        let mut rig = Rig::new("home");
        for (intent, overlay) in [
            (HomeIntent::OpenSettings, Overlay::Settings),
            (HomeIntent::OpenLibrary, Overlay::Library),
            (HomeIntent::OpenGameSettings, Overlay::GameSettings),
            (HomeIntent::OpenTrophies, Overlay::Trophies),
        ] {
            rig.app.model.apply_intent(intent);
            assert_eq!(rig.app.model.overlay, Some(overlay));
            rig.frames(3);
            rig.press(Command::Back);
            assert_eq!(rig.app.model.overlay, None);
        }
    }

    #[test]
    fn theme_change_reaches_the_shell_and_is_saved() {
        let mut rig = Rig::new("theme");
        rig.app.model.open(Overlay::Settings);
        rig.frames(2);
        // Appearance, into its rows (theme row first), one step right.
        rig.press(Command::Select);
        rig.press(Command::Right);
        assert_eq!(rig.app.model.prefs.theme, CoreTheme::Aurora);
        assert_eq!(rig.app.shell.backdrop.theme(), Theme::Aurora);
        rig.frames(2);
        assert_eq!(rig.saved().theme, CoreTheme::Aurora, "written to disk");
    }

    #[test]
    fn game_setting_changes_are_stored_and_saved() {
        let mut rig = Rig::new("game");
        let id = rig.app.model.selected_game().unwrap().id.clone();
        assert!(!rig.app.model.prefs.game_settings(&id).fullscreen);
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(2);
        // The first row is the display mode (no patch, so no resolution row).
        rig.press(Command::Right);
        assert!(rig.app.model.prefs.game_settings(&id).fullscreen);
        rig.frames(2);
        assert!(rig.saved().game_settings(&id).fullscreen);
    }

    #[test]
    fn looking_without_changing_writes_nothing() {
        let mut rig = Rig::new("quiet");
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(4);
        rig.press(Command::Down);
        rig.press(Command::Up);
        rig.press(Command::Back);
        rig.frames(3);
        assert!(!rig.app.model.paths.preferences_file().exists());
    }

    #[test]
    fn controls_row_opens_the_controls_and_back_returns() {
        let mut rig = Rig::new("pages");
        rig.app.model.open(Overlay::GameSettings);
        rig.frames(3);
        // display, size, vsync, controls
        for _ in 0..3 {
            rig.press(Command::Down);
        }
        rig.press(Command::Select);
        assert_eq!(rig.app.model.overlay, Some(Overlay::Controls));
        rig.frames(3);
        rig.press(Command::Back);
        assert_eq!(rig.app.model.overlay, Some(Overlay::GameSettings));
    }

    #[test]
    fn library_pick_selects_the_game_and_closes() {
        let mut rig = Rig::new("library");
        rig.app.model.open(Overlay::Library);
        rig.frames(3);
        let before = rig.app.model.selected;
        rig.press(Command::Right);
        rig.press(Command::Select);
        assert_eq!(rig.app.model.overlay, None);
        assert_eq!(rig.app.model.selected, before + 1);
    }

    #[test]
    fn reinstall_closes_the_panel_and_queues_the_unlock() {
        let mut rig = Rig::new("reinstall");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        rig.app.model.toolkit = Some("0.1.0".into());
        rig.app.model.open(Overlay::GameSettings);
        let mut textures = Textures::new(&rig.ctx);
        host.game_settings_action(
            &mut cx(&mut rig, &mut textures),
            &game,
            GameSettingsAction::Reinstall,
        );
        assert_eq!(rig.app.model.overlay, None);
        assert!(rig.app.model.queue.contains(&game.id));
    }

    #[test]
    fn page_actions_switch_overlays_without_closing() {
        let mut rig = Rig::new("switch");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        let mut textures = Textures::new(&rig.ctx);
        for (action, expected) in [
            (GameSettingsAction::OpenControls, Overlay::Controls),
            (GameSettingsAction::OpenAdvanced, Overlay::Advanced),
            (GameSettingsAction::OpenTrophies, Overlay::Trophies),
            (GameSettingsAction::AskUninstall, Overlay::ConfirmUninstall),
            (GameSettingsAction::OpenLog, Overlay::Log),
        ] {
            rig.app.model.open(Overlay::GameSettings);
            host.game_settings_action(&mut cx(&mut rig, &mut textures), &game, action);
            assert_eq!(rig.app.model.overlay, Some(expected));
        }
        host.back_to_game_settings(&mut rig.app.model, "advanced");
        assert_eq!(rig.app.model.overlay, Some(Overlay::GameSettings));
        assert_eq!(host.return_row, Some("advanced"));
    }

    #[test]
    fn settings_actions_reach_the_model() {
        let mut rig = Rig::new("settings-actions");
        let mut host = PanelHost::new(&rig.ctx, true);
        let mut textures = Textures::new(&rig.ctx);
        rig.app.model.open(Overlay::Settings);
        host.settings_action(&mut cx(&mut rig, &mut textures), SettingsAction::Rescan);
        assert!(rig.app.model.scanning, "a rescan was asked for");
        host.settings_action(&mut cx(&mut rig, &mut textures), SettingsAction::Close);
        assert_eq!(rig.app.model.overlay, None);
    }

    #[test]
    fn uninstall_removes_the_copy_off_thread_and_updates_the_game() {
        let mut rig = Rig::new("uninstall");
        let mut host = PanelHost::new(&rig.ctx, true);
        let game = rig.app.model.selected_game().cloned().unwrap();
        let root = rig.dir.join("games");
        let copy = anyps5_core::paths::unlocked_copy_dir(&root, &game.title_id, &game.id);
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("eboot.bin"), b"x").unwrap();
        host.start_uninstall(&game, root);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            host.poll_work(&mut rig.app.model);
            if rig.app.model.toast.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!copy.exists());
        let selected = rig.app.model.selected;
        assert!(rig.app.model.games[selected].unlocked_with.is_none());
    }
}
