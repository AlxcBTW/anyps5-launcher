//! Home screen music (docs/DESIGN.md section 7.3): an endless ambient piece generated while it
//! plays. A deep drone on F carries slow chords in F minor with its dreamy neighbours (A flat,
//! G flat, D flat); soft single notes from the current chord float above them. Every chord
//! lasts eight seconds and fades into the next; after two rounds of the eight chords one quieter
//! round follows without the drone, then it starts again.

use std::f32::consts::TAU;

use super::synth::{Lowpass, Reverb, Rng, db_to_gain, midi_hz, pan};

/// Pad voicings as MIDI notes, eight seconds each.
const CHORDS: [[u8; 5]; 8] = [
    [41, 48, 53, 58, 60], // F sus4: F2 C3 F3 B flat3 C4
    [41, 48, 56, 60, 67], // F minor add 9: F2 C3 A flat3 C4 G4
    [44, 51, 55, 60, 63], // A flat maj7: A flat2 E flat3 G3 C4 E flat4
    [42, 49, 58, 61, 65], // G flat maj7: G flat2 D flat3 B flat3 D flat4 F4
    [41, 48, 58, 60, 65], // F sus4, higher: F2 C3 B flat3 C4 F4
    [41, 48, 57, 60, 67], // F add 9: F2 C3 A3 C4 G4
    [37, 44, 53, 60, 63], // D flat maj9: D flat2 A flat2 F3 C4 E flat4
    [42, 49, 54, 56, 61], // G flat add 9: G flat2 D flat3 G flat3 A flat3 D flat4
];
const CHORD_SECONDS: f32 = 8.0;
const ATTACK_SECONDS: f32 = 3.5;
const RELEASE_SECONDS: f32 = 4.5;
/// Rounds of [`CHORDS`] with the drone, then one round without.
const ROUNDS_WITH_DRONE: usize = 2;
/// The drone: F1 and F2.
const DRONE_HZ: f32 = 43.65;
/// Parameters are refreshed every this many frames.
const BLOCK: u32 = 64;

/// Overall level, chosen so the piece sits around -28 dBFS RMS (a quiet bed under the cues).
const GAIN_DB: f32 = -9.0;
const PAD_GAIN: f32 = 0.07;
const DRONE_GAIN: f32 = 0.06;
const NOTE_GAIN: f32 = 0.11;
const REVERB_SEND: f32 = 0.55;

/// A sawtooth with PolyBLEP corners (no aliasing worth hearing).
#[derive(Clone, Debug)]
struct Saw {
    phase: f32,
    step: f32,
}

impl Saw {
    fn new(freq: f32, rate: f32, phase: f32) -> Self {
        Self {
            phase,
            step: freq / rate,
        }
    }

    fn next(&mut self) -> f32 {
        let t = self.phase;
        let dt = self.step;
        let mut value = 2.0 * t - 1.0;
        if t < dt {
            let x = t / dt;
            value -= x + x - x * x - 1.0;
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            value -= x * x + x + x + 1.0;
        }
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }
}

/// One pad note: two sawtooths seven cents apart, one leaning left and one right.
#[derive(Clone, Debug)]
struct PadNote {
    saws: [Saw; 2],
    /// Frame the note started and, once its chord is over, the frame it began to fade.
    born: u64,
    released: Option<u64>,
    level: f32,
}

/// A soft note above the pad: a sine with a quiet octave, a 10 ms attack and a long decay.
#[derive(Clone, Debug)]
struct Note {
    freq: f32,
    age: u32,
    /// `exp(-t / 0.9)`, stepped once per frame instead of evaluated.
    decay: f32,
    amp: f32,
    pan: (f32, f32),
}

/// The generator. [`Music::next`] returns one stereo frame.
pub struct Music {
    rate: f32,
    frame: u64,
    chord: usize,
    round: usize,
    next_chord: u64,
    pad: Vec<PadNote>,
    notes: Vec<Note>,
    next_note: u64,
    drone_phase: f32,
    drone_level: f32,
    /// The drone's slow swell (about 23 s), refreshed every block.
    breathe: f32,
    /// Per-frame factor of a note's decay, and the gains that need a `powf` to find.
    note_decay: f32,
    out_gain: f32,
    quiet_gain: f32,
    filters: [[Lowpass; 2]; 2],
    reverb: Reverb,
    rng: Rng,
}

