//! Synthesised interface sounds and home music (docs/DESIGN.md sections 7.1 and 7.3): nothing
//! is bundled.
//!
//! The first cue or the music opens the default output on a thread of its own, which renders the
//! five cues at the device rate and hands rodio one endless [`engine::Engine`]. After that a cue
//! or a music change is a message on a channel, so the UI thread never waits for audio. A
//! launcher with sounds and music off never touches the device.

mod cues;
mod engine;
mod music;
mod synth;

use std::sync::mpsc::{Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use rodio::DeviceSinkBuilder;

pub use self::cues::Cue;
use self::engine::{Command, Engine};

/// The sound engine. Keep one in the app; call [`Sounds::play`] from event handlers and
/// [`Sounds::set_music`] whenever the home music should be heard or not.
pub struct Sounds {
    /// "Interface Sounds" setting.
    pub enabled: bool,
    music: bool,
    /// Tests and the gallery never open the device, whatever the settings say.
    silent: bool,
    output: Option<Output>,
}

/// The link to the audio thread: commands for the engine, and a sender whose drop tells the
/// thread to close the device.
struct Output {
    commands: Sender<Command>,
    _alive: Sender<()>,
}

impl Default for Sounds {
    fn default() -> Self {
        Self::new()
    }
}

impl Sounds {
    pub fn new() -> Self {
        Self {
            enabled: true,
            music: false,
            silent: false,
            output: None,
        }
    }

    /// A sound engine that stays quiet: for tests, the gallery and screenshots.
    pub fn silent() -> Self {
        Self {
            enabled: false,
            silent: true,
            ..Self::new()
        }
    }

    /// Plays a cue unless sounds are off.
    pub fn play(&mut self, cue: Cue) {
        if self.enabled {
            self.send(Command::Cue(cue));
        }
    }

    /// Fades the home music in (`true`) or out. Cheap to call every frame.
    pub fn set_music(&mut self, on: bool) {
        if on == self.music {
            return;
        }
        self.music = on;
        if on || self.output.is_some() {
            self.send(Command::Music(on));
        }
    }

    fn send(&mut self, command: Command) {
        if self.silent {
            return;
        }
        let output = self.output.get_or_insert_with(Output::open);
        // A send fails only when no device could be opened; that was logged once.
        let _ = output.commands.send(command);
    }
}

impl Output {
    fn open() -> Self {
        let (commands, receiver) = channel();
        let (alive, until_dropped) = channel::<()>();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos());
        let spawned = std::thread::Builder::new()
            .name("sound".into())
            .spawn(move || {
                let mut sink = match DeviceSinkBuilder::open_default_sink() {
                    Ok(sink) => sink,
                    Err(err) => {
                        tracing::warn!("no audio output, sounds and music stay off: {err}");
                        return;
                    }
                };
                sink.log_on_drop(false);
                let rate = sink.config().sample_rate().get();
                sink.mixer().add(Engine::new(rate, receiver, seed));
                // Hold the device until the launcher drops its handle.
                let _ = until_dropped.recv();
            });
        if let Err(err) = spawned {
            tracing::warn!("cannot start the sound thread: {err}");
        }
        Self {
            commands,
            _alive: alive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_sounds_never_open_the_device() {
        let mut sounds = Sounds::new();
        sounds.enabled = false;
        sounds.play(Cue::Move);
        sounds.set_music(false);
        assert!(sounds.output.is_none());
    }

    #[test]
    fn a_silent_engine_ignores_everything() {
        let mut sounds = Sounds::silent();
        sounds.enabled = true;
        sounds.play(Cue::Select);
        sounds.set_music(true);
        assert!(sounds.output.is_none());
    }
}
