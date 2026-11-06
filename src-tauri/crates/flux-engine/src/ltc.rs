//! LTC (SMPTE 12M) encoder: the 80-bit frame word and its biphase-mark audio signal.
//!
//! Pure functions and a small stateful encoder, with no I/O, so it is tested without hardware.

use flux_core::Clock;
use flux_core::Timecode;
use flux_project::FrameRate;

pub const BITS_PER_FRAME: usize = 80;

/// One LTC frame in transmission order: `bits[0]` is sent first.
pub type LtcBits = [bool; BITS_PER_FRAME];

/// Sync word, bits 64-79, written most significant bit first as it goes on the wire.
const SYNC_WORD: u16 = 0x3FFD;

const DROP_FRAME_BIT: usize = 10;
/// Keeps the number of transitions per frame even, so every frame starts with the same polarity.
/// It sits in bit 27, except at 25 fps where bit 27 is a binary group flag and bit 59 is used.
const POLARITY_BIT: usize = 27;
const POLARITY_BIT_25: usize = 59;

/// Builds the 80-bit word for `timecode`. `user_bits` are the eight user-bit nibbles, the least
/// significant nibble first on the wire.
pub fn frame_bits(timecode: Timecode, rate: FrameRate, user_bits: u32) -> LtcBits {
    let mut bits = [false; BITS_PER_FRAME];
    put(&mut bits, 0, 4, timecode.frames % 10);
    put(&mut bits, 8, 2, timecode.frames / 10);
    bits[DROP_FRAME_BIT] = rate.is_drop_frame();
    put(&mut bits, 16, 4, timecode.seconds % 10);
    put(&mut bits, 24, 3, timecode.seconds / 10);
    put(&mut bits, 32, 4, timecode.minutes % 10);
    put(&mut bits, 40, 3, timecode.minutes / 10);
    put(&mut bits, 48, 4, timecode.hours % 10);
    put(&mut bits, 56, 2, timecode.hours / 10);
    for nibble in 0..8 {
        // Truncation is the point: one nibble at a time.
        put(
            &mut bits,
            nibble * 8 + 4,
            4,
            (user_bits >> (4 * nibble)) as u8 & 0xF,
        );
    }
    for (i, bit) in bits[64..].iter_mut().enumerate() {
        *bit = SYNC_WORD & (0x8000 >> i) != 0;
    }

    // Even parity over the whole word.
    if bits.iter().filter(|&&bit| bit).count() % 2 == 1 {
        let polarity = if rate == FrameRate::Fps25 {
            POLARITY_BIT_25
        } else {
            POLARITY_BIT
        };
        bits[polarity] = true;
    }
    bits
}

/// Writes the low `width` bits of `value` at `position`, least significant bit first.
fn put(bits: &mut LtcBits, position: usize, width: usize, value: u8) {
    for i in 0..width {
        bits[position + i] = value >> i & 1 == 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LtcConfig {
    /// Sample rate of the audio device, usually 48 000.
    pub sample_rate: u32,
    /// Peak amplitude, linear, 0.0 to 1.0.
    pub level: f32,
    /// Samples an edge takes to swing from one polarity to the other; 0 gives a hard square wave.
    pub edge_samples: u32,
}

impl Default for LtcConfig {
    /// 48 kHz, half scale (-6 dBFS) and a one-sample edge: a clean signal that most readers accept.
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            level: 0.5,
            edge_samples: 1,
        }
    }
}

/// Turns LTC words into audio samples. It keeps the signal level and the fractional sample
/// position between frames, so consecutive frames join without clicks or drift.
#[derive(Debug, Clone)]
pub struct LtcEncoder {
    clock: Clock,
    /// Largest change of the output per sample.
    max_step: f32,
    /// Polarity the signal is heading to.
    target: f32,
    /// Last sample written.
    last: f32,
    /// Half bit cells emitted so far, to place the next boundary exactly.
    halves: u64,
}

