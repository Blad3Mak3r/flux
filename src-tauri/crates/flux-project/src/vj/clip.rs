use super::*;

/// A video, or a generator, in a cell of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    /// On a deck layer, the id of the column the clip is in; the layer is the one that lists it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// On a global layer, the place of the clip from the left, the same in every deck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    /// Id of an entry in `Project::videos`; empty (absent) for a generator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asset: String,
    /// What makes the clip's picture when it is not a video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
    /// Absent is the video's name, or the generator's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the clip does when it reaches its end; absent is [`Playback::Loop`].
    #[serde(default, skip_serializing_if = "Playback::is_loop")]
    pub playback: Playback,
    /// What a press on the clip does; absent is [`Trigger::Normal`].
    #[serde(default, skip_serializing_if = "Trigger::is_normal")]
    pub trigger: Trigger,
    /// How fast the clip plays when its [`tempo`](Self::tempo) is free: 1 is its own speed, from
    /// [`MIN_SPEED`] to [`MAX_SPEED`]. Absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub speed: f32,
    /// How long a still image or a generator lasts, in seconds, from [`MIN_DURATION`] to
    /// [`MAX_DURATION`]: the length its transport (region, cues, loop, once, tempo) runs over, as
    /// a video's file does for a video, which ignores it. Absent is [`DEFAULT_DURATION`].
    #[serde(
        default = "default_duration",
        skip_serializing_if = "is_default_duration"
    )]
    pub duration: f32,
    /// Which way the clip plays; absent is [`Direction::Forward`].
    #[serde(default, skip_serializing_if = "Direction::is_forward")]
    pub direction: Direction,
    /// Whether a pass lasts as long as the clip says or a number of beats; absent is
    /// [`Tempo::Free`].
    #[serde(default, skip_serializing_if = "Tempo::is_free")]
    pub tempo: Tempo,
    /// The part of the clip that plays and repeats, as fractions of its length; absent is all of it.
    #[serde(default, skip_serializing_if = "Region::is_whole")]
    pub region: Region,
    /// Places the clip jumps to, as fractions of its length; absent are all empty.
    #[serde(default, skip_serializing_if = "no_cues")]
    pub cues: [Option<f32>; CUES],
    /// How the video is sized into the composition; absent is [`VideoFit::Fit`].
    #[serde(default, skip_serializing_if = "is_fit")]
    pub fit: VideoFit,
    /// How much larger than its fit the clip is drawn, about its centre: 1 is 100%. Absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub scale: f32,
    /// How far its centre is moved from where its fit puts it, as fractions of the composition's
    /// width and height, right and down. Absent is not moved.
    #[serde(default, skip_serializing_if = "is_centred")]
    pub position: [f32; 2],
    /// The clip's effects, first applied first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// What drives `scale`, `positionX` or `positionY` by itself (see [`crate::vj_modulation`]), by
    /// field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    /// The clip's dashboard, as the layer's; it goes wherever the clip is moved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboard: Vec<Knob>,
}

fn is_fit(fit: &VideoFit) -> bool {
    *fit == VideoFit::Fit
}

fn is_centred(position: &[f32; 2]) -> bool {
    *position == [0.0, 0.0]
}

fn default_duration() -> f32 {
    DEFAULT_DURATION
}

fn is_default_duration(duration: &f32) -> bool {
    *duration == DEFAULT_DURATION
}

fn no_cues(cues: &[Option<f32>; CUES]) -> bool {
    cues.iter().all(Option::is_none)
}

/// Slowest and fastest a clip plays, times its own speed.
pub const MIN_SPEED: f32 = 0.05;

pub const MAX_SPEED: f32 = 8.0;

/// The length of a still image or a generator, in seconds: the shortest, the longest and the one a
/// new clip has.
pub const MIN_DURATION: f32 = 0.1;

pub const MAX_DURATION: f32 = 3600.0;

pub const DEFAULT_DURATION: f32 = 10.0;

/// The shortest region of a clip, as a fraction of its length.
pub const MIN_REGION: f32 = 0.001;

