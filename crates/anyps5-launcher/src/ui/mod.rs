//! UI foundation of the launcher (docs/DESIGN.md): tokens, motion, backgrounds, components,
//! sounds, localisation and input. Screens build on this module only; nothing here knows about
//! games, preferences or the core crate.
//!
//! | Module | What it gives the screens |
//! |---|---|
//! | [`theme`] | colours, radii, sizes, shadows, type scale, fonts, window scale |
//! | [`anim`] | springs, easings, `ctx.animate_to(...)` / `ctx.focus_t(...)`, endless loops |
//! | [`background`] | the three animated themes, [`Backdrop`](background::Backdrop) driver |
//! | [`blur`], [`glass`] | material blur and frosted panels |
//! | [`components`], [`chrome`] | buttons, switch, progress, hints, tile, rows, panels, toast |
//! | [`text`] | text layout with line height, tracking, truncation, tabular figures |
//! | [`icons`] | Phosphor glyphs |
//! | [`sound`] | the four synthesised cues |
//! | [`i18n`] | 212 keys in English and Turkish |
//! | [`input`], [`focus`], [`gamepad`] | commands, wheel, gamepad, home and panel focus models |
//!
//! A screen host creates a [`Shell`] once and calls [`Shell::begin_frame`] at the start of every
//! frame; see its documentation.

// The foundation is consumed by the screens of later PRs; until then most of its API is unused.
#![allow(dead_code)]

pub mod anim;
pub mod background;
pub mod blur;
pub mod chrome;
pub mod components;
pub mod focus;
pub mod gallery;
pub mod gamepad;
pub mod glass;
pub mod gpu;
pub mod i18n;
pub mod icons;
pub mod input;
pub mod sound;
pub mod text;
pub mod theme;

use eframe::egui_wgpu;
use egui::{Context, Painter, Rect};

use self::anim::AnimExt;
use self::background::Backdrop;
use self::i18n::Language;
use self::input::{Input, InputEvent};
use self::sound::{Cue, Sounds};
use self::theme::{Accent, Theme};

/// Long-lived UI state every screen needs: input pipeline, sounds, backdrop, language and accent.
pub struct Shell {
    pub input: Input,
    pub sounds: Sounds,
    pub backdrop: Backdrop,
    pub language: Language,
    /// The accent colour in use (the theme's default unless the user picked one).
    pub accent: Accent,
    scale: f32,
}

/// What [`Shell::begin_frame`] hands to the screen.
pub struct FrameInput {
    /// Global scale in use (1 dp = 1 egui point once it is applied).
    pub scale: f32,
    /// Commands, wheel steps and shortcuts of this frame, in order.
    pub events: Vec<InputEvent>,
}

impl Shell {
    /// Installs fonts and the dark visuals, builds the GPU pipelines of the backdrop and starts the
    /// gamepad thread. Call from the eframe app creator.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        } else {
            tracing::warn!("no wgpu render state: backdrop and glass are unavailable");
        }
        let pad = gamepad::spawn(cc.egui_ctx.clone());
        let language = Language::system();
        Self {
            input: Input::new(Some(pad)),
            sounds: Sounds::new(),
            backdrop: Backdrop::new(Theme::Cosmic),
            language,
            accent: Theme::Cosmic.default_accent(),
            scale: 1.0,
        }
    }

    /// A shell for tests and the gallery: no gamepad, silent.
    pub fn headless(cc: &eframe::CreationContext<'_>, theme: Theme) -> Self {
        theme::install(&cc.egui_ctx);
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        }
        let mut sounds = Sounds::new();
        sounds.enabled = false;
        Self {
            input: Input::new(None),
            sounds,
            backdrop: Backdrop::new(theme),
            language: Language::English,
            accent: theme.default_accent(),
            scale: 1.0,
        }
    }

    /// Call first thing in every frame: applies the window scale and collects input events.
    pub fn begin_frame(&mut self, ctx: &Context) -> FrameInput {
        self.scale = theme::sync_scale(ctx);
        FrameInput {
            scale: self.scale,
            events: self.input.poll(ctx),
        }
    }

    /// Switches the theme and, unless the user chose an accent, the accent with it.
    pub fn set_theme(&mut self, theme: Theme, user_accent: Option<Accent>) {
        self.backdrop.set_theme(theme);
        self.accent = user_accent.unwrap_or_else(|| theme.default_accent());
    }

    /// "Animations: Reduced": swaps springs for short eases and freezes the background.
    pub fn set_reduced_motion(&mut self, ctx: &Context, reduced: bool) {
        ctx.set_reduced_motion(reduced);
    }

    /// Updates the clock and paints the backdrop into `rect` (usually the whole window). Call
    /// before any other UI.
    pub fn paint_backdrop(&mut self, ctx: &Context, painter: &Painter, rect: Rect) {
        self.backdrop.update(ctx);
        self.backdrop.paint(ctx, painter, rect);
    }

    /// Plays a cue (no-op when sounds are off).
    pub fn play(&mut self, cue: Cue) {
        self.sounds.play(cue);
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The accent as a colour.
    pub fn accent_color(&self) -> egui::Color32 {
        self.accent.color()
    }
}

/// Restricts wgpu to Vulkan and DX12 (OpenGL probing crashes some AMD drivers).
pub fn restrict_backends(options: &mut eframe::NativeOptions) {
    use egui_wgpu::{WgpuSetup, wgpu::Backends};
    if let WgpuSetup::CreateNew(create_new) = &mut options.wgpu_options.wgpu_setup {
        create_new.instance_descriptor.backends = Backends::VULKAN | Backends::DX12;
    }
}
