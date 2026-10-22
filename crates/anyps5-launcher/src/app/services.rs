//! The I/O behind the model's effects: library scan and watcher, size measuring, trophy loading and
//! the native folder dialog. Everything runs on worker threads and answers through one channel of
//! [`CoreEvent`]s (plus one for the dialog), so the UI thread never waits on the disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::events::{CancelToken, CoreEvent, Notifier};
use anyps5_core::library::{self, Game, LibraryWatcher};
use anyps5_core::paths::{self, GameId};
use anyps5_core::settings::{ConsoleLanguage, Preferences};
use anyps5_core::trophies::{Trophies, TrophyWatcher};
use egui::Context;

pub struct Services {
    tx: Sender<CoreEvent>,
    rx: Receiver<CoreEvent>,
    notify: Notifier,
    watcher: Option<LibraryWatcher>,
    watched: Option<PathBuf>,
    busy: Arc<AtomicBool>,
    folder: Option<Receiver<Option<PathBuf>>>,
    /// Polls the running game's trophy records (the banner and the live list).
    trophy_watch: Option<(GameId, TrophyWatcher)>,
}

impl Services {
    pub fn new(ctx: &Context) -> Self {
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        Self {
            tx,
            rx,
            notify: Arc::new(move || repaint.request_repaint()),
            watcher: None,
            watched: None,
            busy: Arc::new(AtomicBool::new(false)),
            folder: None,
            trophy_watch: None,
        }
    }

    /// Events the workers produced since the last call.
    pub fn drain(&mut self) -> Vec<CoreEvent> {
        self.rx.try_iter().collect()
    }

    /// Tells the watcher whether an unlock is queued or running (it stays silent then).
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::Relaxed);
    }

    /// Scans `root` on a worker thread and watches it.
    pub fn rescan(&mut self, root: &Path) {
        if self.watched.as_deref() != Some(root) {
            self.watcher = match LibraryWatcher::start(
                root.to_path_buf(),
                self.tx.clone(),
                self.notify.clone(),
                self.busy.clone(),
            ) {
                Ok(watcher) => Some(watcher),
                Err(error) => {
                    tracing::warn!("{error}");
                    None
                }
            };
            self.watched = Some(root.to_path_buf());
        }
        if let Some(watcher) = &self.watcher {
            watcher.mark_scanned();
        }
        let (tx, notify, root) = (self.tx.clone(), self.notify.clone(), root.to_path_buf());
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let games = library::scan(&root);
                let records: HashMap<GameId, String> = games
                    .iter()
                    .filter_map(|g| Some((g.id.clone(), g.unlocked_with.clone()?)))
                    .collect();
                let _ = tx.send(CoreEvent::LibraryScanned {
                    root,
                    games,
                    records,
                });
                notify();
            })
            .ok();
    }

    /// Measures the size on disk of a game (a game folder can hold tens of gigabytes in many files).
    pub fn measure(&self, game: &Game) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let (id, folder) = (game.id.clone(), game.folder.clone());
        std::thread::Builder::new()
            .name("size".into())
            .spawn(move || {
                let bytes = library::folder_size(&folder, &CancelToken::new());
                let _ = tx.send(CoreEvent::SizeMeasured { game: id, bytes });
                notify();
            })
            .ok();
    }

    /// Reads the trophies of a game in the language it runs in.
    pub fn load_trophies(&self, game: &Game, root: Option<&Path>, prefs: &Preferences) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let game = game.clone();
        let number = prefs
            .game_settings(&game.id)
            .language
            .unwrap_or(match prefs.language {
                anyps5_core::settings::Language::Turkish => 19,
                anyps5_core::settings::Language::English => 1,
            });
        let languages: Vec<String> = ConsoleLanguage::tag(number)
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        let copy = root.map(|root| paths::unlocked_copy_dir(root, &game.title_id, &game.id));
        std::thread::Builder::new()
            .name("trophies".into())
            .spawn(move || {
                let copy = copy.filter(|c| c.is_dir());
                let set = Trophies::load(&game, copy.as_deref(), &languages);
                let _ = tx.send(CoreEvent::Trophies {
                    game: game.id.clone(),
                    set,
                });
                notify();
            })
            .ok();
    }

    /// Starts watching the trophy records of a running game (one game at a time).
    pub fn watch_trophies(&mut self, game: &Game, root: Option<&Path>, prefs: &Preferences) {
        if self
            .trophy_watch
            .as_ref()
            .is_some_and(|(id, _)| *id == game.id)
        {
            return;
        }
        self.stop_trophy_watch();
        let Some(root) = root else { return };
        let copy = paths::unlocked_copy_dir(root, &game.title_id, &game.id);
        let number = prefs
            .game_settings(&game.id)
            .language
            .unwrap_or(match prefs.language {
                anyps5_core::settings::Language::Turkish => 19,
                anyps5_core::settings::Language::English => 1,
            });
        let languages: Vec<String> = ConsoleLanguage::tag(number)
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        let watcher = TrophyWatcher::start(
            game.clone(),
            copy,
            languages,
            self.tx.clone(),
            self.notify.clone(),
        );
        self.trophy_watch = Some((game.id.clone(), watcher));
    }

    /// Stops the trophy watch; the thread is joined off the UI thread.
    pub fn stop_trophy_watch(&mut self) {
        if let Some((_, watcher)) = self.trophy_watch.take() {
            std::thread::spawn(move || drop(watcher));
        }
    }

    /// Opens the native folder dialog on its own thread. False when one is already open.
    pub fn pick_folder(&mut self, title: &str, start: Option<&Path>, ctx: &Context) -> bool {
        if self.folder.is_some() {
            return false;
        }
        let (tx, rx) = channel();
        self.folder = Some(rx);
        let (title, start, ctx) = (title.to_owned(), start.map(Path::to_path_buf), ctx.clone());
        std::thread::Builder::new()
            .name("folder dialog".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title(&title);
                if let Some(start) = start.filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(start);
                }
                let _ = tx.send(dialog.pick_folder());
                ctx.request_repaint();
            })
            .ok();
        true
    }

    /// A native dialog is open: the launcher ignores input meanwhile.
    pub fn dialog_open(&self) -> bool {
        self.folder.is_some()
    }

    /// `Some(answer)` once the dialog closed (`None` inside: cancelled).
    pub fn poll_folder(&mut self) -> Option<Option<PathBuf>> {
        let rx = self.folder.as_ref()?;
        match rx.try_recv() {
            Ok(answer) => {
                self.folder = None;
                Some(answer)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.folder = None;
                Some(None)
            }
        }
    }
}
