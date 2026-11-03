# Timecode signals

Pure encoders in `src-tauri/src/engine/`. They only build bits, bytes and samples; the transport decides when to
send them. Frame math (`Timecode` <-> frame count, 29.97 drop-frame) is `timecode.rs`.

## LTC (`ltc.rs`)

`frame_bits` builds the 80-bit SMPTE 12M word, `bits[0]` is sent first:

| Bits | Content |
| --- | --- |
| 0-3, 8-9 | Frame units, tens (BCD, least significant bit first) |
| 10 | Drop-frame flag (29.97 only) |
| 11 | Colour frame flag (always 0) |
| 16-19, 24-26 | Seconds units, tens |
| 32-35, 40-42 | Minutes units, tens |
| 48-51, 56-57 | Hours units, tens |
| 4, 12, ... 60 | The eight user-bit nibbles |
| 27 (59 at 25 fps) | Polarity correction: makes the number of ones even, so every frame starts with the same polarity |
| 64-79 | Sync word `0x3FFD` |

`LtcEncoder::encode_frame` appends the biphase-mark samples of one word: a transition at the start of every cell
and another in the middle of a 1. A frame lasts `sample_rate / fps` samples, rarely a whole number, so cell
boundaries are computed from a running half-cell count with exact integer arithmetic and the remainder carries into
the next frame (1000 frames of 29.97 fps at 48 kHz are exactly 1 601 600 samples).

`LtcConfig`: `sample_rate`, `level` (linear peak amplitude) and `edge_samples` (slew limit: samples an edge takes
to swing between polarities, 0 is a hard square wave). `max_frame_samples()` gives the buffer size to reserve so
the audio callback never allocates.

## MTC (`mtc.rs`)

- `quarter_frames` returns the eight `F1 nn` messages; the high nibble of `nn` is the piece number, the low nibble
  is a nibble of the binary frames, seconds, minutes and hours. Piece 7 also carries the rate code (0 = 24,
  1 = 25, 2 = 29.97 drop, 3 = 30).
- A group describes the frame at which **piece 0 is sent**; pieces follow every quarter frame, so it takes two
  frames and receivers add two frames once they have all eight.
- `full_frame` returns `F0 7F 7F 01 01 hh mm ss ff F7` with the rate code in bits 5-6 of `hh`. It is meant for
  start and seek.

## Open decisions

Default LTC level and edge shaping, and the sample-rate policy, are not decided: the encoder takes them from the
caller. Where the LTC word sits relative to the frame boundary is up to the transport.
