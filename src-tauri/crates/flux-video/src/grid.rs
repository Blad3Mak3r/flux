//! What the VJ grid of a project shows: the clips launched by hand, one per layer, each playing
//! from the instant it was launched on a clock of its own, not the timeline's. Which clip plays is
//! the running show's, never stored in the project.
//!
//! Every function takes the time since the grid's clock started, so the picture of an instant is a
//! pure function of what was launched and when.

use std::collections::HashMap;
use std::time::Duration;

use flux_project::VideoFit;
use flux_project::vj::{Playback, Side, Transition, Trigger, Vj};

use crate::layers::{Layer, Probe};
use crate::mapping::Rate;

/// What a layer plays: the clip launched last, and the transition into it while it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Launched {
    /// `None` while the layer fades out to nothing after it was stopped.
    pub clip: Option<String>,
    /// When the clip was launched, on the grid's clock.
    pub at: Duration,
    /// The layer's transition into `clip`, until it ends.
    pub fade: Option<Fade>,
}

/// A transition running on a layer: from the clip it played, or from nothing, to the one launched
/// (or to nothing). It moves at the speed of the layer's transition time, which can change while
/// it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Fade {
    /// The clip leaving and when it was launched; `None` when the layer fades in from nothing.
    pub from: Option<(String, Duration)>,
    /// When the fade started or last changed speed, and how far it was then, 0 to 1.
    pub since: Duration,
    pub done: f32,
    /// How long the whole fade takes at its present speed.
    pub over: Duration,
}

impl Fade {
    /// How far the fade is `now`, 0 to 1.
    pub fn progress(&self, now: Duration) -> f32 {
        if self.over.is_zero() {
            return 1.0;
        }
        let moved = now.saturating_sub(self.since).as_secs_f32() / self.over.as_secs_f32();
        (self.done + moved).min(1.0)
    }
}

impl Launched {
    /// The transition, while it runs `now`.
    pub fn fading(&self, now: Duration) -> Option<&Fade> {
        self.fade.as_ref().filter(|fade| fade.progress(now) < 1.0)
    }

    /// The clip the layer shows `now` most, and when it was launched: the one launched, or the one
    /// leaving while the layer fades out.
    pub fn shown(&self, now: Duration) -> Option<(&str, Duration)> {
        match &self.clip {
            Some(clip) => Some((clip.as_str(), self.at)),
            None => self
                .fading(now)
                .and_then(|fade| fade.from.as_ref())
                .map(|(clip, at)| (clip.as_str(), *at)),
        }
    }
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

fn transition(vj: &Vj, layer: &str) -> Duration {
    vj.layers
        .iter()
        .find(|l| l.id == layer)
        .map_or(Duration::ZERO, |l| Duration::from_secs_f32(l.transition))
}

impl GridPlay {
    /// Plays `clip` on its layer from its first frame, in place of what the layer played, over
    /// the layer's transition. A clip that is already playing starts again, at once. `false` when
    /// the grid has no such clip.
    pub fn launch(&mut self, vj: &Vj, clip: &str, now: Duration) -> bool {
        let Some((layer, _)) = vj.find_clip(clip) else {
            return false;
        };
        self.start(vj, &layer.id, Some(clip), now);
        self.column = None;
        true
    }

    /// Moves `layer` to `clip`, or to nothing, over the layer's transition: what it shows now
    /// leaves as the new clip comes in.
    fn start(&mut self, vj: &Vj, layer: &str, clip: Option<&str>, now: Duration) {
        let over = transition(vj, layer);
        let before = self.playing.get(layer);
        if clip.is_some() && before.and_then(|b| b.clip.as_deref()) == clip {
            self.playing.insert(
                layer.to_owned(),
                Launched {
                    clip: clip.map(str::to_owned),
                    at: now,
                    fade: None,
                },
            );
            return;
        }
        let from = before
            .and_then(|before| before.shown(now))
            .map(|(clip, at)| (clip.to_owned(), at));
        if clip.is_none() && (from.is_none() || over.is_zero()) {
            self.playing.remove(layer);
            return;
        }
        let fade = (!over.is_zero()).then_some(Fade {
            from,
            since: now,
            done: 0.0,
            over,
        });
        let launched = Launched {
            clip: clip.map(str::to_owned),
            at: now,
            fade,
        };
        self.playing.insert(layer.to_owned(), launched);
    }

