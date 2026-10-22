//! Building blocks of the synthesised sounds (docs/DESIGN.md section 7.3): a random source, a
//! one-pole lowpass, a Freeverb reverb, and the soft tones the cues are made of. Everything is
//! plain arithmetic on `f32`, so the same code renders the cues once and runs the music on the
//! audio thread.

use std::f32::consts::{PI, TAU};

/// xorshift32. Deterministic, so renders and tests repeat.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `0..1`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Frequency of a MIDI note (A4 = 69 = 440 Hz).
pub fn midi_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Equal-power pan, `-1` left to `1` right, as (left, right) gains.
pub fn pan(position: f32) -> (f32, f32) {
    let angle = (position.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (angle.cos(), angle.sin())
}

/// One-pole lowpass (6 dB per octave).
#[derive(Clone, Debug, Default)]
pub struct Lowpass {
    a: f32,
    z: f32,
}

impl Lowpass {
    pub fn new(cutoff: f32, rate: f32) -> Self {
        let mut filter = Self::default();
        filter.set(cutoff, rate);
        filter
    }

    pub fn set(&mut self, cutoff: f32, rate: f32) {
        self.a = 1.0 - (-TAU * cutoff / rate).exp();
    }

    pub fn process(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }
}

/// Comb and allpass lengths of Freeverb at 44.1 kHz; the right channel is 23 samples longer.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

#[derive(Clone, Debug)]
struct Delay {
    buffer: Vec<f32>,
    at: usize,
}

impl Delay {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            at: 0,
        }
    }

    fn read(&self) -> f32 {
        self.buffer[self.at]
    }

    fn write_advance(&mut self, value: f32) {
        self.buffer[self.at] = value;
        self.at += 1;
        if self.at == self.buffer.len() {
            self.at = 0;
        }
    }
}

#[derive(Clone, Debug)]
struct Comb {
    delay: Delay,
    feedback: f32,
    damp: f32,
    store: f32,
}

#[derive(Clone, Debug)]
struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Delay>,
}

impl Channel {
    fn new(rate: f32, rt60: f32, damp: f32, extra: usize) -> Self {
        let scale = rate / 44_100.0;
        let combs = COMBS
            .iter()
            .map(|&length| {
                let length = ((length + extra) as f32 * scale).round() as usize;
                // Each comb loses 60 dB over rt60 seconds: g^(rt60 * rate / length) = 10^-3.
                let feedback = 10f32.powf(-3.0 * length as f32 / (rt60 * rate));
                Comb {
                    delay: Delay::new(length),
                    feedback,
                    damp,
                    store: 0.0,
                }
            })
            .collect();
        let allpasses = ALLPASSES
            .iter()
            .map(|&length| Delay::new(((length + extra) as f32 * scale).round() as usize))
            .collect();
        Self { combs, allpasses }
    }

    fn process(&mut self, x: f32) -> f32 {
        let mut sum = 0.0;
        for comb in &mut self.combs {
            let out = comb.delay.read();
            comb.store = out * (1.0 - comb.damp) + comb.store * comb.damp;
            comb.delay.write_advance(x + comb.store * comb.feedback);
            sum += out;
        }
        for allpass in &mut self.allpasses {
            let delayed = allpass.read();
            allpass.write_advance(sum + delayed * 0.5);
            sum = delayed - sum;
        }
        sum
    }
}

/// Freeverb (Jezar at Dreampoint, public domain): eight damped combs and four allpasses per
/// channel. Mono in, stereo wet out; the input gain of 0.015 and wet gain of 3 are Freeverb's.
#[derive(Clone, Debug)]
pub struct Reverb {
    left: Channel,
    right: Channel,
}

impl Reverb {
    /// `rt60` is the decay time to -60 dB in seconds, `damp` (0..1) darkens the tail.
    pub fn new(rate: f32, rt60: f32, damp: f32) -> Self {
        Self {
            left: Channel::new(rate, rt60, damp, 0),
            right: Channel::new(rate, rt60, damp, SPREAD),
        }
    }

    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let input = x * 0.015;
        (
            self.left.process(input) * 3.0,
            self.right.process(input) * 3.0,
        )
    }
}

/// `0..1` over `attack` seconds as a raised cosine, so no tone starts with a click.
fn attack(t: f32, attack: f32) -> f32 {
    if t >= attack {
        1.0
    } else {
        0.5 - 0.5 * (PI * t / attack).cos()
    }
}

/// The envelope every tone shares: a raised-cosine attack, then an exponential decay with time
/// constant `decay` that starts once the attack is over.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub attack: f32,
    pub decay: f32,
}

impl Shape {
    fn at(self, t: f32) -> f32 {
        attack(t, self.attack) * (-(t - self.attack).max(0.0) / self.decay).exp()
    }

    /// Seconds until the envelope has fallen by 80 dB.
    fn length(self) -> f32 {
        self.attack + self.decay * 9.2
    }
}

