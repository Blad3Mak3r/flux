//! What the VJ grid of a project shows: the clips launched by hand, one per layer, each playing
//! from the instant it was launched on a clock of its own, not the timeline's. Which clip plays is
//! the running show's, never stored in the project.
//!
//! Every function takes the time since the grid's clock started, so the picture of an instant is a
//! pure function of what was launched and when.

use std::collections::HashMap;
use std::time::Duration;

use flux_project::vj::{Playback, Side, Transition, Trigger, Vj};

use crate::layers::{Layer, Probe};
use crate::mapping::Rate;

/// A clip launched on a layer, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Launched {
    pub clip: String,
    /// On the grid's clock.
    pub at: Duration,
    /// The clip it replaced, while the layer's transition from it runs.
    pub leaving: Option<Leaving>,
}

/// The clip a layer played before the one launched, which shows until the transition ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaving {
    pub clip: String,
    /// When it was launched, on the grid's clock.
    pub at: Duration,
    /// How long the transition takes from the launch of the new clip.
    pub over: Duration,
}

/// What the grid is playing: at most one clip per layer, and where the crossfader is.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPlay {
    /// By layer id.
    playing: HashMap<String, Launched>,
    /// 0 is all side A, 1 all side B; in the middle both show fully.
    crossfader: f32,
    /// The column launched last, until a clip is launched or a layer stopped by hand.
    column: Option<String>,
}

impl Default for GridPlay {
    fn default() -> Self {
        Self {
            playing: HashMap::new(),
            crossfader: 0.5,
            column: None,
        }
    }
}

impl GridPlay {
    /// Plays `clip` on its layer from its first frame, in place of what the layer played, over
    /// the layer's transition. A clip that is already playing starts again, at once. `false` when
    /// the grid has no such clip.
    pub fn launch(&mut self, vj: &Vj, clip: &str, now: Duration) -> bool {
        let Some((layer, _)) = vj.find_clip(clip) else {
            return false;
        };
        let over = Duration::from_secs_f32(layer.transition);
        self.start(&layer.id, clip, over, now);
        self.column = None;
        true
    }

    fn start(&mut self, layer: &str, clip: &str, over: Duration, now: Duration) {
        let leaving = self
            .playing
            .get(layer)
            .filter(|before| before.clip != clip && !over.is_zero())
            .map(|before| Leaving {
                clip: before.clip.clone(),
                at: before.at,
                over,
            });
        let launched = Launched {
            clip: clip.to_owned(),
            at: now,
            leaving,
        };
        self.playing.insert(layer.to_owned(), launched);
    }

    /// Lets go of `clip`: a clip triggered as a piano key stops when it is let go, if it still
    /// plays. `false` when the grid has no such clip.
    pub fn release(&mut self, vj: &Vj, clip: &str) -> bool {
        let Some((layer, found)) = vj.find_clip(clip) else {
            return false;
        };
        if found.trigger == Trigger::Piano
            && self.playing.get(&layer.id).is_some_and(|l| l.clip == clip)
        {
            self.playing.remove(&layer.id);
        }
        true
    }

