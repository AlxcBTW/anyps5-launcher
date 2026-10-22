//! Deep links in the running launcher (docs/DESIGN.md 9.5): arguments arrive at start or from a
//! later launch, become [`Link`]s, wait until the library is ready and are then shown.
//!
//! A link only moves the player to a place: it selects a game, focuses a button, opens a page of
//! the settings. It never starts a game or an unlock; the player presses the button.

use anyps5_core::library::{self, Game};
use anyps5_core::links::{self, Link, SettingsPage};
use anyps5_core::paths::GameId;

use crate::app::LauncherApp;
use crate::app::model::{Effect, Model};
use crate::app::overlay::Overlay;
use crate::instance::Inbox;
use crate::screens::strings::Extra;
use crate::ui::focus::HomeFocus;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;
use crate::ui::sound::Cue;

/// How many links may wait for the library; a flood of launches does not pile up.
const MAX_PENDING: usize = 8;

/// The links of a launcher: where they come from and which wait.
pub struct LinkState {
    inbox: Option<Inbox>,
    pending: Vec<Link>,
    scheme: String,
}

impl Default for LinkState {
    fn default() -> Self {
        Self {
            inbox: None,
            pending: Vec::new(),
            scheme: scheme(),
        }
    }
}

/// The scheme name of this run: `anyps5`, or the test scheme `ANYPS5_LINK_SCHEME` names (only
/// names that start with `anyps5`, so the variable cannot make the launcher answer foreign links).
pub fn scheme() -> String {
    match std::env::var("ANYPS5_LINK_SCHEME") {
        Ok(name) if name.starts_with(links::SCHEME) => name.to_ascii_lowercase(),
        _ => links::SCHEME.to_owned(),
    }
}

impl LauncherApp {
    /// Connects the launcher to later launches and hands it the arguments of its own start.
    pub fn attach_links(
        &mut self,
        ctx: &egui::Context,
        inbox: Option<Inbox>,
        arguments: &[String],
    ) {
        if let Some(inbox) = &inbox {
            inbox.attach(ctx);
        }
        self.links.inbox = inbox;
        self.accept(arguments, false);
    }

    /// Takes the arguments of a launch. Only the first one counts: a link that smuggles a quote
    /// into the registered command line would otherwise add arguments of its own.
    fn accept(&mut self, arguments: &[String], bring_forward: bool) {
        if bring_forward {
            self.model.effects.push(Effect::RestoreWindow);
        }
        let Some(argument) = arguments.first() else {
            return;
        };
        match links::parse_argument(argument, &self.links.scheme) {
            Ok(link) => {
                tracing::info!("link: {link:?}");
                if self.links.pending.len() < MAX_PENDING {
                    self.links.pending.push(link);
                }
            }
            Err(error) => {
                tracing::warn!("link: refused ({error})");
                let text = Extra::LinkInvalid.text(self.model.language());
                self.model.toast(text, Icon::WARNING);
            }
        }
    }

    /// Once per frame: takes what later launches sent and shows the links that can be shown.
    pub(super) fn poll_links(&mut self) {
        let launches = self
            .links
            .inbox
            .as_ref()
            .map(Inbox::drain)
            .unwrap_or_default();
        for arguments in launches {
            self.accept(&arguments, true);
        }
        while let Some(link) = self.links.pending.first() {
            if !self.link_ready(link) {
                break;
            }
            let link = self.links.pending.remove(0);
            self.show_link(link);
        }
    }

    /// Settings need the first-run setup done; the others also need the library to be read.
    fn link_ready(&self, link: &Link) -> bool {
        if !self.model.onboarded() {
            return false;
        }
        matches!(link, Link::Settings(_))
            || self.model.prefs.games_folder.is_none()
            || (!self.model.scanning && (self.offline || self.model.library_loaded))
    }

    fn show_link(&mut self, link: Link) {
        match link {
            Link::Game { title_id } => self.model.show_game(&title_id),
            Link::Unlock { title_id } => self.model.show_unlock(&title_id),
            Link::Settings(page) => self.model.show_settings(page),
            Link::Open { path } => match library::inspect(&path) {
                Some(game) => self.model.show_dump(&game),
                None => {
                    let text = Extra::LinkNotADump.text(self.model.language());
                    self.model.toast(text, Icon::WARNING);
                }
            },
        }
    }
}

impl Model {
    /// Brings the game with this title ID to the front of the home screen.
    pub fn show_game(&mut self, title_id: &str) {
        self.reveal_title(title_id);
    }

    /// Shows the game and points at its button. A locked game gets the hint to press Unlock; the
    /// press itself stays the player's.
    pub fn show_unlock(&mut self, title_id: &str) {
        let Some(game) = self.reveal_title(title_id) else {
            return;
        };
        let (extra, focus) = match self.state_of(&game) {
            crate::app::model::GameState::Locked => (Some(Extra::LinkUnlock), true),
            crate::app::model::GameState::Installed { .. } => (Some(Extra::LinkUnlocked), false),
            _ => (None, false),
        };
        if focus {
            self.focus = HomeFocus::Actions(0);
        }
        if let Some(extra) = extra {
            let text = extra.format(self.language(), &[Arg::Str(&game.name)]);
            self.toast(text, Icon::LOCK_OPEN);
        }
    }