/// Samples from `start` for `seconds`, cut to the buffer.
fn span(out: &[f32], rate: f32, start: f32, seconds: f32) -> std::ops::Range<usize> {
    let first = (start * rate) as usize;
    let last = first + (seconds * rate) as usize;
    first.min(out.len())..last.min(out.len())
}

/// A soft tone: sine partials at `ratio * freq` with relative `gain`, all under one envelope.
/// With `detune` (in cents) every partial is played by two voices pulled apart by that much,
/// which makes it shimmer slowly instead of standing still.
#[allow(clippy::too_many_arguments)]
pub fn tone(
    out: &mut [f32],
    rate: f32,
    start: f32,
    freq: f32,
    amp: f32,
    shape: Shape,
    partials: &[(f32, f32)],
    detune: f32,
) {
    let first = (start * rate) as usize;
    let spread = 2f32.powf(detune / 2.0 / 1200.0);
    for i in span(out, rate, start, shape.length()) {
        let t = (i - first) as f32 / rate;
        let mut sample = 0.0;
        for &(ratio, gain) in partials {
            let f = freq * ratio;
            if f > rate * 0.45 {
                continue;
            }
            sample += if detune > 0.0 {
                0.5 * gain * ((TAU * f * spread * t).sin() + (TAU * f / spread * t).sin())
            } else {
                gain * (TAU * f * t).sin()
            };
        }
        out[i] += sample * shape.at(t) * amp;
    }
}

/// A faint high glitter for chimes: inharmonic sines between 3 and 8 kHz with random phases
/// that die away within `decay`.
pub fn shimmer(out: &mut [f32], rate: f32, start: f32, amp: f32, decay: f32, seed: u32) {
    const PARTIALS: [f32; 6] = [3150.0, 3820.0, 4480.0, 5260.0, 6310.0, 7880.0];
    let mut rng = Rng::new(seed);
    let phases: Vec<f32> = PARTIALS.iter().map(|_| rng.unit() * TAU).collect();
    let shape = Shape {
        attack: 0.004,
        decay,
    };
    let first = (start * rate) as usize;
    for i in span(out, rate, start, shape.length()) {
        let t = (i - first) as f32 / rate;
        let mut sample = 0.0;
        for (k, (&f, &phase)) in PARTIALS.iter().zip(&phases).enumerate() {
            if f > rate * 0.45 {
                continue;
            }
            // Higher partials fade faster.
            let fade = (-t * k as f32 / decay).exp();
            sample += (TAU * f * t + phase).sin() * fade / PARTIALS.len() as f32;
        }
        out[i] += sample * shape.at(t) * amp;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_tail_decays_by_its_rt60() {
        let rate = 48_000.0;
        let mut reverb = Reverb::new(rate, 1.0, 0.2);
        let energy = |from: usize, to: usize, reverb: &mut Reverb| {
            let mut sum = 0.0f64;
            for i in from..to {
                let (l, r) = reverb.process(if i == 0 { 1.0 } else { 0.0 });
                sum += f64::from(l * l + r * r);
            }
            sum
        };
        let early = energy(0, 24_000, &mut reverb);
        // Half a second later a 1 s tail has lost about 30 dB; damping only makes that faster.
        let late = energy(24_000, 48_000, &mut reverb);
        let drop_db = 10.0 * (early / late).log10();
        assert!(drop_db > 25.0 && drop_db < 45.0, "drop {drop_db} dB");
    }

    #[test]
    fn tones_start_at_zero_and_end_quiet() {
        let rate = 48_000.0;
        let shape = Shape {
            attack: 0.012,
            decay: 0.04,
        };
        let mut out = vec![0.0; 48_000];
        tone(
            &mut out,
            rate,
            0.0,
            349.0,
            1.0,
            shape,
            &[(1.0, 1.0), (2.0, 0.8)],
            0.0,
        );
        tone(&mut out, rate, 0.2, 523.0, 1.0, shape, &[(1.0, 1.0)], 6.0);
        shimmer(&mut out, rate, 0.4, 0.2, 0.05, 3);
        assert_eq!(out[0], 0.0);
        assert!(out[47_999].abs() < 1e-6);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    #[test]
    fn the_envelope_peaks_after_the_attack() {
        let shape = Shape {
            attack: 0.012,
            decay: 0.04,
        };
        assert_eq!(shape.at(0.0), 0.0);
        assert!((shape.at(0.012) - 1.0).abs() < 1e-6);
        assert!(shape.at(0.052) < 0.37 && shape.at(0.052) > 0.36);
    }

    #[test]
    fn pan_is_equal_power() {
        for position in [-1.0, -0.3, 0.0, 0.5, 1.0] {
            let (l, r) = pan(position);
            assert!((l * l + r * r - 1.0).abs() < 1e-5);
        }
        assert!(pan(-1.0).1.abs() < 1e-6);
    }

    #[test]
    fn rng_is_uniform_enough() {
        let mut rng = Rng::new(7);
        let mean = (0..10_000).map(|_| rng.unit()).sum::<f32>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02);
    }
}