impl Music {
    pub fn new(rate: u32, seed: u32) -> Self {
        let rate = rate as f32;
        let mut music = Self {
            rate,
            frame: 0,
            chord: 0,
            round: 0,
            next_chord: (rate * CHORD_SECONDS) as u64,
            pad: Vec::new(),
            notes: Vec::new(),
            next_note: (rate * 4.0) as u64,
            drone_phase: 0.0,
            drone_level: 0.0,
            breathe: 1.0,
            note_decay: (-1.0 / (0.9 * rate)).exp(),
            out_gain: db_to_gain(GAIN_DB),
            quiet_gain: db_to_gain(-3.0),
            filters: Default::default(),
            reverb: Reverb::new(rate, 5.0, 0.6),
            rng: Rng::new(seed),
        };
        music.start_chord();
        music
    }

    fn with_drone(&self) -> bool {
        self.round < ROUNDS_WITH_DRONE
    }

    fn start_chord(&mut self) {
        for note in &mut self.pad {
            note.released.get_or_insert(self.frame);
        }
        for &midi in &CHORDS[self.chord] {
            let freq = midi_hz(f32::from(midi));
            let spread = 2f32.powf(3.5 / 1200.0);
            self.pad.push(PadNote {
                saws: [
                    Saw::new(freq * spread, self.rate, self.rng.unit()),
                    Saw::new(freq / spread, self.rate, self.rng.unit()),
                ],
                born: self.frame,
                released: None,
                level: 0.0,
            });
        }
    }

    /// Envelopes, filter sweep, chord changes and new notes; called every [`BLOCK`] frames.
    fn update(&mut self) {
        let rate = self.rate;
        let seconds = self.frame as f32 / rate;
        if self.frame >= self.next_chord {
            self.next_chord += (CHORD_SECONDS * rate) as u64;
            self.chord = (self.chord + 1) % CHORDS.len();
            if self.chord == 0 {
                self.round = (self.round + 1) % (ROUNDS_WITH_DRONE + 1);
            }
            self.start_chord();
        }
        let frame = self.frame;
        for note in &mut self.pad {
            let attack = ((frame - note.born) as f32 / (ATTACK_SECONDS * rate)).min(1.0);
            let release = note.released.map_or(1.0, |at| {
                1.0 - ((frame - at) as f32 / (RELEASE_SECONDS * rate)).min(1.0)
            });
            // Equal-power curves, so a crossfade between chords keeps the level.
            note.level = (attack * TAU / 4.0).sin() * (release * TAU / 4.0).sin();
        }
        self.pad.retain(|n| {
            n.released
                .is_none_or(|at| frame - at < (RELEASE_SECONDS * rate) as u64)
        });

        self.breathe = 1.0 + 0.15 * (TAU * seconds / 23.0).sin();
        let drone_target = if self.with_drone() { 1.0 } else { 0.0 };
        let drone_step = BLOCK as f32 / (6.0 * rate);
        self.drone_level += (drone_target - self.drone_level).clamp(-drone_step, drone_step);

        // The lowpass breathes between about 520 and 880 Hz over 25 s.
        let cutoff = 700.0 + 180.0 * (TAU * seconds / 25.0).sin();
        for channel in &mut self.filters {
            for filter in channel {
                filter.set(cutoff, rate);
            }
        }

        if frame >= self.next_note {
            self.play_note();
            let wait = self.rng.range(1.4, 3.6);
            self.next_note = frame + (wait * rate) as u64;
        }
        self.notes.retain(|n| (n.age as f32) < 4.0 * rate);
    }

