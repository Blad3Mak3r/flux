//! MTC (MIDI Timecode) encoder: quarter-frame messages and the full-frame SysEx.
//!
//! Pure functions that build the bytes; when to send them is the transport's job.

use super::timecode::Timecode;
use crate::project::FrameRate;

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
}