impl LtcEncoder {
    pub fn new(config: LtcConfig, rate: FrameRate) -> Self {
        let level = config.level.clamp(0.0, 1.0);
        let max_step = if config.edge_samples == 0 {
            f32::INFINITY
        } else {
            2.0 * level / config.edge_samples as f32
        };
        Self {
            clock: Clock::new(config.sample_rate, rate),
            max_step,
            target: -level,
            last: -level,
            halves: 0,
        }
    }

    /// An upper bound for the samples of one frame, to reserve the output buffer up front: the
    /// audio callback must not allocate.
    pub fn max_frame_samples(&self) -> usize {
        self.clock.max_samples_per_frame()
    }

    /// Positions the encoder so the next frame it encodes is frame number `frame` of the
    /// timeline, starting exactly where the transport's clock puts that frame.
    pub fn start_at_frame(&mut self, frame: u64) {
        self.halves = frame * BITS_PER_FRAME as u64 * 2;
    }

    /// Appends the samples of one frame to `out`. The frame lasts `sample_rate / fps` samples,
    /// which is rarely a whole number: the remainder is carried into the next frame.
    pub fn encode_frame(&mut self, bits: &LtcBits, out: &mut Vec<f32>) {
        for &bit in bits {
            // Biphase mark: a transition at the start of every cell, and another in the middle of
            // a 1.
            self.flip();
            self.emit_half(out);
            if bit {
                self.flip();
            }
            self.emit_half(out);
        }
    }

    fn flip(&mut self) {
        self.target = -self.target;
    }

    fn emit_half(&mut self, out: &mut Vec<f32>) {
        let samples = self.boundary(self.halves + 1) - self.boundary(self.halves);
        self.halves += 1;
        for _ in 0..samples {
            let step = (self.target - self.last).clamp(-self.max_step, self.max_step);
            self.last += step;
            out.push(self.last);
        }
    }

    /// Sample index where half cell number `halves` starts, rounded to the nearest sample.
    fn boundary(&self, halves: u64) -> u64 {
        self.clock
            .ticks_to_samples(halves, 2 * BITS_PER_FRAME as u64)
    }
}

/// Plays LTC in step with the transport: silence until the next frame boundary, then one frame
/// after another. Everything is preallocated, so [`LtcStream::fill`] suits the audio callback.
#[derive(Debug, Clone)]
pub struct LtcStream {
    encoder: LtcEncoder,
    clock: Clock,
    rate: FrameRate,
    /// Frame count of the timeline origin.
    start: u64,
    user_bits: u32,
    /// Samples of the frame being played and how many have gone out.
    frame: Vec<f32>,
    played: usize,
    /// Timeline frame to encode next.
    next_frame: u64,
    /// Samples of silence left before the first frame.
    lead_in: u64,
    /// Samples the signal runs ahead of the transport: the timecode played at position `p` is the
    /// one of `p + offset`. Negative runs behind.
    offset: i64,
}

impl LtcStream {
    pub fn new(config: LtcConfig, rate: FrameRate, start: u64, user_bits: u32) -> Self {
        let encoder = LtcEncoder::new(config, rate);
        Self {
            frame: Vec::with_capacity(encoder.max_frame_samples()),
            clock: encoder.clock,
            encoder,
            rate,
            start,
            user_bits,
            played: 0,
            next_frame: 0,
            lead_in: 0,
            offset: 0,
        }
    }

    /// Plays the timecode `offset` samples ahead of the transport (behind when negative), to make
    /// up for the delay of the device.
    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    /// Restarts at transport position `position` (samples since the timeline origin), after a
    /// start or a seek. A frame cannot start halfway, so the output is silent until the next one.
    pub fn resync(&mut self, position: u64) {
        // Before the timeline origin there is nothing to play yet: wait it out in silence.
        let shifted = position as i64 + self.offset;
        let from = shifted.max(0) as u64;
        let mut frame = self.clock.samples_to_frames(from);
        if self.clock.frames_to_samples(frame) < from {
            frame += 1;
        }
        self.lead_in = self.clock.frames_to_samples(frame) - from + shifted.min(0).unsigned_abs();
        self.next_frame = frame;
        self.encoder.start_at_frame(frame);
        self.frame.clear();
        self.played = 0;
    }

    /// Writes the next `out.len()` samples of the signal.
    pub fn fill(&mut self, out: &mut [f32]) {
        for sample in out {
            *sample = self.next_sample();
        }
    }

