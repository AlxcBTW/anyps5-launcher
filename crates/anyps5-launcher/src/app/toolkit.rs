//! The toolkit work behind the Toolkit category of the settings: asking GitHub for the latest
//! release, downloading it and adopting a folder. Everything runs on worker threads and reports
//! through one channel, so the UI thread never waits on the network or the disk.
//!
//! Simulated services (the demo and the tests) never touch the network or the disk.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use anyps5_core::events::CancelToken;
use anyps5_core::paths::AppPaths;
use anyps5_core::toolkit::{self, Toolkit, ToolkitError};
use egui::Context;

use crate::screens::settings::UpdateState;

/// What a worker reports.
#[derive(Debug)]
pub enum ToolkitEvent {
    /// The update check ended: `Ok(Some(tag))` a newer (or first) release exists.
    Checked(Result<Option<String>, String>),
    /// The download ended: the installed version.
    Installed(Result<String, String>),
    /// A folder was adopted: the folder and the version.
    Adopted(Result<(std::path::PathBuf, String), String>),
}

pub struct ToolkitService {
    tx: Sender<ToolkitEvent>,
    rx: Receiver<ToolkitEvent>,
    ctx: Context,
    simulated: bool,
    checking: bool,
    installing: bool,
    /// What the settings show.
    pub state: UpdateState,
    /// The latest release found by the last check, for "install".
    latest: Option<String>,
}

impl ToolkitService {
    pub fn new(ctx: &Context, simulated: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            ctx: ctx.clone(),
            simulated,
            checking: false,
            installing: false,
            state: UpdateState::Unknown,
            latest: None,
        }
    }

    pub fn installing(&self) -> bool {
        self.installing
    }

    /// Asks for the latest release. Ignored while a check or a download runs.
    pub fn check(&mut self, paths: &AppPaths, repository: &str) {
        if self.checking || self.installing {
            return;
        }
        self.checking = true;
        self.state = UpdateState::Checking;
        let (tx, ctx, paths, repo) = (
            self.tx.clone(),
            self.ctx.clone(),
            paths.clone(),
            repository.to_owned(),
        );
        let simulated = self.simulated;
        spawn("toolkit check", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(700));
                Ok(None)
            } else {
                check_latest(&paths, &repo)
            };
            let _ = tx.send(ToolkitEvent::Checked(result));
            ctx.request_repaint();
        });
    }

    /// Downloads and installs the latest release. False when a download already runs.
    pub fn install(&mut self, paths: &AppPaths, repository: &str) -> bool {
        if self.installing {
            return false;
        }
        self.installing = true;
        self.state = UpdateState::Downloading;
        let (tx, ctx, paths, repo) = (
            self.tx.clone(),
            self.ctx.clone(),
            paths.clone(),
            repository.to_owned(),
        );
        let simulated = self.simulated;
        spawn("toolkit install", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(900));
                Err("the demo does not download".to_owned())
            } else {
                install_latest(&paths, &repo)
            };
            let _ = tx.send(ToolkitEvent::Installed(result));
            ctx.request_repaint();
        });
        true
    }

    /// Checks a toolkit folder the player picked (it hashes the files, so not on the UI thread).
    pub fn adopt(&mut self, folder: std::path::PathBuf) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let simulated = self.simulated;
        spawn("toolkit adopt", move || {
            let result = if simulated {
                Err("the demo does not adopt folders".to_owned())
            } else {
                toolkit::adopt_folder(&folder)
                    .map(|t| (folder.clone(), t.version().to_owned()))
                    .map_err(describe)
            };
            let _ = tx.send(ToolkitEvent::Adopted(result));
            ctx.request_repaint();
        });
    }

    /// Finished work; also updates [`state`](Self::state).
    pub fn poll(&mut self) -> Vec<ToolkitEvent> {
        let events: Vec<ToolkitEvent> = self.rx.try_iter().collect();
        for event in &events {
            match event {
                ToolkitEvent::Checked(result) => {
                    self.checking = false;
                    self.state = match result {
                        Ok(Some(tag)) => {
                            self.latest = Some(tag.clone());
                            UpdateState::Available(tag.clone())
                        }
                        Ok(None) => UpdateState::UpToDate,
                        Err(_) => UpdateState::Failed,
                    };
                }
                ToolkitEvent::Adopted(_) => {}
                ToolkitEvent::Installed(result) => {
                    self.installing = false;
                    self.state = if result.is_ok() {
                        UpdateState::UpToDate
                    } else {
                        UpdateState::Failed
                    };
                }
            }
        }
        events
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}

fn describe(error: ToolkitError) -> String {
    error.to_string()
}

fn check_latest(paths: &AppPaths, repository: &str) -> Result<Option<String>, String> {
    let latest = toolkit::check_latest(repository).map_err(describe)?;
    let newer = match Toolkit::locate(paths, None) {
        Some(installed) => toolkit::update_available(&latest, &installed),
        None => true,
    };
    Ok(newer.then(|| latest.plain_tag().to_owned()))
}

fn install_latest(paths: &AppPaths, repository: &str) -> Result<String, String> {
    let info = toolkit::check_latest(repository).map_err(describe)?;
    let cancel = CancelToken::new();
    let installed = toolkit::download_and_install(&info, paths, None, &mut |_, _| {}, &cancel)
        .map_err(describe)?;
    Ok(installed.version().to_owned())
}