    /// Launches every clip of `column` of any deck, all on the same instant; a deck layer without
    /// a clip in it stops, so the column is the whole picture. Global layers play on. `false` when
    /// the grid has no such column.
    pub fn launch_column(&mut self, vj: &Vj, column: &str, now: Duration) -> bool {
        if vj.column_place(column).is_none() {
            return false;
        }
        self.column = Some(column.to_owned());
        for (layer, clip) in vj.column_clips(column) {
            match clip {
                Some(clip) => {
                    let over = Duration::from_secs_f32(layer.transition);
                    self.start(&layer.id, &clip.id, over, now);
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
        self.column = None;
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

    /// The column launched last, while nothing was launched or stopped by hand since.
    pub fn column(&self) -> Option<&str> {
        self.column.as_deref()
    }

    /// The clip playing on each layer, by layer id.
    pub fn playing(&self) -> &HashMap<String, Launched> {
        &self.playing
    }

    /// Every clip that may show `now`, those leaving included, as (clip id, asset id).
    pub fn clips<'a>(&self, vj: &'a Vj, now: Duration) -> Vec<(&'a str, &'a str)> {
        let mut clips = Vec::new();
        for layer in &vj.layers {
            let Some(launched) = self.playing.get(&layer.id) else {
                continue;
            };
            let leaving = launched
                .leaving
                .as_ref()
                .filter(|leaving| now.saturating_sub(launched.at) < leaving.over);
            for id in std::iter::once(&launched.clip).chain(leaving.map(|l| &l.clip)) {
                if let Some(clip) = layer.clips.iter().find(|c| &c.id == id) {
                    clips.push((clip.id.as_str(), clip.asset.as_str()));
                }
            }
        }
        clips
    }

    /// Forgets what plays on layers the grid no longer has, or with clips it no longer has there.
    pub fn retain(&mut self, vj: &Vj) {
        self.playing.retain(|layer, launched| {
            let Some(layer) = vj.layers.iter().find(|l| &l.id == layer) else {
                return false;
            };
            let has = |clip: &str| layer.clips.iter().any(|c| c.id == clip);
            if launched.leaving.as_ref().is_some_and(|l| !has(&l.clip)) {
                launched.leaving = None;
            }
            has(&launched.clip)
        });
    }

    /// What the grid shows `now`, the topmost first. A layer shows nothing when it plays nothing,
    /// is bypassed, another layer is soloed and it is not, its video is not open yet, a clip played
    /// once has ended, or it is faded out. While a transition runs, a layer shows its new clip over
    /// the one leaving it.
    pub fn layers(&self, vj: &Vj, now: Duration, probe: Probe<'_>) -> Vec<Layer> {
        let mut shown = Vec::new();
        for layer in vj.layers.iter().rev() {
            let Some(launched) = self.playing.get(&layer.id) else {
                continue;
            };
            let opacity = self.level(vj, layer);
            if opacity <= 0.0 {
                continue;
            }
            let picture = |clip: &str, at: Duration, opacity: f32| {
                let clip = layer.clips.iter().find(|c| c.id == clip)?;
                let (rate, frames) = probe(&clip.asset)?;
                let frame = grid_frame(now.saturating_sub(at), rate, frames, clip.playback)?;
                (opacity > 0.0).then(|| Layer {
                    track: layer.id.clone(),
                    clip: clip.id.clone(),
                    asset: clip.asset.clone(),
                    frame,
                    fit: clip.fit,
                    opacity,
                    blend: layer.blend,
                })
            };
            let elapsed = now.saturating_sub(launched.at);
            match launched.leaving.as_ref().filter(|l| elapsed < l.over) {
                Some(leaving) => {
                    let t = elapsed.as_secs_f32() / leaving.over.as_secs_f32();
                    let old = match layer.transition_effect {
                        Transition::Alpha => opacity,
                        Transition::Add => opacity * (1.0 - t),
                    };
                    shown.extend(picture(&launched.clip, launched.at, opacity * t));
                    shown.extend(picture(&leaving.clip, leaving.at, old));
                }
                None => shown.extend(picture(&launched.clip, launched.at, opacity)),
            }
        }
        shown
    }

    /// How much of `layer` shows, 0 to 1: its master, opacity and crossfader side, or nothing when it
    /// is bypassed or another layer is soloed and it is not.
    pub fn level(&self, vj: &Vj, layer: &flux_project::vj::Layer) -> f32 {
        let solo = vj.layers.iter().any(|layer| layer.solo);
        if layer.bypass || (solo && !layer.solo) {
            return 0.0;
        }
        layer.master * layer.opacity * self.side_level(layer.crossfade)
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
    use flux_project::vj::{Blend, Clip, Column, Deck, Layer as GridLayer};

    const RATE: Rate = Rate::new(25, 1);

    fn secs(seconds: f64) -> Duration {
        Duration::from_secs_f64(seconds)
    }

    fn clip(id: &str, column: &str, playback: Playback) -> Clip {
        Clip {
            id: id.into(),
            column: Some(column.into()),
            slot: None,
            asset: "v1".into(),
            name: None,
            playback,
            trigger: Trigger::Normal,
            fit: Default::default(),
        }
    }

    fn layer(id: &str, clips: Vec<Clip>, crossfade: Option<Side>) -> GridLayer {
        GridLayer {
            id: id.into(),
            name: None,
            global: false,
            master: 1.0,
            opacity: 1.0,
            volume: 1.0,
            blend: Blend::Alpha,
            transition: 0.0,
            transition_effect: Transition::Alpha,
            crossfade,
            bypass: false,
            solo: false,
            clips,
        }
    }

    /// Two layers (l1 at the bottom, l2 on top) and a deck of two columns.
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
            decks: vec![Deck {
                id: "d1".into(),
                name: None,
                columns: vec![column("c1"), column("c2")],
            }],
            deck: "d1".into(),
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

    #[test]
    fn launch_should_cross_over_the_layer_transition() {
        let mut vj = grid();
        vj.layers[0].transition = 2.0;
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.launch(&vj, "b", secs(10.0));
        let shown = play.layers(&vj, secs(10.5), &probe);
        let pictures: Vec<_> = shown.iter().map(|l| (l.clip.as_str(), l.opacity)).collect();
        assert_eq!(pictures, [("b", 0.25), ("a", 1.0)]);
        assert_eq!(play.clips(&vj, secs(10.5)).len(), 2);
        vj.layers[0].transition_effect = Transition::Add;
        let shown = play.layers(&vj, secs(10.5), &probe);
        assert_eq!(shown[1].opacity, 0.75);
        assert_eq!(play.layers(&vj, secs(12.0), &probe).len(), 1);
        assert_eq!(play.clips(&vj, secs(12.0)), [("b", "v1")]);
    }

    #[test]
    fn release_should_stop_only_a_piano_clip_still_playing() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.release(&vj, "a");
        assert_eq!(play.playing().len(), 1);
        vj.layers[0].clips[0].trigger = Trigger::Piano;
        play.release(&vj, "a");
        assert!(play.playing().is_empty());
        assert!(!play.release(&vj, "nope"));
    }

    #[test]
    fn layers_should_skip_bypassed_and_unsoloed_layers_and_apply_the_master() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(0.0));
        vj.layers[0].master = 0.5;
        vj.layers[0].blend = Blend::Add;
        let shown = play.layers(&vj, secs(0.0), &probe);
        assert_eq!((shown[1].opacity, shown[1].blend), (0.5, Blend::Add));
        vj.layers[1].bypass = true;
        assert_eq!(play.layers(&vj, secs(0.0), &probe).len(), 1);
        vj.layers[1].bypass = false;
        vj.layers[1].solo = true;
        let shown = play.layers(&vj, secs(0.0), &probe);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].track, "l2");
    }

    #[test]
    fn launch_column_should_leave_global_layers_alone() {
        let mut vj = grid();
        let mut global = layer("g", vec![clip("logo", "c1", Playback::Loop)], None);
        global.global = true;
        global.clips[0].column = None;
        global.clips[0].slot = Some(0);
        vj.layers.push(global);
        let mut play = GridPlay::default();
        play.launch(&vj, "logo", secs(0.0));
        play.launch_column(&vj, "c2", secs(1.0));
        assert_eq!(play.playing()["g"].clip, "logo");
        assert!(!play.playing().contains_key("l2"));
    }
}