/// How many cues a clip has.
pub const CUES: usize = 8;

/// The lengths, in beats, that one pass of a clip can be made to last.
pub const TEMPO_BEATS: [u32; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

/// Which way a clip plays through its region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
    /// Holds the picture where it is, and is silent.
    Pause,
}

impl Direction {
    fn is_forward(&self) -> bool {
        *self == Self::Forward
    }

    /// 1 forwards, -1 backwards, 0 paused.
    pub fn sign(self) -> f64 {
        match self {
            Self::Forward => 1.0,
            Self::Reverse => -1.0,
            Self::Pause => 0.0,
        }
    }
}

/// How long one pass through a clip's region lasts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Tempo {
    /// As long as the clip's speed makes it.
    #[default]
    Free,
    /// Exactly this many beats of the composition's tempo, one of [`TEMPO_BEATS`]; the speed does
    /// not count.
    Beats { beats: u32 },
}

impl Tempo {
    fn is_free(&self) -> bool {
        *self == Self::Free
    }
}

/// The part of a clip that plays, as fractions of its length: `start` below `end`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub start: f32,
    pub end: f32,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
        }
    }
}

impl Region {
    fn is_whole(&self) -> bool {
        *self == Self::default()
    }

    /// The region with both ends brought into 0 to 1 and at least [`MIN_REGION`] apart: the start
    /// is kept unless there is no room for the end after it, and the end gives way to it.
    pub fn tidy(start: f32, end: f32) -> Result<Self> {
        if !start.is_finite() || !end.is_finite() {
            return Err(ProjectError::Invalid("a region is made of numbers".into()));
        }
        let start = start.clamp(0.0, 1.0 - MIN_REGION);
        let end = end.clamp(start + MIN_REGION, 1.0);
        Ok(Self { start, end })
    }

    /// Its length, as a fraction of the clip's.
    pub fn span(&self) -> f32 {
        self.end - self.start
    }

    pub(super) fn valid(&self) -> bool {
        self.start.is_finite()
            && self.end.is_finite()
            && self.start >= 0.0
            && self.end <= 1.0
            && self.end - self.start >= MIN_REGION * 0.999
    }
}

/// Largest scale of a clip: 1000%.
pub const MAX_SCALE: f32 = 10.0;

/// Farthest a clip's centre moves from its place, in compositions.
pub const MAX_OFFSET: f32 = 2.0;

/// What a launched clip does when it reaches the end of its video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Playback {
    /// Starts again from the beginning.
    #[default]
    Loop,
    /// Stops, and the layer shows nothing.
    Once,
    /// Plays backwards to the beginning, then forwards again.
    PingPong,
}

impl Playback {
    fn is_loop(&self) -> bool {
        *self == Self::Loop
    }
}

/// Where a clip sits: a column of a deck, or a slot of a global layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Cell {
    Column(String),
    Slot(usize),
}

impl Clip {
    pub(super) fn cell(&self) -> Option<Cell> {
        match (&self.column, self.slot) {
            (Some(column), None) => Some(Cell::Column(column.clone())),
            (None, Some(slot)) => Some(Cell::Slot(slot)),
            _ => None,
        }
    }

    pub(super) fn put(&mut self, cell: Cell) {
        match cell {
            Cell::Column(column) => (self.column, self.slot) = (Some(column), None),
            Cell::Slot(slot) => (self.column, self.slot) = (None, Some(slot)),
        }
    }
}

pub(crate) fn new_clip(id: String, asset: String, generator: Option<Generator>) -> Clip {
    Clip {
        id,
        column: None,
        slot: None,
        asset,
        generator,
        name: None,
        playback: Playback::Loop,
        trigger: Trigger::Normal,
        speed: 1.0,
        duration: DEFAULT_DURATION,
        direction: Direction::Forward,
        tempo: Tempo::Free,
        region: Region::default(),
        cues: [None; CUES],
        fit: VideoFit::Fit,
        scale: 1.0,
        position: [0.0, 0.0],
        effects: Vec::new(),
        modulation: BTreeMap::new(),
        dashboard: Vec::new(),
    }
}