    /// Opens the settings, on a page when one is named.
    pub fn show_settings(&mut self, page: Option<SettingsPage>) {
        self.settings_page = page;
        self.open(Overlay::Settings);
    }

    /// Shows a dump the player pointed at: it has to be in the library to be shown.
    pub fn show_dump(&mut self, dump: &Game) {
        let id = GameId::from_path(&dump.folder);
        if self.games.iter().any(|g| g.id == id) {
            self.reveal(&id);
            return;
        }
        let text = Extra::LinkOutside.format(self.language(), &[Arg::Str(&dump.name)]);
        self.toast(text, Icon::FOLDER_OPEN);
        self.show_settings(Some(SettingsPage::Library));
    }

    /// Selects the game with this title ID, or says that there is none.
    fn reveal_title(&mut self, title_id: &str) -> Option<Game> {
        let found = self
            .games
            .iter()
            .find(|g| g.title_id.eq_ignore_ascii_case(title_id))
            .cloned();
        match &found {
            Some(game) => self.reveal(&game.id),
            None => {
                let text = Extra::LinkNoGame.format(self.language(), &[Arg::Str(title_id)]);
                self.toast(text, Icon::WARNING);
            }
        }
        found
    }

    /// Selects a game on the home screen, closing what is open over it.
    fn reveal(&mut self, id: &GameId) {
        let Some(index) = self.games.iter().position(|g| &g.id == id) else {
            return;
        };
        self.overlay = None;
        self.focus = HomeFocus::Tiles;
        if self.selected != index {
            self.selected = index;
            self.effects.push(Effect::Sound(Cue::Move));
        }
        self.measure_selected();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyps5_core::library::ExecutableKind;
    use anyps5_core::paths::AppPaths;
    use anyps5_core::settings::Preferences;

    use super::*;
    use crate::app::pipeline::PipelineCommand;

    fn game(name: &str, title_id: &str, unlocked: bool) -> Game {
        let folder = PathBuf::from(format!("C:/games/{name}"));
        Game {
            id: GameId::from_path(&folder),
            folder,
            title_id: title_id.to_owned(),
            name: name.to_owned(),
            names: Default::default(),
            version: "01.000.000".to_owned(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: ExecutableKind::Elf,
            size: None,
            unlocked_with: unlocked.then(|| "v1".to_owned()),
        }
    }

    fn model() -> Model {
        let prefs = Preferences {
            onboarded: true,
            games_folder: Some(PathBuf::from("C:/games")),
            ..Preferences::default()
        };
        let mut model = Model::new(
            prefs,
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        );
        model.toolkit = Some("v1".to_owned());
        model.games = vec![
            game("Alpha", "PPSA00001", true),
            game("Beta", "PPSA00002", false),
        ];
        model
    }

    fn starts_something(model: &Model) -> bool {
        model
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Pipeline(_)))
            || !model.queue.is_empty()
            || model.starting.is_some()
    }

    #[test]
    fn a_game_link_selects_and_closes_what_is_open() {
        let mut m = model();
        m.open(Overlay::Settings);
        m.show_game("ppsa00002");
        assert_eq!(m.selected, 1);
        assert_eq!(m.overlay, None);
        assert!(!starts_something(&m));
    }

    #[test]
    fn an_unknown_title_only_says_so() {
        let mut m = model();
        m.show_game("PPSA99999");
        assert_eq!(m.selected, 0);
        assert!(
            m.toast
                .as_ref()
                .is_some_and(|t| t.text.contains("PPSA99999"))
        );
    }

    #[test]
    fn an_unlock_link_points_at_the_button_and_never_unlocks() {
        let mut m = model();
        m.show_unlock("PPSA00002");
        assert_eq!(m.selected, 1);
        assert_eq!(m.focus, HomeFocus::Actions(0));
        assert!(m.toast.as_ref().is_some_and(|t| t.text.contains("Beta")));
        assert!(!starts_something(&m));
        assert!(
            !m.effects
                .iter()
                .any(|e| matches!(e, Effect::Pipeline(PipelineCommand::Unlock(_))))
        );
    }

    #[test]
    fn an_unlock_link_for_an_unlocked_game_does_not_touch_it() {
        let mut m = model();
        m.show_unlock("PPSA00001");
        assert_eq!(m.focus, HomeFocus::Tiles);
        assert!(m.toast.as_ref().is_some_and(|t| t.text.contains("already")));
        assert!(!starts_something(&m));
    }

    #[test]
    fn settings_links_open_the_settings_on_their_page() {
        let mut m = model();
        m.show_settings(Some(SettingsPage::Toolkit));
        assert_eq!(m.overlay, Some(Overlay::Settings));
        assert_eq!(m.settings_page, Some(SettingsPage::Toolkit));
    }

    #[test]
    fn a_dump_outside_the_library_is_not_added() {
        let mut m = model();
        let outside = game("Gamma", "PPSA00003", false);
        let mut other = outside.clone();
        other.folder = PathBuf::from("D:/elsewhere/Gamma");
        m.show_dump(&other);
        assert_eq!(m.games.len(), 2);
        assert_eq!(m.settings_page, Some(SettingsPage::Library));
        m.show_dump(&m.games[1].clone());
        assert_eq!(m.selected, 1);
        assert!(!starts_something(&m));
    }
}
