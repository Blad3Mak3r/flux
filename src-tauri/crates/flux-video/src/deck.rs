use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use std::time::Duration;

use flux_project::playhead::Tick;

use flux_project::generator::Generator;
use flux_project::vj::Vj;
use flux_project::{Timeline, TrackKind};

use crate::decode::Frame;
use crate::generate::{self, Moment};
use crate::grid::GridPlay;
use crate::layers::{Cue, Layer, layers, upcoming};
use crate::player::VideoPlayer;

/// How far ahead a clip's file is opened and its first frame decoded: two seconds at 30 fps.
pub const PRELOAD_FRAMES: u64 = 60;

/// How many pictures of a generator clip the deck keeps.
const KEPT_PICTURES: usize = 4;

/// Gives the file on disk of a video asset id.
type Resolve = Box<dyn Fn(&str) -> Option<PathBuf> + Send>;

/// A layer and the picture for it.
#[derive(Debug, Clone)]
pub struct Shown {
    pub layer: Layer,
    /// The newest frame its player decoded. Right after a seek or a jump it can still be an
    /// earlier one, or `None` before the first; `frame.index == layer.frame` once it has caught up.
    pub frame: Option<Arc<Frame>>,
}

/// Turns where the transport is into pictures: it keeps a decoding player for each clip that is
/// showing or about to, asks each for the frame the position maps to, and closes the rest. Every
/// call is driven by the engine position, never by a timer of its own, so seek, stop and loops stay
/// locked to the clock.
pub struct VideoDeck {
    resolve: Resolve,
    /// One player per clip id, so two clips of the same file never fight over one decoder.
    players: HashMap<String, VideoPlayer>,
    /// Clips whose file could not be opened, kept so it is not retried on every call.
    failed: HashMap<String, PathBuf>,
    /// The last few pictures of each generator clip shown, made again only when it changes. More
    /// than one, so that screens asking a moment apart for a modulated generator, and the monitor
    /// and the screens, find the picture another of them made instead of making it again.
    generated: HashMap<String, VecDeque<Arc<Frame>>>,
}

impl VideoDeck {
    /// `resolve` gives the file on disk of a video asset id, `None` when there is none.
    pub fn new(resolve: impl Fn(&str) -> Option<PathBuf> + Send + 'static) -> Self {
        Self {
            resolve: Box::new(resolve),
            players: HashMap::new(),
            failed: HashMap::new(),
            generated: HashMap::new(),
        }
    }

    /// Where the open clips are decoded: the GPU device when any of them decodes on one,
    /// `Some(Software)` when they all decode on the CPU, and `None` with no clip open.
    pub fn decoding(&self) -> Option<crate::Decoding> {
        use crate::Decoding::Software;
        let mut all = self.players.values().map(VideoPlayer::decoding).peekable();
        all.peek()?;
        Some(all.find(|d| *d != Software).unwrap_or(Software))
    }

    /// The layers to draw for `cue`, topmost first. Call it for every picture the outputs draw.
    pub fn show(&mut self, timeline: &Timeline, cue: Cue) -> Vec<Shown> {
        if let Cue::At(position) = cue {
            self.keep_players_near(timeline, position);
        }
        let players = &self.players;
        let probe = |clip_asset: &str| {
            players
                .values()
                .find(|player| player.asset() == clip_asset)
                .map(|player| (player.info().rate, player.info().frames))
        };
        let shown = layers(timeline, cue, &probe);
        if let Cue::At(position) = cue {
            for next in upcoming(timeline, position, PRELOAD_FRAMES, &probe) {
                if let Some(player) = self.players.get(&next.clip) {
                    player.request(next.frame);
                }
            }
        }
        self.pictures(shown, (0, 0))
    }

