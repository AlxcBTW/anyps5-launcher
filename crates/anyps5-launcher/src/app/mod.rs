//! The application: the model, the I/O behind it and the host that drives the screens each frame.
//!
//! | Module | What it is |
//! |---|---|
//! | [`model`] | the state machine (`AppModel.swift`): states, actions, onboarding, library bookkeeping |
//! | [`pipeline`] | the seam to the unlock/launch pipeline of `anyps5-core` |
//! | [`overlay`] | the overlay enum and the hook the panel modules implement |
//! | [`services`] | scan, watcher, size measuring, trophy loading, folder dialog |
//! | [`textures`] | icons and artwork as textures, decoded off the UI thread |
//! | [`preview`] | made-up games, scenes and the `--gallery=home...` screenshots |
//!
//! [`LauncherApp`] is the `eframe::App`. Per frame it applies finished work to the model, turns
//! input into model commands, runs the model's effects and paints: backdrop, game artwork, the
//! onboarding or the home screen, the overlays, the launch splash and the toast.

// Parts of the model, pipeline and overlay API are for the panel modules and the pipeline adapter.
#![allow(dead_code)]

pub mod clock;
pub mod core_pipeline;
pub mod model;
pub mod overlay;
pub mod panels;
pub mod pipeline;
pub mod preview;
pub mod services;
pub mod textures;
pub mod toolkit;
pub mod window_chrome;

use std::path::PathBuf;
use std::time::Duration;

use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::settings::Preferences;
use egui::{Context, Rect, Ui, UiBuilder, ViewportCommand};

use crate::screens::{self, ScreenCx};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::artwork;
use crate::ui::chrome;
use crate::ui::i18n::Key;
use crate::ui::input::InputEvent;

use self::core_pipeline::CorePipeline;
use self::model::{Effect, Model, Routed, ui_accent};
use self::overlay::{Overlay, OverlayCx, OverlayHost};
use self::pipeline::{Pipeline, PipelineContext};
use self::services::Services;
use self::textures::Textures;

/// Everything the host is made of; the real launcher and the previews differ only in these.
pub struct Config {
    pub prefs: Preferences,
    pub paths: AppPaths,
    /// Save the preferences (off for previews and tests).
    pub persist: bool,
    pub pipeline: Box<dyn Pipeline>,
    pub panels: Box<dyn OverlayHost>,
    /// Scan the games folder at start (off when the library is set up by hand).
    pub scan_on_start: bool,
}

impl Config {
    /// The launcher as users run it: preferences from `%APPDATA%`, the core pipeline, and no panels
    /// until a panel module registers its host.
    pub fn from_environment(ctx: &Context) -> Self {
        let paths = AppPaths::from_env()
            .unwrap_or_else(|| AppPaths::under(&std::env::temp_dir().join("anyps5-launcher")));
        let prefs = Preferences::load(&paths);
        let repaint = ctx.clone();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || repaint.request_repaint());
        Self {
            pipeline: Box::new(CorePipeline::new(paths.clone(), &prefs, wake)),
            prefs,
            paths,
            persist: true,
            panels: Box::new(panels::PanelHost::new(ctx, false)),
            scan_on_start: true,
        }
    }
}

/// Which preference values the shell has been given, to apply changes once.
#[derive(Default)]
struct Applied {
    reduced_motion: Option<bool>,
}

