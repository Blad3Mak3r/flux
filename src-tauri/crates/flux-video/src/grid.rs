//! What the VJ grid of a project shows: the clips launched by hand, one per layer, each playing
//! from the instant it was launched on a clock of its own, not the timeline's. Which clip plays is
//! the running show's, never stored in the project.
//!
//! Every function takes the time since the grid's clock started, so the picture of an instant is a
//! pure function of what was launched and when.

use std::collections::HashMap;
use std::time::Duration;

use flux_project::vj::{Playback, Side, Vj};

use crate::layers::{Layer, Probe};
use crate::mapping::Rate;

/// A clip launched on a layer, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    pub clip: String,
    /// On the grid's clock.
    pub at: Duration,
}

/// What the grid is playing: at most one clip per layer, and where the crossfader is.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPlay {
    /// By layer id.
    playing: HashMap<String, Launched>,
    /// 0 is all side A, 1 all side B; in the middle both show fully.
    crossfader: f32,
}

impl Default for GridPlay {
    fn default() -> Self {
        Self {
            playing: HashMap::new(),
            crossfader: 0.5,
        }
    }
}

impl GridPlay {
    /// Plays `clip` on its layer from its first frame, in place of what the layer played. A clip
    /// that is already playing starts again. `false` when the grid has no such clip.
    pub fn launch(&mut self, vj: &Vj, clip: &str, now: Duration) -> bool {
        let Some(layer) = vj
            .layers
            .iter()
            .find(|layer| layer.clips.iter().any(|c| c.id == clip))
        else {
            return false;
        };
        self.playing.insert(
            layer.id.clone(),
            Launched {
                clip: clip.to_owned(),
                at: now,
            },
        );
        true
    }

    /// Launches every clip of `column`, all on the same instant; a layer without a clip in it
    /// stops, so the column is the whole picture. `false` when the grid has no such column.
    pub fn launch_column(&mut self, vj: &Vj, column: &str, now: Duration) -> bool {
        if !vj.columns.iter().any(|c| c.id == column) {
            return false;
        }
        for layer in &vj.layers {
            match layer.clips.iter().find(|clip| clip.column == column) {
                Some(clip) => {
                    let launched = Launched {
                        clip: clip.id.clone(),
                        at: now,
                    };
                    self.playing.insert(layer.id.clone(), launched);
                }
                None => {
                    self.playing.remove(&layer.id);
                }
            }
        }
        true
    }

    /// Stops the layer, or every layer with `None`.
    pub fn stop(&mut self, layer: Option<&str>) {
        match layer {
            Some(layer) => {
                self.playing.remove(layer);
            }
            None => self.playing.clear(),
        }
    }

    /// Moves the crossfader, kept between 0 and 1.
    pub fn set_crossfader(&mut self, position: f32) {
        if position.is_finite() {
            self.crossfader = position.clamp(0.0, 1.0);
        }
    }

    pub fn crossfader(&self) -> f32 {
        self.crossfader
    }

    /// The clip playing on each layer, by layer id.
    pub fn playing(&self) -> &HashMap<String, Launched> {
        &self.playing
    }

    /// Forgets what plays on layers the grid no longer has, or with clips it no longer has there.
    pub fn retain(&mut self, vj: &Vj) {
        self.playing.retain(|layer, launched| {
            vj.layers
                .iter()
                .find(|l| &l.id == layer)
                .is_some_and(|l| l.clips.iter().any(|c| c.id == launched.clip))
        });
    }

    /// What the grid shows `now`, the topmost layer first. A layer shows nothing when it plays
    /// nothing, its video is not open yet, a clip played once has ended, or it is faded out.
    pub fn layers(&self, vj: &Vj, now: Duration, probe: Probe<'_>) -> Vec<Layer> {
        vj.layers
            .iter()
            .rev()
            .filter_map(|layer| {
                let launched = self.playing.get(&layer.id)?;
                let clip = layer.clips.iter().find(|c| c.id == launched.clip)?;
                let (rate, frames) = probe(&clip.asset)?;
                let frame =
                    grid_frame(now.saturating_sub(launched.at), rate, frames, clip.playback)?;
                let opacity = layer.opacity * self.side_level(layer.crossfade);
                (opacity > 0.0).then(|| Layer {
                    track: layer.id.clone(),
                    clip: clip.id.clone(),
                    asset: clip.asset.clone(),
                    frame,
                    fit: clip.fit,
                    opacity,
                })
            })
            .collect()
    }

    /// How much the crossfader lets a layer on `side` through: each side is full from its end to
    /// the middle and fades out over the other half.
    fn side_level(&self, side: Option<Side>) -> f32 {
        match side {
            None => 1.0,
            Some(Side::A) => (2.0 * (1.0 - self.crossfader)).min(1.0),
            Some(Side::B) => (2.0 * self.crossfader).min(1.0),
        }
    }
}