    /// Asks each layer's player for its frame and pairs the layer with the newest one decoded.
    /// A generator's layer gets the picture it makes at its time, for a composition of `size`.
    fn pictures(&mut self, shown: Vec<Layer>, size: (u32, u32)) -> Vec<Shown> {
        self.generated
            .retain(|clip, _| shown.iter().any(|layer| &layer.clip == clip));
        shown
            .into_iter()
            .map(|layer| {
                let frame = match &layer.generator {
                    Some(generator) => {
                        let moment = Moment {
                            elapsed: Duration::from_millis(layer.frame),
                            beats: layer.beats,
                            size,
                        };
                        self.generate(&layer.clip, generator, moment)
                    }
                    None => self.players.get(&layer.clip).and_then(|player| {
                        player.request(layer.frame);
                        player.latest()
                    }),
                };
                Shown { layer, frame }
            })
            .collect()
    }

    /// The picture of `generator`, the clip `clip`, at `moment`.
    fn generate(
        &mut self,
        clip: &str,
        generator: &Generator,
        moment: Moment,
    ) -> Option<Arc<Frame>> {
        let index = generate::index(generator, moment)?;
        let made = self.generated.entry(clip.to_owned()).or_default();
        if let Some(frame) = made.iter().find(|f| f.index == index) {
            return Some(Arc::clone(frame));
        }
        let frame = Arc::new(generate::picture(generator, index, moment));
        if made.len() >= KEPT_PICTURES {
            made.pop_front();
        }
        made.push_back(Arc::clone(&frame));
        Some(frame)
    }

    /// The layers of the VJ grid to draw `now` on the grid's clock, topmost first, in a composition
    /// of `size` pixels: it keeps a player for each clip playing and closes the rest.
    pub fn show_grid(
        &mut self,
        vj: &Vj,
        play: &GridPlay,
        now: Tick,
        size: (u32, u32),
    ) -> Vec<Shown> {
        let playing = play.clips(vj, now.time);
        self.keep_players(&playing);
        let players = &self.players;
        let probe = |asset: &str| {
            players
                .values()
                .find(|player| player.asset() == asset)
                .map(|player| (player.info().rate, player.info().frames))
        };
        let shown = play.layers(vj, now, &probe);
        self.pictures(shown, size)
    }

    /// Tries again the clips whose file failed to open: it may have been relinked.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    /// Clips whose file failed to open, with the path tried, for Show check to report.
    pub fn failed(&self) -> impl Iterator<Item = (&str, &PathBuf)> {
        self.failed.iter().map(|(clip, path)| (clip.as_str(), path))
    }

    /// Opens a player for every clip that shows at `position` or starts within the preload window,
    /// and closes the others.
    fn keep_players_near(&mut self, timeline: &Timeline, position: u64) {
        let near: Vec<(&str, &str)> = timeline
            .tracks
            .iter()
            .filter(|track| !track.muted)
            .filter_map(|track| match &track.kind {
                TrackKind::Video { clips } => Some(clips),
                _ => None,
            })
            .flatten()
            .filter(|clip| {
                let started = clip.start <= position + PRELOAD_FRAMES;
                let ended = clip
                    .length
                    .is_some_and(|length| clip.start + length <= position);
                started && !ended
            })
            .map(|clip| (clip.id.as_str(), clip.asset.as_str()))
            .collect();
        self.keep_players(&near);
    }

    /// Keeps a player open for each `(clip, asset)` and closes the others.
    fn keep_players(&mut self, near: &[(&str, &str)]) {
        self.players
            .retain(|clip, _| near.iter().any(|(id, _)| id == clip));
        for &(clip, asset) in near {
            if self.players.contains_key(clip) || self.failed.contains_key(clip) {
                continue;
            }
            let Some(path) = (self.resolve)(asset) else {
                self.failed.insert(clip.to_owned(), PathBuf::new());
                continue;
            };
            match VideoPlayer::open(&path) {
                Ok(player) => {
                    self.players
                        .insert(clip.to_owned(), player.for_asset(asset));
                }
                Err(_) => {
                    self.failed.insert(clip.to_owned(), path);
                }
            }
        }
    }
}