pub struct LauncherApp {
    pub shell: Shell,
    pub model: Model,
    pub textures: Textures,
    pub services: Services,
    pub pipeline: Box<dyn Pipeline>,
    pub panels: Box<dyn OverlayHost>,
    applied: Applied,
    focused: bool,
    /// Overlays that are open or still animating out.
    overlays_shown: Vec<Overlay>,
    /// Artwork behind the home screen that is fading in or out, newest last.
    artwork: Vec<GameId>,
    started: bool,
    start_screen: screens::start::StartScreen,
    scan_on_start: bool,
    /// No scan, size or trophy work (previews and the demo have a made-up library).
    pub offline: bool,
    /// Keep the backdrop clock still (screenshots).
    pub freeze_backdrop: bool,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Self::from_shell(Shell::new(cc), &cc.egui_ctx, config)
    }

    /// With an already built shell (previews use a silent one).
    pub fn from_shell(shell: Shell, ctx: &Context, config: Config) -> Self {
        let mut model = Model::new(config.prefs, config.paths, config.persist);
        model.toolkit = config.pipeline.toolkit_version();
        Self {
            shell,
            model,
            textures: Textures::new(ctx),
            services: Services::new(ctx),
            pipeline: config.pipeline,
            panels: config.panels,
            applied: Applied::default(),
            focused: true,
            overlays_shown: Vec::new(),
            artwork: Vec::new(),
            started: false,
            start_screen: screens::start::StartScreen::default(),
            scan_on_start: config.scan_on_start,
            offline: false,
            freeze_backdrop: false,
        }
    }

    // --- per frame ----------------------------------------------------------------------------

    /// Everything before painting: finished work, input, effects, preferences.
    pub fn step(&mut self, ctx: &Context) {
        self.shell.input.blocked = self.services.dialog_open();
        let frame = self.shell.begin_frame(ctx);
        let now = ctx.clock();
        self.model.tick(now);

        if !self.started {
            self.started = true;
            if self.scan_on_start && self.model.onboarded() {
                self.model.rescan();
            }
        }

        for event in self.services.drain() {
            self.model.on_core(event);
        }
        self.model.toolkit = self.pipeline.toolkit_version();
        self.model.patches_dir = self.pipeline.patches_dir();
        for event in self.pipeline.poll(now) {
            self.model.on_pipeline(event);
        }
        if let Some(answer) = self.services.poll_folder() {
            self.model.choose_folder(answer);
        }
        self.textures.poll();

        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        if focused && !self.focused {
            self.model.window_focused();
        }
        self.focused = focused;

        self.apply_preferences(ctx);
        for event in frame.events {
            self.handle_event(ctx, event);
        }
        self.run_effects(ctx);
        self.watch_trophies();

        self.services.set_busy(self.model.busy());
        self.save_preferences();
    }

    /// Writes the preferences when something changed (atomic, see `Preferences::save`).
    pub fn save_preferences(&mut self) {
        if self.model.take_prefs_dirty()
            && let Err(error) = self.model.prefs.save(&self.model.paths)
        {
            tracing::warn!("cannot save the preferences: {error}");
        }
    }

    /// The running game's trophy records are polled for the banner while it runs.
    fn watch_trophies(&mut self) {
        if self.offline {
            return;
        }
        let running = self.model.running.clone();
        match running.and_then(|id| self.model.game(&id).cloned()) {
            Some(game) => {
                let root = self.model.prefs.games_folder.clone();
                self.services
                    .watch_trophies(&game, root.as_deref(), &self.model.prefs);
            }
            None => self.services.stop_trophy_watch(),
        }
    }

    /// Shows the preferences to the shell: language, theme, accent, motion, sounds, music.
    fn apply_preferences(&mut self, ctx: &Context) {
        self.shell.language = self.model.language();
        let theme = self.model.theme();
        let accent = self.model.prefs.accent.map(ui_accent);
        let wanted_accent = accent.unwrap_or_else(|| theme.default_accent());
        if self.shell.backdrop.theme() != theme || self.shell.accent != wanted_accent {
            self.shell.set_theme(theme, accent);
        }
        let reduced = self.model.prefs.reduced_motion;
        if self.applied.reduced_motion != Some(reduced) {
            self.applied.reduced_motion = Some(reduced);
            self.shell.set_reduced_motion(ctx, reduced);
        }
        self.shell.sounds.enabled = self.model.prefs.sounds;
        // The music fades out while a game starts or runs and while another window has focus.
        let playing = self.model.running.is_some() || self.model.starting.is_some();
        self.shell
            .sounds
            .set_music(self.model.prefs.music && !playing && self.focused);
        self.shell.backdrop.paused =
            self.model.running.is_some() || !self.focused || self.freeze_backdrop;
    }

    pub fn handle_event(&mut self, ctx: &Context, event: InputEvent) {
        match event {
            InputEvent::Command(command) => match self.model.handle(command) {
                Routed::Overlay(overlay) => {
                    let mut cx = OverlayCx {
                        model: &mut self.model,
                        shell: &mut self.shell,
                        textures: &mut self.textures,
                        ctx,
                    };
                    self.panels.command(&mut cx, overlay, command);
                }
                Routed::Done => {}
            },
            InputEvent::Scroll(command) => {
                if self.model.onboarded()
                    && self.model.overlay.is_none()
                    && self.model.starting.is_none()
                {
                    let delta = if command == crate::ui::input::Command::Left {
                        -1
                    } else {
                        1
                    };
                    self.model.move_selection(delta);
                }
            }
            InputEvent::OpenSettings => {
                if self.model.onboarded() && self.model.starting.is_none() {
                    self.model.open(Overlay::Settings);
                }
            }
            InputEvent::ToggleFullscreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!fullscreen));
            }
            InputEvent::CapturedKey {
                key,
                physical_key,
                modifiers,
            } => {
                let mut cx = OverlayCx {
                    model: &mut self.model,
                    shell: &mut self.shell,
                    textures: &mut self.textures,
                    ctx,
                };
                self.panels
                    .captured_key(&mut cx, key, physical_key, modifiers);
            }
        }
    }

    /// Executes what the model queued.
    fn run_effects(&mut self, ctx: &Context) {
        for _ in 0..4 {
            let effects = std::mem::take(&mut self.model.effects);
            if effects.is_empty() {
                break;
            }
            for effect in effects {
                self.run_effect(ctx, effect);
            }
        }
    }

    fn run_effect(&mut self, ctx: &Context, effect: Effect) {
        let root = self.model.prefs.games_folder.clone();
        match effect {
            Effect::Sound(cue) => self.shell.play(cue),
            Effect::Pipeline(command) => {
                let game = self.model.game(command.game()).cloned();
                self.pipeline.submit(
                    command,
                    &PipelineContext {
                        root: root.as_deref(),
                        game: game.as_ref(),
                        prefs: &self.model.prefs,
                    },
                );
            }
            Effect::Rescan if self.offline => self.model.scanning = false,
            Effect::MeasureSize(_) | Effect::LoadTrophies { .. } if self.offline => {}
            Effect::Rescan => {
                if let Some(root) = root {
                    if !self.model.busy() {
                        self.pipeline.before_scan(&root);
                    }
                    self.services.rescan(&root);
                }
            }
            Effect::PickGamesFolder => {
                let title = self.model.language().text(Key::PickFolder);
                self.services.pick_folder(title, root.as_deref(), ctx);
            }
            Effect::MeasureSize(id) => {
                if let Some(game) = self.model.game(&id) {
                    self.services.measure(game);
                }
            }
            Effect::LoadTrophies { game, .. } => {
                if let Some(game) = self.model.game(&game) {
                    self.services
                        .load_trophies(game, root.as_deref(), &self.model.prefs);
                }
            }
            Effect::ReloadToolkit => self.pipeline.reload_toolkit(&self.model.prefs),
            Effect::MinimizeWindow => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Effect::RestoreWindow => {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
        }
    }

    // --- painting ------------------------------------------------------------------------------

    /// Paints one frame: backdrop, artwork, screen, overlays, splash, toast.
    pub fn paint(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let painter = ui.painter().clone();
        self.shell.paint_backdrop(&ctx, &painter, screen);
        self.paint_artwork(&ctx, screen);

        let onboarded = ctx.animate_to(
            "onboarded",
            f32::from(u8::from(self.model.onboarded())),
            Motion::Fade,
        );
        // Fade the onboarding out, then the home screen in.
        let (show_onboarding, show_home, opacity) = if onboarded < 0.5 {
            (true, false, 1.0 - 2.0 * onboarded)
        } else {
            (false, true, 2.0 * onboarded - 1.0)
        };
        {
            let mut child = ui.new_child(UiBuilder::new().max_rect(screen));
            child.set_opacity(opacity.clamp(0.0, 1.0));
            let mut cx = ScreenCx {
                model: &mut self.model,
                shell: &self.shell,
                textures: &mut self.textures,
                ctx: &ctx,
            };
            if show_onboarding {
                screens::onboarding::show(&mut child, &mut cx);
            } else if show_home {
                screens::home::show(&mut child, &mut cx);
            }
        }

        self.paint_overlays(ui, &ctx);
        self.paint_start(&ctx);
        self.paint_toast(&ctx);
        let mut cx = OverlayCx {
            model: &mut self.model,
            shell: &mut self.shell,
            textures: &mut self.textures,
            ctx: &ctx,
        };
        self.panels.frame(&mut cx);
        self.run_effects(&ctx);
        self.save_preferences();
        self.schedule_repaints(&ctx);
    }

    /// The launch splash (the panels module's `StartScreen`) follows the model's `starting`.
    fn paint_start(&mut self, ctx: &Context) {
        let starting = self.model.starting.clone();
        if starting.is_some() && !self.start_screen.is_open() {
            self.start_screen.open_at(ctx.clock());
        } else if starting.is_none() && self.start_screen.is_open() {
            self.start_screen.close();
        }
        if !self.start_screen.is_active() {
            return;
        }
        // The last game shown stays for the way out.
        let id = starting.or_else(|| self.model.running.clone());
        let Some(game) = id.and_then(|id| self.model.game(&id).cloned()) else {
            return;
        };
        let icon = game.icon.as_ref().and_then(|path| {
            self.textures
                .get(path, 400)
                .map(|l| crate::ui::chrome::TileImage {
                    texture: l.id,
                    aspect: l.aspect,
                })
        });
        let data = screens::start::StartData {
            game_name: &game.name,
            icon,
            language: self.model.language(),
            accent: self.shell.accent_color(),
            reduced_motion: ctx.reduced_motion(),
        };
        // Back is the model's: it cancels the start and closes the splash.
        self.start_screen.show(&mut self.shell, ctx, &[], &data);
    }

    fn paint_overlays(&mut self, ui: &mut Ui, ctx: &Context) {
        if let Some(open) = self.model.overlay
            && !self.overlays_shown.contains(&open)
        {
            self.overlays_shown.push(open);
        }
        let open = self.model.overlay;
        let mut still_shown = Vec::new();
        for overlay in self.overlays_shown.clone() {
            let openness = ctx.animate_to(
                ("overlay", overlay),
                f32::from(u8::from(open == Some(overlay))),
                Motion::Overlay,
            );
            if openness <= 0.003 && open != Some(overlay) {
                continue;
            }
            still_shown.push(overlay);
            let mut cx = OverlayCx {
                model: &mut self.model,
                shell: &mut self.shell,
                textures: &mut self.textures,
                ctx,
            };
            self.panels
                .ui(ui, &mut cx, overlay, openness.clamp(0.0, 1.0));
        }
        self.overlays_shown = still_shown;
    }

    fn paint_toast(&mut self, ctx: &Context) {
        let visible = self.model.toast_visible();
        let v = ctx.animate_to(
            "toast-visible",
            f32::from(u8::from(visible)),
            Motion::Overlay,
        );
        if let Some(toast) = &self.model.toast {
            chrome::toast(ctx, "launcher-toast", toast.icon, &toast.text, v);
        }
        if visible && let Some(toast) = &self.model.toast {
            let left = (toast.until - ctx.clock()).max(0.0);
            ctx.request_repaint_after(Duration::from_secs_f64(left + 0.02));
        }
    }

    /// The clock of the top bar changes every minute.
    fn schedule_repaints(&self, ctx: &Context) {
        ctx.request_repaint_after(Duration::from_secs_f32(clock::seconds_to_next_minute()));
    }

    /// The selected game's `pic0.png` behind the home screen, cross-faded per selection, drifting.
    fn paint_artwork(&mut self, ctx: &Context, screen: Rect) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let wanted = self.model.onboarded()
            && self.model.prefs.game_artwork
            && self.model.overlay != Some(Overlay::Settings);
        let current = if wanted {
            self.model
                .selected_game()
                .and_then(|g| g.background.clone().map(|path| (g.id.clone(), path)))
        } else {
            None
        };
        if current.is_none() {
            ctx.data_mut(|d| d.remove::<screens::ArtState>(screens::art_state_id()));
        }
        if let Some((id, _)) = &current {
            self.artwork.retain(|a| a != id);
            self.artwork.push(id.clone());
        }
        let ppp = ctx.pixels_per_point();
        let max_dim = ((screen.width().max(screen.height()) * ppp * 1.6) as u32).min(2048);
        let reduced = ctx.reduced_motion();
        let mut keep = Vec::new();
        for id in std::mem::take(&mut self.artwork) {
            let is_current = current.as_ref().is_some_and(|(c, _)| *c == id);
            let Some(path) = self.model.game(&id).and_then(|g| g.background.clone()) else {
                continue;
            };
            let Some(tex) = self.textures.get(&path, max_dim) else {
                keep.push(id);
                continue;
            };
            let alpha = ctx.animate_from_to(
                ("artwork", id.as_str().to_owned()),
                0.0,
                f32::from(u8::from(is_current)),
                Motion::Fade,
            );
            if is_current {
                let material = self.textures.get(&path, textures::MATERIAL);
                let state = material.map(|m| screens::ArtState {
                    material: m.id,
                    aspect: tex.aspect,
                    alpha,
                    zoom: artwork::drift_zoom(ctx.clock(), reduced),
                });
                ctx.data_mut(|d| match state {
                    Some(state) => {
                        d.insert_temp(screens::art_state_id(), state);
                    }
                    None => d.remove::<screens::ArtState>(screens::art_state_id()),
                });
            }
            if alpha > 0.003 || is_current {
                artwork::paint_artwork(
                    &painter,
                    screen,
                    tex.id,
                    tex.aspect,
                    ctx.clock(),
                    reduced,
                    alpha,
                );
                keep.push(id);
            }
            if alpha > 0.003 && !reduced {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        self.artwork = keep;
    }
}

/// Runs the launcher.
pub fn run() -> eframe::Result {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyPS5")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([
                crate::ui::theme::MIN_WINDOW.0,
                crate::ui::theme::MIN_WINDOW.1,
            ])
            .with_maximized(true),
        ..Default::default()
    };
    crate::ui::restrict_backends(&mut options);
    eframe::run_native(
        "AnyPS5",
        options,
        Box::new(|cc| {
            Ok(Box::new(App {
                app: LauncherApp::new(cc, Config::from_environment(&cc.egui_ctx)),
                chrome_applied: false,
            }))
        }),
    )
}

/// The eframe wrapper of [`LauncherApp`].
pub struct App {
    pub app: LauncherApp,
    /// The title bar is recoloured once, on the first frame (the window exists by then).
    chrome_applied: bool,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if !self.chrome_applied {
            self.chrome_applied = true;
            window_chrome::apply(frame);
        }
        let ctx = ui.ctx().clone();
        self.app.step(&ctx);
        self.app.paint(ui);
    }

    fn on_exit(&mut self) {
        self.app.save_preferences();
    }
}

/// Where the preview writes its generated game art.
pub fn preview_dir() -> PathBuf {
    std::env::temp_dir().join(format!("anyps5-preview-{}", std::process::id()))
}
