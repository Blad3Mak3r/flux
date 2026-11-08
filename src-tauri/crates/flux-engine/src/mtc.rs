//! MTC (MIDI Timecode) encoder: quarter-frame messages and the full-frame SysEx.
//!
//! Pure functions that build the bytes; when to send them is the transport's job.

use flux_core::Clock;
use flux_core::Timecode;
use flux_project::FrameRate;

/// Status byte of a quarter-frame message.
const QUARTER_FRAME: u8 = 0xF1;

/// Rate code carried in the quarter frame of piece 7 and in the full-frame hours byte.
fn rate_code(rate: FrameRate) -> u8 {
    match rate {
        FrameRate::Fps24 => 0,
        FrameRate::Fps25 => 1,
        FrameRate::Fps2997Drop => 2,
        FrameRate::Fps30 => 3,
    }
}

/// The eight quarter-frame messages that describe `timecode`, in the order they are sent.
///
/// The group describes the frame at which piece 0 is sent. Pieces go out four per frame, so
/// the group takes two frames and a receiver adds two frames once it has all eight. Piece 0
/// must therefore go out exactly when the transport reaches `timecode`, and the following pieces
/// every quarter of a frame after that.
pub fn quarter_frames(timecode: Timecode, rate: FrameRate) -> [[u8; 2]; 8] {
    let nibbles = [
        timecode.frames & 0xF,
        timecode.frames >> 4,
        timecode.seconds & 0xF,
        timecode.seconds >> 4,
        timecode.minutes & 0xF,
        timecode.minutes >> 4,
        timecode.hours & 0xF,
        // Bit 4 of the hours plus the rate code in bits 1-2.
        (timecode.hours >> 4) | rate_code(rate) << 1,
    ];
    let mut pieces = [[0; 2]; 8];
    for (piece, (message, nibble)) in (0u8..).zip(pieces.iter_mut().zip(nibbles)) {
        *message = [QUARTER_FRAME, piece << 4 | nibble];
    }
    pieces
}

/// The full-frame SysEx (`F0 7F 7F 01 01 hh mm ss ff F7`), sent on start and after a seek so
/// receivers jump to the position without waiting for eight quarter frames.
pub fn full_frame(timecode: Timecode, rate: FrameRate) -> [u8; 10] {
    [
        0xF0,
        0x7F,
        0x7F,
        0x01,
        0x01,
        rate_code(rate) << 5 | timecode.hours,
        timecode.minutes,
        timecode.seconds,
        timecode.frames,
        0xF7,
    ]
}

/// Quarter frames per frame, and per group of eight.
const QUARTERS_PER_FRAME: u64 = 4;
const QUARTERS_PER_GROUP: u64 = 8;

/// Decides when each quarter frame goes out, from the transport position, so MTC runs on the same
/// sample clock as the audio. It only produces bytes: the caller sends them to a MIDI port.
#[derive(Debug, Clone)]
pub struct MtcScheduler {
    clock: Clock,
    rate: FrameRate,
    /// Frame count of the timeline origin.
    start: u64,
    /// Quarter frame to send next, counted from the timeline origin.
    next_quarter: u64,
    /// Samples the timecode runs ahead of the transport: what goes out at position `p` is the
    /// timecode of `p + offset`. Negative runs behind.
    offset: i64,
}

impl MtcScheduler {
    pub fn new(clock: Clock, rate: FrameRate, start: u64) -> Self {
        Self {
            clock,
            rate,
            start,
            next_quarter: 0,
            offset: 0,
        }
    }

    pub fn set_timecode(&mut self, clock: Clock, rate: FrameRate, start: u64) {
        self.clock = clock;
        self.rate = rate;
        self.start = start;
    }

    /// Sends the timecode `offset` samples ahead of the transport (behind when negative), to make
    /// up for the delay of the port and the receiver.
    pub fn with_offset(mut self, offset: i64) -> Self {
        self.offset = offset;
        self
    }

    /// Restarts at transport position `position` (samples since the timeline origin), after a
    /// start or a seek. Returns the full-frame message to send now; quarter frames resume at the
    /// next group of eight, because a receiver needs a group to begin at piece 0.
    pub fn resync(&mut self, position: u64) -> [u8; 10] {
        let position = (position as i64 + self.offset).max(0) as u64;
        let mut quarter = self.clock.samples_to_ticks(position, QUARTERS_PER_FRAME);
        if self.clock.ticks_to_samples(quarter, QUARTERS_PER_FRAME) < position {
            quarter += 1;
        }
        self.next_quarter = quarter.next_multiple_of(QUARTERS_PER_GROUP);

        let frame = self.start + self.clock.samples_to_frames(position);
        full_frame(Timecode::from_frames(frame, self.rate), self.rate)
    }

    /// The transport position (samples since the timeline origin) at which the next quarter frame
    /// is due, so a caller can wait for it instead of polling.
    pub fn next_due(&self) -> u64 {
        let due = self
            .clock
            .ticks_to_samples(self.next_quarter, QUARTERS_PER_FRAME);
        (due as i64 - self.offset).max(0) as u64
    }

