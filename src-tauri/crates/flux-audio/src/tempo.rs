//! Tempo detection: the BPM of a piece of audio, from where its sound starts.
//!
//! 1. The loudness is measured every [`HOP`] samples (about 10 ms) and its rises are kept: that
//!    is the onset strength, large where a drum or a note starts.
//! 2. The autocorrelation of the onset strength says which spacing between onsets repeats. Tempos
//!    a musician would count (around 120 BPM) are favoured, which settles the "is it 70 or 140?"
//!    ambiguity the way a listener would.
//! 3. The winning tempo is refined by sliding a comb of equally spaced beats over the onsets at
//!    finer steps, since the 10 ms of the first step is a few tenths of a BPM.
//!
//! It finds the pulse of music that has one. A recording with a free or changing tempo, or none,
//! gives a number or nothing, and the user can correct the result.

use super::decode::{Decoded, ENGINE_RATE};

/// Samples per onset-strength value.
const HOP: usize = 512;
/// The slowest and fastest tempo searched.
const MIN_BPM: f64 = 55.0;
const MAX_BPM: f64 = 210.0;
/// Where a listener most often taps; tempos are weighed by their distance from it, in octaves.
const PREFERRED_BPM: f64 = 120.0;
const PREFERENCE_OCTAVES: f64 = 1.4;
/// Longest stretch analysed, from the middle of the audio, where the tempo is steadiest.
const MAX_SECONDS: usize = 120;
/// Shortest audio a tempo is read from: a few bars.
const MIN_SECONDS: usize = 4;
/// The correlation at the winning lag, relative to the correlation at lag 0, below which the
/// onsets show no steady pulse.
const MIN_STRENGTH: f64 = 0.04;
/// How far the refinement looks around the first estimate.
const REFINE_SPAN: f64 = 0.03;
/// Width of the blur that turns each onset into a bump a few hops wide (standard deviation, in
/// hops). Without it two onsets a hop apart would not correlate at all, and the search would only
/// find a tempo whose beat happens to fall on a whole number of hops.
const BLUR_HOPS: f64 = 1.5;
/// The loudest rise, in log-loudness units, below which the audio has no onsets worth reading: a
/// steady tone only wobbles by thousandths.
const MIN_ONSET: f64 = 0.1;

/// The tempo of `audio` in BPM, to two decimals, or `None` when there is too little of it, it is
/// silent, or it shows no steady pulse.
pub fn detect_bpm(audio: &Decoded) -> Option<f64> {
    let frames = audio.frames();
    let rate = ENGINE_RATE as usize;
    if frames < MIN_SECONDS * rate {
        return None;
    }
    let length = frames.min(MAX_SECONDS * rate);
    let from = (frames - length) / 2;
    let mono: Vec<f32> = audio.samples[from * 2..(from + length) * 2]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (f32::from(pair[0]) + f32::from(pair[1])) * (0.5 / 32_768.0))
        .collect();
    bpm_of(&onset_strength(&mono))
}

/// How much louder each stretch of `HOP` samples is than the one before, on a log scale (so a
/// quiet verse counts as much as a loud chorus), with the slow average taken out.
fn onset_strength(mono: &[f32]) -> Vec<f64> {
    let loudness: Vec<f64> = mono
        .chunks(HOP)
        .map(|hop| {
            let energy = hop
                .iter()
                .map(|&s| f64::from(s) * f64::from(s))
                .sum::<f64>();
            (1.0 + 1_000.0 * (energy / hop.len() as f64).sqrt()).ln()
        })
        .collect();
    let rises: Vec<f64> = std::iter::once(0.0)
        .chain(loudness.windows(2).map(|pair| (pair[1] - pair[0]).max(0.0)))
        .collect();
    // Subtract the average over about a second, so a loud passage does not look like a pulse.
    let window = hops_per_second() as usize;
    let mut out = Vec::with_capacity(rises.len());
    let mut sum = 0.0;
    for (at, &rise) in rises.iter().enumerate() {
        sum += rise;
        if at >= window {
            sum -= rises[at - window];
        }
        let mean = sum / window.min(at + 1) as f64;
        out.push((rise - mean).max(0.0));
    }
    out
}

