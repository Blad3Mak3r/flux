use super::decode::{Decoded, ENGINE_RATE};

/// Waveform resolution sent to the UI: one min/max pair every 10 ms.
pub const PEAKS_PER_SECOND: u32 = 100;

const FRAMES_PER_PEAK: usize = (ENGINE_RATE / PEAKS_PER_SECOND) as usize;

/// Min and max of every 10 ms window, as interleaved signed bytes (`-128..=127`, full scale at the
/// ends): `[min0, max0, min1, max1, ...]`. Both channels are folded into one trace.
pub fn peaks(decoded: &Decoded) -> Vec<i8> {
    decoded
        .samples
        .chunks(FRAMES_PER_PEAK * 2)
        .flat_map(|window| {
            let min = window.iter().copied().min().unwrap_or(0);
            let max = window.iter().copied().max().unwrap_or(0);
            [(min >> 8) as i8, (max >> 8) as i8]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(frames: usize, value: i16) -> Decoded {
        Decoded {
            samples: vec![value; frames * 2],
            source_rate: ENGINE_RATE,
        }
    }

    #[test]
    fn peaks_should_give_one_pair_per_10_ms() {
        let one_second = stereo(48_000, 0);
        assert_eq!(peaks(&one_second).len(), 100 * 2);
    }

    #[test]
    fn peaks_should_round_a_partial_window_up() {
        let ten_and_a_bit_ms = stereo(FRAMES_PER_PEAK + 1, 0);
        assert_eq!(peaks(&ten_and_a_bit_ms).len(), 2 * 2);
    }

    #[test]
    fn peaks_of_silence_should_be_zero() {
        assert!(peaks(&stereo(4_800, 0)).iter().all(|&p| p == 0));
    }

    #[test]
    fn peaks_should_report_the_extremes_of_each_window() {
        let mut decoded = stereo(FRAMES_PER_PEAK * 2, 0);
        // Window 0: left channel dips to full scale negative; window 1: right channel peaks.
        decoded.samples[10] = i16::MIN;
        decoded.samples[FRAMES_PER_PEAK * 2 + 5] = i16::MAX;

        let peaks = peaks(&decoded);

        assert_eq!(peaks, [-128, 0, 0, 127]);
    }

    #[test]
    fn peaks_should_fold_both_channels_into_one_trace() {
        let mut decoded = stereo(FRAMES_PER_PEAK, 0);
        decoded.samples[0] = 8_192; // left +0.25
        decoded.samples[1] = -16_384; // right -0.5

        assert_eq!(peaks(&decoded), [-64, 32]);
    }
}