    /// The next sample of the signal.
    pub fn next_sample(&mut self) -> f32 {
        if self.lead_in > 0 {
            self.lead_in -= 1;
            return 0.0;
        }
        if self.played == self.frame.len() {
            self.encode_next_frame();
        }
        let sample = self.frame[self.played];
        self.played += 1;
        sample
    }

    fn encode_next_frame(&mut self) {
        let timecode = Timecode::from_frames(self.start + self.next_frame, self.rate);
        let bits = frame_bits(timecode, self.rate, self.user_bits);
        self.frame.clear();
        self.encoder.encode_frame(&bits, &mut self.frame);
        self.played = 0;
        self.next_frame += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [FrameRate; 4] = [
        FrameRate::Fps24,
        FrameRate::Fps25,
        FrameRate::Fps2997Drop,
        FrameRate::Fps30,
    ];

    fn tc(hours: u8, minutes: u8, seconds: u8, frames: u8) -> Timecode {
        Timecode {
            hours,
            minutes,
            seconds,
            frames,
        }
    }

    fn field(bits: &LtcBits, position: usize, width: usize) -> u8 {
        (0..width).map(|i| u8::from(bits[position + i]) << i).sum()
    }

    /// Reads a word back into timecode, drop-frame flag and user bits.
    fn parse(bits: &LtcBits) -> (Timecode, bool, u32) {
        let timecode = tc(
            field(bits, 56, 2) * 10 + field(bits, 48, 4),
            field(bits, 40, 3) * 10 + field(bits, 32, 4),
            field(bits, 24, 3) * 10 + field(bits, 16, 4),
            field(bits, 8, 2) * 10 + field(bits, 0, 4),
        );
        let user = (0..8).fold(0u32, |user, n| {
            user | u32::from(field(bits, n * 8 + 4, 4)) << (4 * n)
        });
        (timecode, bits[DROP_FRAME_BIT], user)
    }

    fn config(sample_rate: u32, edge_samples: u32) -> LtcConfig {
        LtcConfig {
            sample_rate,
            level: 0.5,
            edge_samples,
        }
    }

    /// Reads biphase-mark audio back into bits, starting at a cell boundary.
    fn decode(samples: &[f32], sample_rate: u32, rate: FrameRate) -> Vec<bool> {
        let fps = match rate {
            FrameRate::Fps24 => 24.0,
            FrameRate::Fps25 => 25.0,
            FrameRate::Fps2997Drop => 30_000.0 / 1_001.0,
            FrameRate::Fps30 => 30.0,
        };
        let cell = f64::from(sample_rate) / (BITS_PER_FRAME as f64 * fps);

        // The encoder starts low and flips at once; a low sample in front makes that visible.
        // A flipped sample at the end closes the last half cell the same way.
        let closing = -samples.last().map_or(1.0, |last| last.signum());
        let lead = -samples.first().map_or(1.0, |first| first.signum());
        let signal: Vec<f32> = std::iter::once(lead)
            .chain(samples.iter().copied())
            .chain(std::iter::once(closing))
            .collect();
        let edges: Vec<usize> = (1..signal.len())
            .filter(|&i| (signal[i] > 0.0) != (signal[i - 1] > 0.0))
            .collect();
        let gap = |from: usize| (edges[from + 1] - edges[from]) as f64;
        let near = |value: f64, target: f64| (value - target).abs() < cell / 4.0;

        let mut bits = Vec::new();
        let mut at = 0;
        while at + 1 < edges.len() {
            if near(gap(at), cell) {
                bits.push(false);
                at += 1;
            } else if at + 2 < edges.len()
                && near(gap(at), cell / 2.0)
                && near(gap(at + 1), cell / 2.0)
            {
                bits.push(true);
                at += 2;
            } else {
                panic!("bad edge spacing at transition {at}: {} samples", gap(at));
            }
        }
        bits
    }

    fn encode(
        rate: FrameRate,
        config: LtcConfig,
        start: u64,
        count: u64,
    ) -> (Vec<f32>, Vec<LtcBits>) {
        let mut encoder = LtcEncoder::new(config, rate);
        let mut out = Vec::new();
        let mut words = Vec::new();
        for frame in start..start + count {
            let bits = frame_bits(Timecode::from_frames(frame, rate), rate, 0);
            encoder.encode_frame(&bits, &mut out);
            words.push(bits);
        }
        (out, words)
    }

    #[test]
    fn frame_bits_should_place_each_bcd_digit_least_significant_bit_first() {
        let bits = frame_bits(tc(12, 34, 56, 18), FrameRate::Fps30, 0);
        assert_eq!(field(&bits, 0, 4), 8);
        assert_eq!(field(&bits, 8, 2), 1);
        assert_eq!(field(&bits, 16, 4), 6);
        assert_eq!(field(&bits, 24, 3), 5);
        assert_eq!(field(&bits, 32, 4), 4);
        assert_eq!(field(&bits, 40, 3), 3);
        assert_eq!(field(&bits, 48, 4), 2);
        assert_eq!(field(&bits, 56, 2), 1);
    }

    #[test]
    fn frame_bits_should_end_with_the_sync_word() {
        let bits = frame_bits(tc(0, 0, 0, 0), FrameRate::Fps25, 0);
        let expected = [
            false, false, true, true, true, true, true, true, true, true, true, true, true, true,
            false, true,
        ];
        assert_eq!(bits[64..], expected);
    }

    #[test]
    fn frame_bits_should_flag_drop_frame_only_for_29_97() {
        for rate in RATES {
            let bits = frame_bits(tc(0, 1, 0, 2), rate, 0);
            assert_eq!(
                bits[DROP_FRAME_BIT],
                rate == FrameRate::Fps2997Drop,
                "{rate:?}"
            );
        }
    }

    #[test]
    fn frame_bits_should_carry_user_bits() {
        let bits = frame_bits(tc(1, 2, 3, 4), FrameRate::Fps25, 0x1234_ABCD);
        assert_eq!(parse(&bits).2, 0x1234_ABCD);
    }

    #[test]
    fn frame_bits_should_have_an_even_number_of_ones_for_every_frame_of_an_hour() {
        for rate in RATES {
            for frame in 0..rate.frames_per_hour() {
                let bits = frame_bits(Timecode::from_frames(frame, rate), rate, 0);
                assert_eq!(
                    bits.iter().filter(|&&b| b).count() % 2,
                    0,
                    "{rate:?} {frame}"
                );
            }
        }
    }

    #[test]
    fn frame_bits_should_correct_polarity_in_bit_27_or_bit_59_at_25_fps() {
        // 00:00:00:00 leaves only the 13 ones of the sync word: odd, so it needs the correction.
        let thirty = frame_bits(tc(0, 0, 0, 0), FrameRate::Fps30, 0);
        let twenty_five = frame_bits(tc(0, 0, 0, 0), FrameRate::Fps25, 0);
        assert!(thirty[POLARITY_BIT] && !thirty[POLARITY_BIT_25]);
        assert!(twenty_five[POLARITY_BIT_25] && !twenty_five[POLARITY_BIT]);
        // One more set bit makes the count even already.
        let even = frame_bits(tc(0, 0, 0, 1), FrameRate::Fps30, 0);
        assert!(!even[POLARITY_BIT]);
    }

    #[test]
    fn encoded_audio_should_decode_back_to_the_same_frames() {
        for rate in RATES {
            for sample_rate in [44_100, 48_000, 96_000] {
                for edge_samples in [0, 2] {
                    // Crossing the minute boundary exercises the drop-frame skip.
                    let start = rate.frames_per_hour() - 5;
                    let (samples, words) =
                        encode(rate, config(sample_rate, edge_samples), start, 12);
                    let bits = decode(&samples, sample_rate, rate);
                    let decoded: Vec<_> = bits
                        .chunks_exact(BITS_PER_FRAME)
                        .map(|chunk| <LtcBits>::try_from(chunk).unwrap())
                        .collect();
                    assert_eq!(
                        decoded, words,
                        "{rate:?} {sample_rate} Hz edge {edge_samples}"
                    );
                }
            }
        }
    }

    #[test]
    fn drop_frame_minute_boundary_should_skip_frames_0_and_1() {
        let rate = FrameRate::Fps2997Drop;
        let (samples, _) = encode(rate, config(48_000, 0), 1_798, 4);
        let bits = decode(&samples, 48_000, rate);
        let seen: Vec<_> = bits
            .chunks_exact(BITS_PER_FRAME)
            .map(|chunk| parse(&<LtcBits>::try_from(chunk).unwrap()).0.to_string())
            .collect();
        assert_eq!(
            seen,
            ["00:00:59:28", "00:00:59:29", "00:01:00:02", "00:01:00:03"]
        );
    }

    #[test]
    fn frames_should_not_drift_when_the_sample_count_per_frame_is_fractional() {
        // 1000 frames at 29.97 fps last exactly 1 601 600 samples at 48 kHz.
        let (samples, _) = encode(FrameRate::Fps2997Drop, config(48_000, 0), 0, 1_000);
        assert_eq!(samples.len(), 1_601_600);
        // 24 fps at 44.1 kHz is 1837.5 samples per frame.
        let (samples, _) = encode(FrameRate::Fps24, config(44_100, 0), 0, 24);
        assert_eq!(samples.len(), 44_100);
    }

    #[test]
    fn every_frame_should_start_with_the_same_polarity() {
        let rate = FrameRate::Fps30;
        let mut encoder = LtcEncoder::new(config(48_000, 0), rate);
        let mut out = Vec::new();
        for frame in 0..200 {
            out.clear();
            let bits = frame_bits(Timecode::from_frames(frame, rate), rate, 0);
            encoder.encode_frame(&bits, &mut out);
            // The first cell flips to the high level.
            assert!(out[0] > 0.0, "frame {frame} started low");
        }
    }

    #[test]
    fn level_should_set_the_peak_amplitude() {
        let (samples, _) = encode(FrameRate::Fps25, config(48_000, 0), 0, 4);
        let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert_eq!(peak, 0.5);
    }

    #[test]
    fn edge_shaping_should_limit_the_change_between_samples() {
        let (samples, _) = encode(FrameRate::Fps25, config(48_000, 4), 0, 4);
        let steepest = samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            steepest <= 2.0 * 0.5 / 4.0 + f32::EPSILON,
            "steepest step {steepest}"
        );
    }