fn hops_per_second() -> f64 {
    f64::from(ENGINE_RATE) / HOP as f64
}

/// Spreads every value over its neighbours with a Gaussian of [`BLUR_HOPS`].
fn blur(values: &[f64]) -> Vec<f64> {
    let radius = (BLUR_HOPS * 3.0).ceil() as isize;
    let kernel: Vec<f64> = (-radius..=radius)
        .map(|k| (-0.5 * (k as f64 / BLUR_HOPS).powi(2)).exp())
        .collect();
    (0..values.len() as isize)
        .map(|at| {
            kernel
                .iter()
                .zip(at - radius..=at + radius)
                .filter_map(|(weight, from)| {
                    values.get(usize::try_from(from).ok()?).map(|v| v * weight)
                })
                .sum()
        })
        .collect()
}

/// The tempo the onset strength points to.
fn bpm_of(onsets: &[f64]) -> Option<f64> {
    if onsets.iter().copied().fold(0.0, f64::max) < MIN_ONSET {
        return None;
    }
    let onsets = &blur(onsets);
    let rate = hops_per_second();
    let lag_of = |bpm: f64| 60.0 / bpm * rate;
    let (shortest, longest) = (lag_of(MAX_BPM), lag_of(MIN_BPM));
    if onsets.len() < (longest * 4.0) as usize {
        return None;
    }
    let zero = correlation(onsets, 0.0);
    if zero <= 0.0 {
        return None;
    }

    // Lags in steps of a quarter hop. A pulse at lag L also repeats at 2L, which counts for it.
    let mut best: Option<(f64, f64)> = None;
    let mut lag = shortest;
    while lag <= longest {
        let bpm = 60.0 * rate / lag;
        let octaves = (bpm / PREFERRED_BPM).log2() / PREFERENCE_OCTAVES;
        let preference = (-0.5 * octaves * octaves).exp();
        let strength = correlation(onsets, lag) + 0.5 * correlation(onsets, 2.0 * lag);
        let score = strength * preference;
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((lag, score));
        }
        lag += 0.25;
    }
    let (lag, _) = best?;
    if correlation(onsets, lag) / zero < MIN_STRENGTH {
        return None;
    }
    let bpm = refine(onsets, 60.0 * rate / lag);
    Some((bpm * 100.0).round() / 100.0)
}

/// The mean of `onsets[i] * onsets[i + lag]`, with the lag between two hops interpolated.
fn correlation(onsets: &[f64], lag: f64) -> f64 {
    let whole = lag.floor() as usize;
    let fraction = lag - whole as f64;
    let at = |shift: usize| {
        onsets[..onsets.len() - shift]
            .iter()
            .zip(&onsets[shift..])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            / (onsets.len() - shift) as f64
    };
    if fraction == 0.0 {
        at(whole)
    } else {
        at(whole) * (1.0 - fraction) + at(whole + 1) * fraction
    }
}