    /// Calls `send` for every quarter frame whose time has come at transport position
    /// `position`, in order. A caller that falls far behind skips ahead to the next group rather
    /// than flooding the port with stale messages.
    pub fn poll(&mut self, position: u64, mut send: impl FnMut([u8; 2])) {
        let position = position as i64 + self.offset;
        let due = self
            .clock
            .samples_to_ticks(position.max(0) as u64, QUARTERS_PER_FRAME);
        if due > self.next_quarter + QUARTERS_PER_GROUP {
            self.next_quarter = due.next_multiple_of(QUARTERS_PER_GROUP);
        }
        while self
            .clock
            .ticks_to_samples(self.next_quarter, QUARTERS_PER_FRAME) as i64
            <= position
        {
            let piece = (self.next_quarter % QUARTERS_PER_GROUP) as usize;
            // A group describes the frame where its piece 0 goes out.
            let group_frame = self.start + self.next_quarter / QUARTERS_PER_GROUP * 2;
            let timecode = Timecode::from_frames(group_frame, self.rate);
            send(quarter_frames(timecode, self.rate)[piece]);
            self.next_quarter += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(hours: u8, minutes: u8, seconds: u8, frames: u8) -> Timecode {
        Timecode {
            hours,
            minutes,
            seconds,
            frames,
        }
    }

    #[test]
    fn quarter_frames_should_split_01_02_03_04_at_25_fps_into_eight_pieces() {
        let pieces = quarter_frames(tc(1, 2, 3, 4), FrameRate::Fps25);
        let bytes: Vec<u8> = pieces.iter().flatten().copied().collect();
        assert_eq!(
            bytes,
            [
                0xF1, 0x04, // frames, low nibble
                0xF1, 0x10, // frames, high nibble
                0xF1, 0x23, // seconds, low nibble
                0xF1, 0x30, // seconds, high nibble
                0xF1, 0x42, // minutes, low nibble
                0xF1, 0x50, // minutes, high nibble
                0xF1, 0x61, // hours, low nibble
                0xF1, 0x72, // hours, high bit and the 25 fps rate code
            ]
        );
    }

    #[test]
    fn quarter_frames_should_use_both_nibbles_of_two_digit_fields() {
        let pieces = quarter_frames(tc(23, 59, 58, 29), FrameRate::Fps30);
        let data: Vec<u8> = pieces.iter().map(|message| message[1]).collect();
        assert_eq!(data, [0x0D, 0x11, 0x2A, 0x33, 0x4B, 0x53, 0x67, 0x77]);
    }

    #[test]
    fn quarter_frames_should_carry_the_rate_code_in_piece_7() {
        let code = |rate| quarter_frames(tc(0, 0, 0, 0), rate)[7][1] >> 1 & 0b11;
        assert_eq!(code(FrameRate::Fps24), 0);
        assert_eq!(code(FrameRate::Fps25), 1);
        assert_eq!(code(FrameRate::Fps2997Drop), 2);
        assert_eq!(code(FrameRate::Fps30), 3);
    }

    #[test]
    fn quarter_frames_should_number_the_pieces_in_the_high_nibble() {
        let pieces = quarter_frames(tc(0, 0, 0, 0), FrameRate::Fps24);
        for (piece, message) in pieces.iter().enumerate() {
            assert_eq!(message[0], 0xF1);
            assert_eq!(usize::from(message[1] >> 4), piece);
        }
    }

    #[test]
    fn quarter_frames_should_stay_in_the_7_bit_midi_data_range() {
        for rate in [FrameRate::Fps24, FrameRate::Fps2997Drop, FrameRate::Fps30] {
            for message in quarter_frames(tc(23, 59, 59, 29), rate) {
                assert!(message[1] < 0x80);
            }
        }
    }

    /// 25 fps at 48 kHz: a frame is 1920 samples and a quarter frame 480.
    fn scheduler(start: u64) -> MtcScheduler {
        let rate = FrameRate::Fps25;
        MtcScheduler::new(Clock::new(48_000, rate), rate, start)
    }

    fn poll(scheduler: &mut MtcScheduler, position: u64) -> Vec<[u8; 2]> {
        let mut sent = Vec::new();
        scheduler.poll(position, |message| sent.push(message));
        sent
    }

    #[test]
    fn scheduler_should_send_a_quarter_frame_every_quarter_of_a_frame() {
        let mut s = scheduler(0);
        s.resync(0);
        let first = quarter_frames(tc(0, 0, 0, 0), FrameRate::Fps25);
        assert_eq!(poll(&mut s, 0), [first[0]]);
        assert_eq!(poll(&mut s, 479), Vec::<[u8; 2]>::new());
        assert_eq!(poll(&mut s, 480), [first[1]]);
        assert_eq!(poll(&mut s, 3_839), first[2..]);
        assert_eq!(poll(&mut s, 3_839), Vec::<[u8; 2]>::new());
    }

    #[test]
    fn scheduler_should_start_each_group_at_its_own_frame() {
        let mut s = scheduler(0);
        s.resync(0);
        poll(&mut s, 3_839);
        // The next group begins two frames in and describes 00:00:00:02.
        let second = quarter_frames(tc(0, 0, 0, 2), FrameRate::Fps25);
        assert_eq!(poll(&mut s, 3_840), [second[0]]);
    }

    #[test]
    fn scheduler_should_add_the_start_offset_to_the_timecode() {
        // 01:00:00:00 at 25 fps.
        let mut s = scheduler(90_000);
        let full = s.resync(0);
        assert_eq!(full[5..9], [0x21, 0x00, 0x00, 0x00]);
        let sent = poll(&mut s, 0);
        assert_eq!(sent, [quarter_frames(tc(1, 0, 0, 0), FrameRate::Fps25)[0]]);
    }

    #[test]
    fn next_due_should_follow_the_quarter_frames_as_they_are_sent() {
        let mut s = scheduler(0);
        s.resync(0);
        assert_eq!(s.next_due(), 0);
        poll(&mut s, 0);
        assert_eq!(s.next_due(), 480);
        poll(&mut s, 1_000);
        assert_eq!(s.next_due(), 1_440);
    }

    #[test]
    fn resync_should_return_a_full_frame_for_the_position_and_wait_for_the_next_group() {
        let mut s = scheduler(0);
        // Frame 35 is 00:00:01:10. Groups start on even frames, so the next one is frame 36.
        let full = s.resync(35 * 1_920);
        assert_eq!(full[5..9], [0x20, 0x00, 0x01, 0x0A]);
        assert_eq!(poll(&mut s, 35 * 1_920 + 100), Vec::<[u8; 2]>::new());
        let group = quarter_frames(tc(0, 0, 1, 11), FrameRate::Fps25);
        assert_eq!(poll(&mut s, 36 * 1_920), [group[0]]);
    }

    #[test]
    fn resync_mid_quarter_should_not_resend_the_quarter_in_progress() {
        let mut s = scheduler(0);
        s.resync(100);
        // The first group starts at frame 0 + 2 = sample 3840, not at the past sample 0.
        assert_eq!(poll(&mut s, 3_839), Vec::<[u8; 2]>::new());
        assert_eq!(poll(&mut s, 3_840).len(), 1);
    }

    #[test]
    fn poll_should_not_send_again_without_progress() {
        let mut s = scheduler(0);
        s.resync(0);
        assert_eq!(poll(&mut s, 960).len(), 3);
        assert!(poll(&mut s, 960).is_empty());
    }

    #[test]
    fn poll_should_skip_ahead_after_a_long_stall() {
        let mut s = scheduler(0);
        s.resync(0);
        // Ten seconds late: no backlog of stale pieces, and it resumes at piece 0.
        let sent = poll(&mut s, 480_000);
        assert!(sent.len() <= 1, "{} messages", sent.len());
        for message in sent {
            assert_eq!(message[1] >> 4, 0);
        }
    }

    #[test]
    fn full_frame_should_be_the_universal_sysex_with_the_rate_in_the_hours_byte() {
        assert_eq!(
            full_frame(tc(1, 2, 3, 4), FrameRate::Fps25),
            [0xF0, 0x7F, 0x7F, 0x01, 0x01, 0x21, 0x02, 0x03, 0x04, 0xF7]
        );
        assert_eq!(
            full_frame(tc(23, 59, 58, 29), FrameRate::Fps2997Drop),
            [0xF0, 0x7F, 0x7F, 0x01, 0x01, 0x57, 0x3B, 0x3A, 0x1D, 0xF7]
        );
    }
    #[test]
    fn positive_offset_should_send_each_quarter_frame_earlier() {
        // +480 samples is a quarter frame ahead. From the origin the next group starts at frame 2,
        // 3 840 samples in, so it is due 480 samples sooner.
        let mut s = scheduler(0).with_offset(480);
        s.resync(0);
        assert_eq!(s.next_due(), 3_360);
        let second = quarter_frames(tc(0, 0, 0, 2), FrameRate::Fps25);
        assert_eq!(poll(&mut s, 3_359), Vec::<[u8; 2]>::new());
        assert_eq!(poll(&mut s, 3_360), [second[0]]);
    }

    #[test]
    fn negative_offset_should_hold_the_first_quarter_frame_back() {
        let mut s = scheduler(0).with_offset(-480);
        let full = s.resync(0);
        assert_eq!(full[5..9], [0x20, 0x00, 0x00, 0x00]);
        assert_eq!(s.next_due(), 480);
        let first = quarter_frames(tc(0, 0, 0, 0), FrameRate::Fps25);
        assert_eq!(poll(&mut s, 479), Vec::<[u8; 2]>::new());
        assert_eq!(poll(&mut s, 480), [first[0]]);
    }

    #[test]
    fn offset_should_shift_the_full_frame_after_a_seek() {
        // 35 frames in, 2 frames ahead: the receiver is told frame 37, 00:00:01:12.
        let mut s = scheduler(0).with_offset(2 * 1_920);
        let full = s.resync(35 * 1_920);
        assert_eq!(full[5..9], [0x20, 0x00, 0x01, 0x0C]);
    }
}
