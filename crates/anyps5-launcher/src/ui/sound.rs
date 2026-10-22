//! Synthesised interface sounds (docs/DESIGN.md section 7.1, `Sounds.swift`): nothing is bundled.
//!
//! Four cues are rendered once into short stereo buffers at 44 100 Hz and played through four
//! voices used round-robin (a retrigger of the same voice cuts it). Master volume is 0.32. The
//! audio device is opened lazily on the first cue, so a muted launcher never touches it.

use std::num::NonZero;

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, buffer::SamplesBuffer};

pub const SAMPLE_RATE: u32 = 44_100;
pub const MASTER_VOLUME: f32 = 0.32;
const VOICES: usize = 4;

/// The four UI cues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Any selection or focus change, value change, binding captured.
    Move,
    /// Something opened, an action ran, advance in onboarding.
    Select,
    /// Closing, leaving, cancelling.
    Back,
    /// Unlock finished, onboarding finished, trophy banner.
    Unlocked,
}

impl Cue {
    pub const ALL: [Cue; 4] = [Cue::Move, Cue::Select, Cue::Back, Cue::Unlocked];

    /// Sine partials as (frequency Hz, start s, duration s), and the exponential decay constant.
    pub const fn recipe(self) -> (&'static [(f64, f64, f64)], f64) {
        match self {
            Cue::Move => (&[(1320.0, 0.0, 0.045)], 70.0),
            Cue::Select => (&[(880.0, 0.0, 0.07), (1320.0, 0.05, 0.10)], 32.0),
            Cue::Back => (&[(990.0, 0.0, 0.06), (660.0, 0.045, 0.09)], 34.0),
            Cue::Unlocked => (
                &[
                    (660.0, 0.0, 0.12),
                    (990.0, 0.09, 0.14),
                    (1320.0, 0.18, 0.30),
                ],
                12.0,
            ),
        }
    }
}

/// Renders one cue to mono samples (the two channels are identical), before master volume.
///
/// For every partial with `start <= t < start + duration` and `l = t - start`:
/// `sin(2 pi f l) * attack * exp(-l d) * 0.5 + sin(4 pi f l) * attack * exp(-l d 1.6) * 0.12`
/// with `attack = min(1, l / 0.004)`. The buffer is `max(start + duration) + 0.02` s long.
pub fn render_cue(cue: Cue) -> Vec<f32> {
    let (partials, decay) = cue.recipe();
    let rate = f64::from(SAMPLE_RATE);
    let length = partials
        .iter()
        .map(|(_, start, duration)| start + duration)
        .fold(0.0, f64::max);
    let frames = (rate * (length + 0.02)) as usize;
    (0..frames)
        .map(|frame| {
            let time = frame as f64 / rate;
            let mut sample = 0.0;
            for &(frequency, start, duration) in partials {
                if time >= start && time < start + duration {
                    let local = time - start;
                    let attack = (local / 0.004).min(1.0);
                    sample += (std::f64::consts::TAU * frequency * local).sin()
                        * attack
                        * (-local * decay).exp()
                        * 0.5;
                    sample += (2.0 * std::f64::consts::TAU * frequency * local).sin()
                        * attack
                        * (-local * decay * 1.6).exp()
                        * 0.12;
                }
            }
            sample as f32
        })
        .collect()
}

/// Stereo interleaved samples at master volume, as handed to the mixer.
fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter()
        .flat_map(|s| {
            let v = s * MASTER_VOLUME;
            [v, v]
        })
        .collect()
}

/// The sound engine. Keep one in the app; call [`Sounds::play`] from event handlers.
pub struct Sounds {
    /// "Interface Sounds" setting.
    pub enabled: bool,
    buffers: [Vec<f32>; 4],
    sink: Option<MixerDeviceSink>,
    voices: [Option<Player>; VOICES],
    next: usize,
    failed: bool,
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
            buffers: Cue::ALL.map(|cue| stereo(&render_cue(cue))),
            sink: None,
            voices: [None, None, None, None],
            next: 0,
            failed: false,
        }
    }

    /// Plays a cue unless sounds are off or no output device could be opened.
    pub fn play(&mut self, cue: Cue) {
        if !self.enabled || self.failed {
            return;
        }
        if self.sink.is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    self.sink = Some(sink);
                }
                Err(err) => {
                    tracing::warn!("no audio output, interface sounds stay off: {err}");
                    self.failed = true;
                    return;
                }
            }
        }
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let samples = self.buffers[cue as usize].clone();
        let (Some(channels), Some(rate)) = (NonZero::new(2u16), NonZero::new(SAMPLE_RATE)) else {
            return;
        };
        let player = Player::connect_new(sink.mixer());
        player.append(SamplesBuffer::new(channels, rate, samples));
        // Replacing the voice drops the old player, which cuts its sound.
        self.voices[self.next] = Some(player);
        self.next = (self.next + 1) % VOICES;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_lengths_follow_the_recipe() {
        // `AVAudioFrameCount(rate * (length + 0.02))` truncates; float noise may cost one frame.
        let near = |cue, secs: f64| {
            let want = (44_100.0 * (secs + 0.02)) as i64;
            (render_cue(cue).len() as i64 - want).abs() <= 1
        };
        assert!(near(Cue::Move, 0.045));
        assert!(near(Cue::Select, 0.15));
        assert!(near(Cue::Back, 0.135));
        assert!(near(Cue::Unlocked, 0.48));
    }

    #[test]
    fn samples_start_silent_and_stay_bounded() {
        for cue in Cue::ALL {
            let samples = render_cue(cue);
            assert_eq!(samples[0], 0.0, "{cue:?} starts at the zero crossing");
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.1 && peak <= 1.0, "{cue:?} peak {peak}");
        }
    }

    #[test]
    fn move_tick_matches_the_formula() {
        let samples = render_cue(Cue::Move);
        // t = 10 samples in: attack 10/44100/0.004, decay 70.
        let l = 10.0f64 / 44_100.0;
        let attack = l / 0.004;
        let want = (std::f64::consts::TAU * 1320.0 * l).sin() * attack * (-l * 70.0).exp() * 0.5
            + (2.0 * std::f64::consts::TAU * 1320.0 * l).sin()
                * attack
                * (-l * 70.0 * 1.6).exp()
                * 0.12;
        assert!((f64::from(samples[10]) - want).abs() < 1e-6);
        // After the partial ends only silence remains (the 0.02 s tail).
        assert_eq!(*samples.last().unwrap(), 0.0);
    }

    #[test]
    fn master_volume_is_applied_to_both_channels() {
        let out = stereo(&[1.0, -0.5]);
        assert_eq!(out, vec![0.32, 0.32, -0.16, -0.16]);
    }

    #[test]
    fn disabled_sounds_never_open_the_device() {
        let mut sounds = Sounds::new();
        sounds.enabled = false;
        sounds.play(Cue::Move);
        assert!(sounds.sink.is_none());
    }
}
