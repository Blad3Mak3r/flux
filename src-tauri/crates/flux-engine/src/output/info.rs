use super::*;

/// One entry of a device's `supported_output_configs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigRange {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
    pub format: SampleFormat,
}

/// Picks the stream format: the fewest channels that still cover `needed`, at `rate`, in a sample
/// format the renderer can write (32-bit float preferred, then 32, 24 and 16-bit integer). Many
/// interfaces only offer 24-bit, which is what their ASIO and WASAPI drivers report.
pub fn pick_config(ranges: &[ConfigRange], needed: u16, rate: u32) -> Option<(u16, SampleFormat)> {
    let preference = |format: SampleFormat| match format {
        SampleFormat::F32 => Some(0),
        SampleFormat::I32 => Some(1),
        SampleFormat::I24 => Some(2),
        SampleFormat::I16 => Some(3),
        _ => None,
    };
    ranges
        .iter()
        .filter(|range| {
            range.channels >= needed && (range.min_rate..=range.max_rate).contains(&rate)
        })
        .filter_map(|range| Some((range.channels, preference(range.format)?, range.format)))
        .min_by_key(|&(channels, preference, _)| (channels, preference))
        .map(|(channels, _, format)| (channels, format))
}

/// What a running output looks like to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// One entry per LTC output the project asks for, in the project's order.
    pub ltc: Vec<LtcInfo>,
    /// One entry per MIDI port the project asks for MTC on, in the project's order.
    pub mtc: Vec<MtcInfo>,
    /// Where the metronome plays; `None` when the project has no metronome output.
    pub metronome: Option<MetronomeInfo>,
    pub operator: Option<MetronomeInfo>,
    /// One entry per MIDI port and OSC destination the cue tracks of the timeline send to.
    pub cues: Vec<CueInfo>,
    /// Things that did not stop the output but the user should know, e.g. a missing MIDI port.
    pub warnings: Vec<String>,
}

/// How one LTC output ended up. When it did not open, `device` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LtcInfo {
    pub device: String,
    /// 1-based interface channel.
    pub channel: u16,
    pub ok: bool,
}

/// How the metronome ended up. When it did not open, `device` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetronomeInfo {
    pub device: String,
    pub ok: bool,
}

/// How one MTC output ended up. When it did not open, `port` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MtcInfo {
    pub port: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub state: TransportState,
    /// Frame count of the timecode being played, including the project's start offset.
    pub frame: u64,
    pub position_samples: u64,
    /// The programme's audio device failed or went away.
    pub device_lost: bool,
    /// Bit *i* is set when `OutputInfo.ltc[i]` failed or went away. Only the first 32 outputs
    /// have a bit.
    pub ltc_lost: u32,
    /// Bit *i* is set when `OutputInfo.mtc[i]` failed or went away.
    pub mtc_lost: u32,
    /// The stream the metronome plays on failed or went away.
    pub metronome_lost: bool,
    pub operator_lost: bool,
    /// The device renders audio faster than real time (ALSA's null device does), so the position
    /// and the timecode are wrong. Pick another device.
    pub clock_unpaced: bool,
}

/// Reads where the transport is. It does not borrow the [`Output`], so a UI thread can hold one.
#[derive(Debug, Clone)]
pub struct PositionReader {
    pub(super) shared: Arc<SharedClock>,
    pub(super) clock: Clock,
    pub(super) start: u64,
}

impl PositionReader {
    pub fn show(&self) -> Option<(Arc<crate::show::ShowPlan>, crate::show::ShowTick)> {
        Some((
            Arc::clone(self.shared.show_plan.get()?),
            self.shared.load().show.unwrap_or_default(),
        ))
    }

    pub fn show_snapshot(
        &self,
    ) -> Option<(Arc<crate::show::ShowPlan>, crate::show::ShowTick, Position)> {
        let snapshot = self.shared.load();
        Some((
            Arc::clone(self.shared.show_plan.get()?),
            snapshot.show.unwrap_or_default(),
            self.position_at(snapshot),
        ))
    }

    pub fn audio_routes(&self) -> Vec<crate::audio_routing::AudioRouteState> {
        self.shared
            .audio_health
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|r| r.state())
            .collect()
    }

    /// How many times the show has run to its end by itself: a change says it just ended, which
    /// the position alone cannot (a stop from the user looks the same).
    pub fn shows_ended(&self) -> u64 {
        self.shared.shows_ended()
    }

    /// What went wrong on the streams and the MIDI port since the output started.
    pub fn faults(&self) -> Faults {
        self.shared.faults()
    }

    pub fn position(&self) -> Position {
        self.position_at(self.shared.load())
    }

    /// The transport state and the frames played since the origin of the timeline that plays, as
    /// the video output maps them to pictures.
    pub fn timeline_frame(&self) -> (TransportState, u64) {
        let snapshot = self.shared.load();
        let (clock, _) = self
            .shared
            .show_timecode(&snapshot)
            .unwrap_or((self.clock, self.start));
        let samples = snapshot.estimated_position(self.shared.now(), clock);
        (snapshot.state, clock.samples_to_frames(samples))
    }

    fn position_at(&self, snapshot: Snapshot) -> Position {
        let (clock, start) = self
            .shared
            .show_timecode(&snapshot)
            .unwrap_or((self.clock, self.start));
        let position = snapshot.estimated_position(self.shared.now(), clock);
        let device_lost = self.shared.is_lost();
        let health = self.shared.health();
        Position {
            state: snapshot.state,
            frame: start + clock.samples_to_frames(position),
            position_samples: position,
            device_lost,
            ltc_lost: health.ltc_lost(device_lost),
            mtc_lost: health.mtc_lost(),
            metronome_lost: health.click_lost(device_lost),
            operator_lost: self
                .shared
                .operator_health
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|lost| lost.load(Ordering::Relaxed)),
            clock_unpaced: self.shared.is_unpaced(),
        }
    }
}

/// What `Output::reconfigure` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reconfigured {
    /// The LTC and MTC outputs now match the project; the programme's stream was not touched.
    Applied { info: OutputInfo },
    /// The change reaches the programme's stream: the output has to be opened again.
    NeedsRestart,
}