    #[test]
    fn max_frame_samples_should_cover_every_frame() {
        let rate = FrameRate::Fps2997Drop;
        let mut encoder = LtcEncoder::new(config(48_000, 0), rate);
        let bound = encoder.max_frame_samples();
        let mut out = Vec::with_capacity(bound);
        for frame in 0..100 {
            out.clear();
            let bits = frame_bits(Timecode::from_frames(frame, rate), rate, 0);
            encoder.encode_frame(&bits, &mut out);
            assert!(out.len() <= bound);
            assert_eq!(out.capacity(), bound, "the buffer reallocated");
        }
    }

    /// Fills the stream with chunks of uneven sizes, like successive audio callbacks.
    fn fill_in_chunks(stream: &mut LtcStream, total: usize) -> Vec<f32> {
        let mut out = vec![0.0; total];
        let mut at = 0;
        for size in [64, 333, 480, 1, 1_024].into_iter().cycle() {
            if at >= total {
                break;
            }
            let end = (at + size).min(total);
            stream.fill(&mut out[at..end]);
            at = end;
        }
        out
    }

    #[test]
    fn stream_should_match_the_encoder_regardless_of_chunk_size() {
        for rate in RATES {
            let cfg = config(48_000, 1);
            let (expected, _) = encode(rate, cfg, 0, 20);
            let mut stream = LtcStream::new(cfg, rate, 0, 0);
            stream.resync(0);
            let actual = fill_in_chunks(&mut stream, expected.len());
            assert_eq!(actual, expected, "{rate:?}");
        }
    }

