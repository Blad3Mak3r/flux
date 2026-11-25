use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use std::time::Duration;

use flux_project::playhead::Tick;

use flux_project::generator::Generator;
use flux_project::vj::Vj;
use flux_project::{Timeline, TrackKind};

use crate::decode::Frame;
use crate::generate;
use crate::grid::GridPlay;
use crate::layers::{Cue, Layer, layers, upcoming};
use crate::player::VideoPlayer;

/// How far ahead a clip's file is opened and its first frame decoded: two seconds at 30 fps.
pub const PRELOAD_FRAMES: u64 = 60;

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
    /// The last picture of each generator clip shown, made again only when it changes.
    generated: HashMap<String, Arc<Frame>>,
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
        self.pictures(shown)
    }

    /// Asks each layer's player for its frame and pairs the layer with the newest one decoded.
    /// A generator's layer gets the picture it makes at its time.
    fn pictures(&mut self, shown: Vec<Layer>) -> Vec<Shown> {
        self.generated
            .retain(|clip, _| shown.iter().any(|layer| &layer.clip == clip));
        shown
            .into_iter()
            .map(|layer| {
                let frame = match &layer.generator {
                    Some(generator) => self.generate(&layer.clip, generator, layer.frame),
                    None => self.players.get(&layer.clip).and_then(|player| {
                        player.request(layer.frame);
                        player.latest()
                    }),
                };
                Shown { layer, frame }
            })
            .collect()
    }

    /// The picture of `generator`, the clip `clip`, `elapsed_ms` after it was launched.
    fn generate(
        &mut self,
        clip: &str,
        generator: &Generator,
        elapsed_ms: u64,
    ) -> Option<Arc<Frame>> {
        let index = generate::index(generator, Duration::from_millis(elapsed_ms))?;
        if let Some(frame) = self.generated.get(clip).filter(|f| f.index == index) {
            return Some(Arc::clone(frame));
        }
        let frame = Arc::new(generate::picture(generator, index));
        self.generated.insert(clip.to_owned(), Arc::clone(&frame));
        Some(frame)
    }

    /// The layers of the VJ grid to draw `now` on the grid's clock, topmost first: it keeps a player
    /// for each clip playing and closes the rest.
    pub fn show_grid(&mut self, vj: &Vj, play: &GridPlay, now: Tick) -> Vec<Shown> {
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
        self.pictures(shown)
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