    /// Lets go of `clip`: a clip triggered as a piano key stops, over the layer's transition, when
    /// it is let go, if it still plays. `false` when the grid has no such clip.
    pub fn release(&mut self, vj: &Vj, clip: &str, now: Duration) -> bool {
        let Some((layer, found)) = vj.find_clip(clip) else {
            return false;
        };
        if found.trigger == Trigger::Piano
            && self
                .playing
                .get(&layer.id)
                .is_some_and(|l| l.clip.as_deref() == Some(clip))
        {
            self.start(vj, &layer.id, None, now);
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
            self.start(vj, &layer.id, clip.map(|clip| clip.id.as_str()), now);
        }
        true
    }

    /// Stops the layer, or every layer with `None`, each over its transition.
    pub fn stop(&mut self, vj: &Vj, layer: Option<&str>, now: Duration) {
        self.column = None;
        let layers: Vec<String> = match layer {
            Some(layer) => vec![layer.to_owned()],
            None => self.playing.keys().cloned().collect(),
        };
        for layer in layers {
            if self.playing.get(&layer).is_some_and(|l| l.clip.is_some()) {
                self.start(vj, &layer, None, now);
            }
        }
    }

    /// Follows a layer's transition time changed while a transition runs, so it goes on from where
    /// it is at the new speed, and forgets the transitions that ended.
    pub fn retime(&mut self, vj: &Vj, now: Duration) {
        self.playing.retain(|layer, launched| {
            if let Some(fade) = &mut launched.fade {
                let over = transition(vj, layer);
                if over != fade.over {
                    fade.done = fade.progress(now);
                    fade.since = now;
                    fade.over = over;
                }
                if fade.progress(now) >= 1.0 {
                    launched.fade = None;
                }
            }
            launched.clip.is_some() || launched.fade.is_some()
        });
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

    /// What each layer plays, by layer id.
    pub fn playing(&self) -> &HashMap<String, Launched> {
        &self.playing
    }

    /// Every video clip that may show `now`, those leaving included, as (clip id, asset id).
    pub fn clips<'a>(&self, vj: &'a Vj, now: Duration) -> Vec<(&'a str, &'a str)> {
        let mut clips = Vec::new();
        for layer in &vj.layers {
            let Some(launched) = self.playing.get(&layer.id) else {
                continue;
            };
            let leaving = launched.fading(now).and_then(|fade| fade.from.as_ref());
            for id in launched.clip.iter().chain(leaving.map(|(clip, _)| clip)) {
                let found = layer.clips.iter().find(|c| &c.id == id);
                if let Some(clip) = found.filter(|clip| clip.generator.is_none()) {
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
            if let Some(fade) = &mut launched.fade
                && fade.from.as_ref().is_some_and(|(clip, _)| !has(clip))
            {
                fade.from = None;
            }
            match &launched.clip {
                Some(clip) => has(clip),
                None => launched.fade.as_ref().is_some_and(|f| f.from.is_some()),
            }
        });
    }

    /// What the grid shows `now`, the topmost first. A layer shows nothing when it plays nothing,
    /// is bypassed, another layer is soloed and it is not, its video is not open yet, a clip played
    /// once has ended, or it is faded out. While a transition runs, a layer shows its new clip over
    /// the one leaving it: with the `Alpha` effect the new one fades in over the old, with `Add`
    /// one fades out as the other fades in. A layer stopped or started from nothing fades.
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
                let elapsed = now.saturating_sub(at);
                // A generator fills the composition and never ends.
                let (frame, fit) = match &clip.generator {
                    Some(_) => (elapsed.as_millis() as u64, VideoFit::Stretch),
                    None => {
                        let (rate, frames) = probe(&clip.asset)?;
                        (grid_frame(elapsed, rate, frames, clip.playback)?, clip.fit)
                    }
                };
                (opacity > 0.0).then(|| Layer {
                    track: layer.id.clone(),
                    clip: clip.id.clone(),
                    asset: clip.asset.clone(),
                    frame,
                    generator: clip.generator.clone(),
                    fit,
                    opacity,
                    blend: layer.blend,
                    scale: clip.scale,
                    position: clip.position,
                })
            };
            let fading = launched.fading(now);
            let t = fading.map_or(1.0, |fade| fade.progress(now));
            if let Some(clip) = &launched.clip {
                shown.extend(picture(clip, launched.at, opacity * t));
            }
            if let Some((clip, at)) = fading.and_then(|fade| fade.from.as_ref()) {
                let old = match layer.transition_effect {
                    Transition::Alpha if launched.clip.is_some() => opacity,
                    _ => opacity * (1.0 - t),
                };
                shown.extend(picture(clip, *at, old));
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
/// `None` once a clip played once has ended. On integers, so it never drifts. A still image (one
/// frame) never ends: it shows until it is stopped, whatever its playback.
pub fn grid_frame(elapsed: Duration, rate: Rate, frames: u64, playback: Playback) -> Option<u64> {
    if frames == 0 || rate.num == 0 || rate.den == 0 {
        return None;
    }
    if frames == 1 {
        return Some(0);
    }
    let played = elapsed.as_nanos() * u128::from(rate.num) / (u128::from(rate.den) * 1_000_000_000);
    let frames = u128::from(frames);
    let index = match playback {
        Playback::Once => (played < frames).then_some(played)?,
        Playback::Loop => played % frames,
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
            generator: None,
            name: None,
            playback,
            trigger: Trigger::Normal,
            fit: Default::default(),
            scale: 1.0,
            position: [0.0, 0.0],
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
            collapsed: false,
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
            clock: Default::default(),
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
        for playback in [Playback::Once, Playback::Loop, Playback::PingPong] {
            let still = grid_frame(Duration::from_secs(60), Rate::new(25, 1), 1, playback);
            assert_eq!(still, Some(0), "a still image shows until it is stopped");
        }
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
        assert_eq!(play.playing()["l1"].clip.as_deref(), Some("b"));
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
    fn a_generator_should_play_with_no_video_and_never_end() {
        let mut vj = grid();
        let clip = &mut vj.layers[0].clips[1];
        clip.asset.clear();
        clip.generator = Some(flux_project::generator::Generator::Bars);
        let mut play = GridPlay::default();
        play.launch(&vj, "b", secs(0.0));
        assert!(play.clips(&vj, secs(1.0)).is_empty(), "no video to open");
        let none = |_: &str| None;
        let shown = play.layers(&vj, secs(3600.0), &none);
        assert_eq!(shown.len(), 1, "played once, yet still showing");
        assert_eq!(shown[0].frame, 3_600_000);
        assert_eq!(shown[0].fit, VideoFit::Stretch);
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
    fn launch_and_stop_should_fade_from_and_to_nothing() {
        let mut vj = grid();
        vj.layers[0].transition = 2.0;
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        let opacity = |play: &GridPlay, at| -> Vec<(String, f32)> {
            play.layers(&vj, secs(at), &probe)
                .into_iter()
                .map(|l| (l.clip, l.opacity))
                .collect()
        };
        assert_eq!(opacity(&play, 0.5), [("a".to_owned(), 0.25)]);
        assert_eq!(opacity(&play, 2.0), [("a".to_owned(), 1.0)]);
        play.stop(&vj, Some("l1"), secs(10.0));
        assert_eq!(opacity(&play, 11.5), [("a".to_owned(), 0.25)]);
        assert_eq!(
            play.playing()["l1"].shown(secs(11.5)),
            Some(("a", secs(0.0)))
        );
        play.retime(&vj, secs(12.0));
        assert!(play.playing().is_empty());
    }

    #[test]
    fn retime_should_go_on_from_where_the_fade_is_at_the_new_speed() {
        let mut vj = grid();
        vj.layers[0].transition = 4.0;
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.launch(&vj, "b", secs(10.0));
        vj.layers[0].transition = 1.0;
        play.retime(&vj, secs(11.0));
        let fade = play.playing()["l1"].fading(secs(11.0)).unwrap();
        assert_eq!(fade.progress(secs(11.0)), 0.25);
        assert_eq!(fade.progress(secs(11.5)), 0.75);
        play.retime(&vj, secs(11.75));
        assert!(play.playing()["l1"].fade.is_none());
    }

    #[test]
    fn release_should_stop_only_a_piano_clip_still_playing() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.release(&vj, "a", secs(0.0));
        assert_eq!(play.playing().len(), 1);
        vj.layers[0].clips[0].trigger = Trigger::Piano;
        play.release(&vj, "a", secs(0.0));
        assert!(play.playing().is_empty());
        assert!(!play.release(&vj, "nope", secs(0.0)));
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
        assert_eq!(play.playing()["g"].clip.as_deref(), Some("logo"));
        assert!(!play.playing().contains_key("l2"));
    }
}