    #[test]
    fn stream_should_be_silent_until_the_next_frame_boundary() {
        let rate = FrameRate::Fps25;
        let mut stream = LtcStream::new(config(48_000, 0), rate, 0, 0);
        // Frame 1 starts at sample 1920.
        stream.resync(1_000);
        let out = fill_in_chunks(&mut stream, 920 + 200);
        assert!(out[..920].iter().all(|&s| s == 0.0));
        assert!(out[920..].iter().any(|&s| s != 0.0));
        assert_ne!(out[920], 0.0, "the frame must start on the boundary");
    }

    #[test]
    fn stream_should_start_a_frame_at_once_when_resynced_on_a_boundary() {
        let rate = FrameRate::Fps2997Drop;
        let mut stream = LtcStream::new(config(48_000, 0), rate, 0, 0);
        let clock = Clock::new(48_000, rate);
        stream.resync(clock.frames_to_samples(7));
        let mut first = [0.0; 4];
        stream.fill(&mut first);
        assert!(first.iter().all(|&s| s != 0.0));
    }

    #[test]
    fn stream_should_decode_to_the_timecode_it_was_resynced_to() {
        for rate in RATES {
            let clock = Clock::new(48_000, rate);
            let start = 90_000;
            let mut stream = LtcStream::new(config(48_000, 0), rate, start, 0);
            // Play a little, then seek to frame 500 of the timeline.
            stream.resync(0);
            fill_in_chunks(&mut stream, 5_000);
            stream.resync(clock.frames_to_samples(500));
            let samples = fill_in_chunks(&mut stream, clock.frames_to_samples(10) as usize);

            let bits = decode(&samples, 48_000, rate);
            let decoded: Vec<_> = bits
                .chunks_exact(BITS_PER_FRAME)
                .map(|chunk| parse(&<LtcBits>::try_from(chunk).unwrap()).0)
                .collect();
            let expected: Vec<_> = (0..decoded.len() as u64)
                .map(|i| Timecode::from_frames(start + 500 + i, rate))
                .collect();
            assert!(decoded.len() >= 9, "{rate:?}: {} frames", decoded.len());
            assert_eq!(decoded, expected, "{rate:?}");
        }
    }