    /// A chord tone between F4 and C6, now and then with a second one a moment later.
    fn play_note(&mut self) {
        let chord = CHORDS[self.chord];
        let class = chord[(self.rng.next_u32() as usize) % chord.len()] % 12;
        let mut midi = 65 + (i32::from(class) - 65).rem_euclid(12);
        if self.rng.unit() < 0.35 {
            midi += 12;
        }
        let quiet = if self.with_drone() { 1.0 } else { 0.8 };
        self.notes.push(Note {
            freq: midi_hz(midi as f32),
            age: 0,
            decay: 1.0,
            amp: NOTE_GAIN * quiet * self.rng.range(0.55, 1.0),
            pan: pan(self.rng.range(-0.45, 0.45)),
        });
    }

    pub fn next(&mut self) -> (f32, f32) {
        if self.frame.is_multiple_of(u64::from(BLOCK)) {
            self.update();
        }
        let rate = self.rate;

        let (mut left, mut right) = (0.0, 0.0);
        for note in &mut self.pad {
            let a = note.saws[0].next() * note.level;
            let b = note.saws[1].next() * note.level;
            left += a * 0.75 + b * 0.25;
            right += a * 0.25 + b * 0.75;
        }
        let quiet = if self.with_drone() {
            1.0
        } else {
            self.quiet_gain
        };
        let [[l1, l2], [r1, r2]] = &mut self.filters;
        left = l2.process(l1.process(left)) * PAD_GAIN * quiet;
        right = r2.process(r1.process(right)) * PAD_GAIN * quiet;

        self.drone_phase += DRONE_HZ / rate;
        self.drone_phase -= self.drone_phase.floor();
        let p = TAU * self.drone_phase;
        let drone =
            (p.sin() + 0.3 * (2.0 * p).sin()) * DRONE_GAIN * self.drone_level * self.breathe;
        left += drone;
        right += drone;

        let (mut notes_l, mut notes_r) = (0.0, 0.0);
        for note in &mut self.notes {
            let t = note.age as f32 / rate;
            let attack = (t / 0.01).min(1.0);
            let envelope = attack * attack * note.decay;
            let q = TAU * note.freq * t;
            let s = (q.sin() + 0.2 * (2.0 * q).sin()) * envelope * note.amp;
            notes_l += s * note.pan.0;
            notes_r += s * note.pan.1;
            note.age += 1;
            note.decay *= self.note_decay;
        }

        let (wet_l, wet_r) = self
            .reverb
            .process((left + right) * 0.5 * REVERB_SEND + (notes_l + notes_r) * 0.9);
        self.frame += 1;
        let gain = self.out_gain;
        (
            (left + notes_l * 0.6 + wet_l) * gain,
            (right + notes_r * 0.6 + wet_r) * gain,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(seconds: f32, rate: u32) -> Vec<(f32, f32)> {
        let mut music = Music::new(rate, 1);
        (0..(seconds * rate as f32) as usize)
            .map(|_| music.next())
            .collect()
    }

    #[test]
    fn starts_silent_and_stays_bounded() {
        let frames = render(12.0, 24_000);
        assert!(frames[0].0.abs() < 1e-4 && frames[0].1.abs() < 1e-4);
        let peak = frames
            .iter()
            .fold(0.0f32, |m, (l, r)| m.max(l.abs()).max(r.abs()));
        assert!(peak < db_to_gain(-6.0), "peak {peak}");
    }

    #[test]
    fn sits_well_below_the_cues() {
        let frames = render(20.0, 24_000);
        // After the opening swell.
        let body = &frames[8 * 24_000..];
        let rms =
            (body.iter().map(|(l, r)| l * l + r * r).sum::<f32>() / (2 * body.len()) as f32).sqrt();
        let db = 20.0 * rms.log10();
        assert!((-34.0..-22.0).contains(&db), "rms {db} dBFS");
    }

    #[test]
    fn chords_never_pile_up() {
        let mut music = Music::new(24_000, 1);
        for _ in 0..(30 * 24_000) {
            music.next();
            assert!(music.pad.len() <= 10, "{} pad notes", music.pad.len());
        }
    }
}