/// Which frame of a video of `frames` at `rate` a clip shows `elapsed` after it was launched, or
/// `None` once a clip played once has ended. On integers, so it never drifts.
pub fn grid_frame(elapsed: Duration, rate: Rate, frames: u64, playback: Playback) -> Option<u64> {
    if frames == 0 || rate.num == 0 || rate.den == 0 {
        return None;
    }
    let played = elapsed.as_nanos() * u128::from(rate.num) / (u128::from(rate.den) * 1_000_000_000);
    let frames = u128::from(frames);
    let index = match playback {
        Playback::Once => (played < frames).then_some(played)?,
        Playback::Loop => played % frames,
        Playback::PingPong if frames == 1 => 0,
        Playback::PingPong => {
            // Forwards over frames 0 to last, then back over last - 1 to 1.
            let period = 2 * (frames - 1);
            let at = played % period;
            if at < frames { at } else { period - at }
        }
    };
    u64::try_from(index).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::vj::{Clip, Column, Layer as GridLayer};

    const RATE: Rate = Rate::new(25, 1);

    fn secs(seconds: f64) -> Duration {
        Duration::from_secs_f64(seconds)
    }

    fn clip(id: &str, column: &str, playback: Playback) -> Clip {
        Clip {
            id: id.into(),
            column: column.into(),
            asset: "v1".into(),
            name: None,
            playback,
            fit: Default::default(),
        }
    }

    fn layer(id: &str, clips: Vec<Clip>, crossfade: Option<Side>) -> GridLayer {
        GridLayer {
            id: id.into(),
            name: None,
            opacity: 1.0,
            gain_db: 0.0,
            crossfade,
            clips,
        }
    }

    /// Two layers (l1 at the bottom, l2 on top) and two columns.
    fn grid() -> Vj {
        let column = |id: &str| Column {
            id: id.into(),
            name: None,
        };
        Vj {
            layers: vec![
                layer(
                    "l1",
                    vec![
                        clip("a", "c1", Playback::Loop),
                        clip("b", "c2", Playback::Once),
                    ],
                    Some(Side::A),
                ),
                layer("l2", vec![clip("c", "c1", Playback::Loop)], Some(Side::B)),
            ],
            columns: vec![column("c1"), column("c2")],
        }
    }

    fn probe(_: &str) -> Option<(Rate, u64)> {
        Some((RATE, 100))
    }

    #[test]
    fn grid_frame_should_loop_stop_or_bounce_at_the_end() {
        let at = |seconds, playback| grid_frame(secs(seconds), RATE, 100, playback);
        assert_eq!(at(1.0, Playback::Loop), Some(25));
        assert_eq!(at(4.4, Playback::Loop), Some(10));
        assert_eq!(at(3.96, Playback::Once), Some(99));
        assert_eq!(at(4.0, Playback::Once), None);
        assert_eq!(at(4.0, Playback::PingPong), Some(98));
        assert_eq!(at(7.92, Playback::PingPong), Some(0));
        assert_eq!(at(8.0, Playback::PingPong), Some(2));
    }

    #[test]
    fn grid_frame_should_follow_a_fractional_rate() {
        let ntsc = Rate::new(30000, 1001);
        let played = grid_frame(Duration::from_secs(1001), ntsc, 100_000, Playback::Loop);
        assert_eq!(played, Some(30_000));
    }

    #[test]
    fn launch_should_replace_the_clip_of_its_layer_and_restart_a_playing_one() {
        let vj = grid();
        let mut play = GridPlay::default();
        assert!(play.launch(&vj, "a", secs(1.0)));
        assert!(play.launch(&vj, "b", secs(2.0)));
        assert_eq!(play.playing().len(), 1);
        assert_eq!(play.playing()["l1"].clip, "b");
        play.launch(&vj, "b", secs(3.0));
        assert_eq!(play.playing()["l1"].at, secs(3.0));
        assert!(!play.launch(&vj, "nope", secs(3.0)));
    }

    #[test]
    fn launch_column_should_stop_the_layers_without_a_clip_in_it() {
        let vj = grid();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(0.0));
        assert_eq!(play.playing().len(), 2);
        play.launch_column(&vj, "c2", secs(1.0));
        let playing: Vec<_> = play.playing().keys().cloned().collect();
        assert_eq!(playing, ["l1"]);
    }

    #[test]
    fn layers_should_list_the_top_first_with_the_frame_since_the_launch() {
        let vj = grid();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(1.0));
        let shown = play.layers(&vj, secs(2.0), &probe);
        let order: Vec<_> = shown.iter().map(|l| (l.track.as_str(), l.frame)).collect();
        assert_eq!(order, [("l2", 25), ("l1", 25)]);
    }

    #[test]
    fn layers_should_drop_a_clip_played_once_after_its_end() {
        let vj = grid();
        let mut play = GridPlay::default();
        play.launch(&vj, "b", secs(0.0));
        assert_eq!(play.layers(&vj, secs(3.0), &probe).len(), 1);
        assert!(play.layers(&vj, secs(5.0), &probe).is_empty());
    }

    #[test]
    fn crossfader_should_fade_each_side_over_the_far_half() {
        let vj = grid();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(0.0));
        let levels = |play: &GridPlay| {
            play.layers(&vj, secs(0.0), &probe)
                .iter()
                .map(|l| l.opacity)
                .collect::<Vec<_>>()
        };
        assert_eq!(levels(&play), [1.0, 1.0]);
        play.set_crossfader(0.25);
        assert_eq!(levels(&play), [0.5, 1.0]);
        play.set_crossfader(0.0);
        assert_eq!(levels(&play), [1.0]);
        play.set_crossfader(f32::NAN);
        assert_eq!(play.crossfader(), 0.0);
    }

    #[test]
    fn retain_should_forget_clips_the_grid_lost() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(0.0));
        vj.layers[1].clips.clear();
        play.retain(&vj);
        assert_eq!(play.playing().len(), 1);
    }
}