    #[test]
    fn encoder_boundaries_should_agree_with_the_transport_clock() {
        for rate in RATES {
            let clock = Clock::new(48_000, rate);
            let encoder = LtcEncoder::new(config(48_000, 0), rate);
            for frame in 0..500 {
                assert_eq!(
                    encoder.boundary(frame * 2 * BITS_PER_FRAME as u64),
                    clock.frames_to_samples(frame)
                );
            }
        }
    }
    fn signal(mut stream: LtcStream, position: u64, len: usize) -> Vec<f32> {
        stream.resync(position);
        let mut out = vec![0.0; len];
        stream.fill(&mut out);
        out
    }

    #[test]
    fn offset_should_play_the_timecode_of_the_shifted_position() {
        let rate = FrameRate::Fps25;
        let plain = || LtcStream::new(config(48_000, 0), rate, 0, 0);
        // A positive offset runs ahead: at position 10 000 it plays what 10 000 + 960 would.
        assert_eq!(
            signal(plain().with_offset(960), 10_000, 8_000),
            signal(plain(), 10_960, 8_000)
        );
        // A negative one runs behind.
        assert_eq!(
            signal(plain().with_offset(-960), 10_000, 8_000),
            signal(plain(), 9_040, 8_000)
        );
    }

    #[test]
    fn offset_should_wait_in_silence_until_the_timeline_origin() {
        let rate = FrameRate::Fps25;
        let plain = || LtcStream::new(config(48_000, 0), rate, 0, 0);
        // Position 100 with -500 is 400 samples before the origin: 400 silent samples, then frame 0.
        let late = signal(plain().with_offset(-500), 100, 4_000);
        let origin = signal(plain(), 0, 4_000 - 400);
        assert!(late[..400].iter().all(|&s| s == 0.0));
        assert_eq!(late[400..], origin[..]);
    }

    #[test]
    fn no_offset_should_play_as_before() {
        let rate = FrameRate::Fps25;
        let plain = || LtcStream::new(config(48_000, 0), rate, 0, 0);
        assert_eq!(
            signal(plain().with_offset(0), 12_345, 8_000),
            signal(plain(), 12_345, 8_000)
        );
    }
}