/// Slides a comb of beats, `60 / bpm` apart, over the onsets for tempos close to `bpm`, and keeps
/// the one that lands on the most onset strength, wherever its first beat falls.
fn refine(onsets: &[f64], bpm: f64) -> f64 {
    let rate = hops_per_second();
    let value = |position: f64| {
        let at = position.floor() as usize;
        match (onsets.get(at), onsets.get(at + 1)) {
            (Some(a), Some(b)) => a + (b - a) * (position - at as f64),
            (Some(a), None) => *a,
            _ => 0.0,
        }
    };
    let steps = (REFINE_SPAN / 0.001) as i32;
    let mut best = (bpm, f64::MIN);
    for step in -steps..=steps {
        let candidate = bpm * (1.0 + f64::from(step) * 0.001);
        let period = 60.0 / candidate * rate;
        let mut phase = 0.0;
        while phase < period {
            let (mut sum, mut count) = (0.0, 0);
            let mut position = phase;
            while position < onsets.len() as f64 - 1.0 {
                sum += value(position);
                count += 1;
                position += period;
            }
            let score = sum / f64::from(count.max(1));
            if score > best.1 {
                best = (candidate, score);
            }
            phase += 0.5;
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audio with a short burst of noise at every beat of `bpm`, plus quieter ones between the
    /// beats, and a steady tone under it all.
    fn drums(bpm: f64, seconds: usize, offbeat: f32) -> Decoded {
        let rate = ENGINE_RATE as usize;
        let mut samples = vec![0i16; seconds * rate * 2];
        let beat = 60.0 / bpm * rate as f64;
        let mut seed = 12_345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) as f32 / 32_768.0 - 1.0
        };
        let mut hit = |samples: &mut Vec<i16>, start: usize, level: f32| {
            for n in 0..(rate / 20) {
                let at = (start + n) * 2;
                if at + 1 >= samples.len() {
                    break;
                }
                let value = noise() * level * (-(n as f32) / 400.0).exp() * 20_000.0;
                samples[at] = samples[at].saturating_add(value as i16);
                samples[at + 1] = samples[at + 1].saturating_add(value as i16);
            }
        };
        let mut index = 0.0f64;
        while (index * beat) < (seconds * rate) as f64 {
            hit(&mut samples, (index * beat) as usize, 1.0);
            hit(&mut samples, ((index + 0.5) * beat) as usize, offbeat);
            index += 1.0;
        }
        Decoded {
            samples,
            source_rate: ENGINE_RATE,
        }
    }

    fn assert_near(found: Option<f64>, expected: f64, tolerance: f64) {
        let found = found.expect("a tempo was expected");
        assert!(
            (found - expected).abs() <= tolerance,
            "found {found}, expected {expected}"
        );
    }

    #[test]
    fn should_find_the_tempo_of_a_drum_pattern() {
        for bpm in [70.0, 90.0, 100.0, 120.0, 128.0, 140.0, 160.0] {
            assert_near(detect_bpm(&drums(bpm, 30, 0.0)), bpm, 0.6);
        }
    }

    #[test]
    fn should_not_mistake_the_offbeats_for_the_beat() {
        // Quieter hits halfway between the beats double the onsets without doubling the tempo.
        for bpm in [96.0, 124.0] {
            assert_near(detect_bpm(&drums(bpm, 30, 0.4)), bpm, 0.6);
        }
    }

    #[test]
    fn should_settle_an_even_pulse_on_the_tempo_nearest_to_where_listeners_tap() {
        // Evenly spaced hits at 174 BPM are as much 87 BPM with every other beat left out; nothing
        // in the audio can tell, so the tempo nearer to 120 wins. A drummer's accents would.
        assert_near(detect_bpm(&drums(174.0, 30, 0.0)), 87.0, 0.6);
    }

    #[test]
    fn should_find_a_fractional_tempo() {
        assert_near(detect_bpm(&drums(127.5, 40, 0.0)), 127.5, 0.4);
    }

    #[test]
    fn should_read_a_tempo_from_the_middle_of_a_long_file() {
        assert_near(detect_bpm(&drums(110.0, 200, 0.0)), 110.0, 0.6);
    }

    #[test]
    fn should_find_nothing_in_silence() {
        let silent = Decoded {
            samples: vec![0; ENGINE_RATE as usize * 2 * 10],
            source_rate: ENGINE_RATE,
        };
        assert_eq!(detect_bpm(&silent), None);
    }

    #[test]
    fn should_find_nothing_in_audio_that_is_too_short() {
        assert_eq!(detect_bpm(&drums(120.0, 2, 0.0)), None);
    }

    #[test]
    fn should_find_nothing_in_a_steady_tone() {
        let tone = (0..ENGINE_RATE as usize * 10)
            .flat_map(|n| {
                let phase = std::f64::consts::TAU * 220.0 * n as f64 / f64::from(ENGINE_RATE);
                let value = (phase.sin() * 8_000.0) as i16;
                [value, value]
            })
            .collect();
        let tone = Decoded {
            samples: tone,
            source_rate: ENGINE_RATE,
        };
        assert_eq!(detect_bpm(&tone), None);
    }
}
