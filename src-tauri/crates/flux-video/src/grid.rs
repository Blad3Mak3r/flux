//! What the VJ grid of a project shows: the clips launched by hand, one per layer, each playing
//! from the instant it was launched on a clock of its own, not the timeline's. Which clip plays is
//! the running show's, never stored in the project.
//!
//! Every function takes the time since the grid's clock started, so the picture of an instant is a
//! pure function of what was launched and when.

use std::collections::HashMap;
use std::time::Duration;

use flux_project::VideoFit;
use flux_project::playhead::{Motion, Playhead, Tick};
use flux_project::vj::{Autopilot, Blend, Side, Trigger, Vj};

use crate::layers::{GroupLook, Layer, Probe};
use crate::mapping::Rate;

/// What a layer plays: the clip launched last, and the transition into it while it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Launched {
    /// `None` while the layer fades out to nothing after it was stopped.
    pub clip: Option<String>,
    /// When the clip was launched, on the grid's clock.
    pub at: Duration,
    /// Where `clip` is in its video and how it moves; `None` with it.
    pub head: Option<Playhead>,
    /// The layer's transition into `clip`, until it ends.
    pub fade: Option<Fade>,
}

/// The clip a transition leaves behind: it keeps playing, on its own playhead, as it fades.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaving {
    pub clip: String,
    /// When it was launched.
    pub at: Duration,
    pub head: Playhead,
}

/// A transition running on a layer: from the clip it played, or from nothing, to the one launched
/// (or to nothing). It moves at the speed of the layer's transition time, which can change while
/// it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Fade {
    /// The clip leaving; `None` when the layer fades in from nothing.
    pub from: Option<Leaving>,
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

    /// The clip the layer shows `now` most, when it was launched and where it is: the one
    /// launched, or the one leaving while the layer fades out.
    pub fn shown(&self, now: Duration) -> Option<(&str, Duration, &Playhead)> {
        match &self.clip {
            Some(clip) => Some((clip.as_str(), self.at, self.head.as_ref()?)),
            None => self
                .fading(now)
                .and_then(|fade| fade.from.as_ref())
                .map(|left| (left.clip.as_str(), left.at, &left.head)),
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
    /// Where the layers' random autopilot draws from (xorshift); it moves only when one draws.
    rng: u64,
}

impl Default for GridPlay {
    fn default() -> Self {
        Self {
            playing: HashMap::new(),
            crossfader: Self::CROSSFADER_DEFAULT,
            column: None,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

/// How many seconds a clip lasts, for `length` saying how long an asset's video is and whether it
/// is a still image; a generator and a still image last the clip's duration.
fn clip_seconds(
    clip: &flux_project::vj::Clip,
    length: &dyn Fn(&str) -> Option<(f64, bool)>,
) -> Option<f64> {
    match clip.generator {
        Some(_) => Some(f64::from(clip.duration)),
        None => length(&clip.asset)
            .map(|(seconds, still)| {
                if still {
                    f64::from(clip.duration)
                } else {
                    seconds
                }
            })
            .filter(|seconds| *seconds > 0.0),
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
    pub fn launch(&mut self, vj: &Vj, clip: &str, now: impl Into<Tick>) -> bool {
        let Some((layer, _)) = vj.find_clip(clip) else {
            return false;
        };
        self.start(vj, &layer.id, Some(clip), now.into());
        self.column = None;
        true
    }

    /// Moves `layer` to `clip`, or to nothing, over the layer's transition: what it shows now
    /// leaves as the new clip comes in.
    fn start(&mut self, vj: &Vj, layer: &str, clip: Option<&str>, tick: Tick) {
        let now = tick.time;
        let over = transition(vj, layer);
        let before = self.playing.get(layer);
        let head = clip
            .and_then(|clip| vj.find_clip(clip))
            .map(|(_, clip)| Playhead::launch(Motion::of(clip), tick));
        if clip.is_some() && before.and_then(|b| b.clip.as_deref()) == clip {
            self.playing.insert(
                layer.to_owned(),
                Launched {
                    clip: clip.map(str::to_owned),
                    at: now,
                    head,
                    fade: None,
                },
            );
            return;
        }
        let from = before
            .and_then(|before| before.shown(now))
            .map(|(clip, at, head)| Leaving {
                clip: clip.to_owned(),
                at,
                head: *head,
            });
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
            head,
            fade,
        };
        self.playing.insert(layer.to_owned(), launched);
    }

    /// Lets go of `clip`: a clip triggered as a piano key stops, over the layer's transition, when
    /// it is let go, if it still plays. `false` when the grid has no such clip.
    pub fn release(&mut self, vj: &Vj, clip: &str, now: impl Into<Tick>) -> bool {
        let Some((layer, found)) = vj.find_clip(clip) else {
            return false;
        };
        if found.trigger == Trigger::Piano
            && self
                .playing
                .get(&layer.id)
                .is_some_and(|l| l.clip.as_deref() == Some(clip))
        {
            self.start(vj, &layer.id, None, now.into());
        }
        true
    }

    /// Launches every clip of `column` of any deck, all on the same instant; a deck layer without
    /// a clip in it stops, so the column is the whole picture. Global layers play on. `false` when
    /// the grid has no such column.
    pub fn launch_column(&mut self, vj: &Vj, column: &str, now: impl Into<Tick>) -> bool {
        if vj.column_place(column).is_none() {
            return false;
        }
        let tick = now.into();
        self.column = Some(column.to_owned());
        for (layer, clip) in vj.column_clips(column) {
            self.start(vj, &layer.id, clip.map(|clip| clip.id.as_str()), tick);
        }
        true
    }

    /// Launches the clips of `column` on the layers of `group` only, as a group's own column
    /// trigger does in Resolume: a deck layer of the group without a clip in it stops, the layers
    /// outside the group and the global ones play on. `false` when there is no such group or
    /// column.
    pub fn launch_group_column(
        &mut self,
        vj: &Vj,
        group: &str,
        column: &str,
        now: impl Into<Tick>,
    ) -> bool {
        if vj.group(group).is_none() || vj.column_place(column).is_none() {
            return false;
        }
        let tick = now.into();
        for (layer, clip) in vj.column_clips(column) {
            if layer.group.as_deref() == Some(group) {
                self.start(vj, &layer.id, clip.map(|clip| clip.id.as_str()), tick);
            }
        }
        true
    }

    /// Stops the layer, or every layer with `None`, each over its transition.
    pub fn stop(&mut self, vj: &Vj, layer: Option<&str>, now: impl Into<Tick>) {
        let tick = now.into();
        self.column = None;
        let layers: Vec<String> = match layer {
            Some(layer) => vec![layer.to_owned()],
            None => self.playing.keys().cloned().collect(),
        };
        for layer in layers {
            if self.playing.get(&layer).is_some_and(|l| l.clip.is_some()) {
                self.start(vj, &layer, None, tick);
            }
        }
    }

    /// Moves the playhead of the clip playing on `layer` to `place`, a fraction of the clip's
    /// length brought into its region. `false` when the layer plays no clip.
    pub fn scrub(&mut self, layer: &str, place: f64, now: impl Into<Tick>) -> bool {
        let Some(head) = self
            .playing
            .get_mut(layer)
            .filter(|launched| launched.clip.is_some())
            .and_then(|launched| launched.head.as_mut())
        else {
            return false;
        };
        head.jump(now.into(), place);
        true
    }

    /// Plays `clip` from its cue number `index`: the playhead of the clip jumps there if it is
    /// playing, and it is launched there if it is not. `None` when the grid has no such clip, or
    /// the cue is empty.
    pub fn cue(&mut self, vj: &Vj, clip: &str, index: usize, now: impl Into<Tick>) -> Option<()> {
        let tick = now.into();
        let (layer, found) = vj.find_clip(clip)?;
        let place = found.cues.get(index).copied().flatten()?;
        let playing = self
            .playing
            .get(&layer.id)
            .is_some_and(|launched| launched.clip.as_deref() == Some(clip));
        if !playing {
            self.launch(vj, clip, tick);
        }
        self.scrub(&layer.id, f64::from(place), tick);
        Some(())
    }

    /// Follows what the clips playing are now: a change of a clip's speed, direction, tempo,
    /// region or playback anchors its playhead again from where it is, so it never jumps. `length`
    /// says how many seconds a video lasts and whether it is a still image, by asset; the length
    /// of a still image or a generator is the clip's duration. A clip whose video is not known yet
    /// waits.
    pub fn follow(&mut self, vj: &Vj, tick: Tick, length: &dyn Fn(&str) -> Option<(f64, bool)>) {
        let seconds = |clip: &flux_project::vj::Clip| clip_seconds(clip, length);
        for (layer, launched) in &mut self.playing {
            let Some(layer) = vj.layers.iter().find(|l| &l.id == layer) else {
                continue;
            };
            let mut heads = vec![];
            if let (Some(id), Some(head)) = (&launched.clip, &mut launched.head) {
                heads.push((id.as_str(), head));
            }
            if let Some(left) = launched.fade.as_mut().and_then(|f| f.from.as_mut()) {
                heads.push((left.clip.as_str(), &mut left.head));
            }
            for (id, head) in heads {
                let Some(clip) = layer.clips.iter().find(|c| c.id == id) else {
                    continue;
                };
                let motion = Motion::of(clip);
                if motion != *head.motion()
                    && let Some(seconds) = seconds(clip)
                {
                    head.retime(motion, tick, seconds);
                }
            }
        }
    }

    /// Launches, on each layer whose autopilot is on, the clip that follows the one playing when
    /// that one has played a whole pass (see [`Playhead::pass_ended`]), through [`Self::launch`] so
    /// the layer's transition applies. A clip whose length is not known yet waits. `true` when a
    /// clip was launched.
    pub fn autopilot(
        &mut self,
        vj: &Vj,
        tick: impl Into<Tick>,
        length: &dyn Fn(&str) -> Option<(f64, bool)>,
    ) -> bool {
        let tick = tick.into();
        let mut due = Vec::new();
        for layer in vj.layers.iter().filter(|l| l.autopilot != Autopilot::Off) {
            let Some((Some(id), Some(head))) = self
                .playing
                .get(&layer.id)
                .map(|launched| (&launched.clip, &launched.head))
            else {
                continue;
            };
            let Some(clip) = layer.clips.iter().find(|c| &c.id == id) else {
                continue;
            };
            if clip_seconds(clip, length).is_some_and(|seconds| head.pass_ended(tick, seconds)) {
                due.push((layer, id.clone()));
            }
        }
        let mut launched = false;
        for (layer, id) in due {
            // Only the random pick moves the generator.
            let roll = if layer.autopilot == Autopilot::Random {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 7;
                self.rng ^= self.rng << 17;
                self.rng ^ tick.time.as_nanos() as u64
            } else {
                0
            };
            if let Some(next) = vj.autopilot_next(layer, &id, roll) {
                launched |= self.launch(vj, &next.id, tick);
            }
        }
        launched
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

    /// Where the crossfader starts and goes back to: the middle, where both sides show fully.
    pub const CROSSFADER_DEFAULT: f32 = 0.5;

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
            for id in launched.clip.iter().chain(leaving.map(|left| &left.clip)) {
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
                && fade.from.as_ref().is_some_and(|left| !has(&left.clip))
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
    pub fn layers(&self, vj: &Vj, now: impl Into<Tick>, probe: Probe<'_>) -> Vec<Layer> {
        let tick = now.into();
        let now = tick.time;
        let mut shown = Vec::new();
        for layer in vj.layers.iter().rev() {
            let Some(launched) = self.playing.get(&layer.id) else {
                continue;
            };
            let opacity = self.level(vj, layer);
            if opacity <= 0.0 {
                continue;
            }
            let picture = |clip: &str,
                           at: Duration,
                           head: &Playhead,
                           opacity: f32,
                           blend: Blend,
                           group: Option<String>| {
                let clip = layer.clips.iter().find(|c| c.id == clip)?;
                let elapsed = now.saturating_sub(at);
                // A generator fills the composition and lasts its duration: its time is the place in
                // it, in milliseconds, so a loop starts its strobe and noise over.
                let length = f64::from(clip.duration);
                let (frame, fit) = match &clip.generator {
                    Some(_) => {
                        let place = head.at(tick, length)?;
                        ((place * length * 1000.0) as u64, VideoFit::Stretch)
                    }
                    None => {
                        let (rate, frames) = probe(&clip.asset)?;
                        (frame_at(head, tick, rate, frames, length)?, clip.fit)
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
                    blend,
                    scale: clip.scale,
                    position: clip.position,
                    effects: clip
                        .effects
                        .iter()
                        .chain(&layer.effects)
                        .filter(|effect| !effect.bypass)
                        .cloned()
                        .collect(),
                    elapsed,
                    beats: tick.beats,
                    group,
                })
            };
            let fading = launched.fading(now);
            let t = fading.map_or(1.0, |fade| fade.progress(now));
            // Both clips of a blend transition are mixed in a group of their own, drawn like the
            // layer would be; the new one is drawn with the transition's blend.
            let mixed = Self::mixes(layer, launched, now);
            let group = || {
                mixed
                    .then(|| mix_id(&layer.id))
                    .or_else(|| layer.group.clone())
            };
            let (new_blend, new_opacity) = if mixed {
                (layer.transition_effect, t)
            } else {
                (layer.blend, opacity * t)
            };
            if let (Some(clip), Some(head)) = (&launched.clip, &launched.head) {
                shown.extend(picture(
                    clip,
                    launched.at,
                    head,
                    new_opacity,
                    new_blend,
                    group(),
                ));
            }
            if let Some(left) = fading.and_then(|fade| fade.from.as_ref()) {
                let (old, blend) = match (mixed, layer.transition_effect) {
                    (true, _) => (1.0 - t, Blend::Alpha),
                    (false, Blend::Alpha) if launched.clip.is_some() => (opacity, layer.blend),
                    _ => (opacity * (1.0 - t), layer.blend),
                };
                shown.extend(picture(
                    &left.clip,
                    left.at,
                    &left.head,
                    old,
                    blend,
                    group(),
                ));
            }
        }
        shown
    }

    /// Whether `layer` mixes the clip leaving and the one arriving in a group of their own: it
    /// does with a transition other than `Alpha`, outside a group (a group does not nest, there
    /// the new clip is blended over the old one flat).
    fn mixes(layer: &flux_project::vj::Layer, launched: &Launched, now: Duration) -> bool {
        layer.transition_effect != Blend::Alpha
            && layer.group.is_none()
            && launched.clip.is_some()
            && launched.fading(now).is_some_and(|fade| fade.from.is_some())
    }

    /// How the layers whose transition mixes two clips are drawn `now`: each mix is drawn over
    /// what is below with the layer's level, blend and no effects (the clips keep theirs).
    pub fn transition_looks(&self, vj: &Vj, now: impl Into<Tick>) -> Vec<GroupLook> {
        let now = now.into().time;
        vj.layers
            .iter()
            .filter_map(|layer| {
                let launched = self.playing.get(&layer.id)?;
                Self::mixes(layer, launched, now).then(|| GroupLook {
                    id: mix_id(&layer.id),
                    opacity: self.level(vj, layer),
                    blend: layer.blend,
                    effects: Vec::new(),
                })
            })
            .collect()
    }

    /// Whether `layer` is silenced: it or its group is bypassed, or something is soloed and neither
    /// it nor its group is.
    fn muted(vj: &Vj, layer: &flux_project::vj::Layer) -> bool {
        layer.bypass || vj.group_of(layer).is_some_and(|group| group.bypass) || !vj.solo_lets(layer)
    }

    /// How much of `layer` shows, 0 to 1: its master, opacity and crossfader side, or nothing when it
    /// is bypassed or another layer is soloed and it is not (the same for its group). The group's
    /// own master and opacity are applied to the mix of its layers, not to each (see
    /// [`Self::group_looks`]).
    pub fn level(&self, vj: &Vj, layer: &flux_project::vj::Layer) -> f32 {
        if Self::muted(vj, layer) {
            return 0.0;
        }
        layer.master * layer.opacity * self.side_level(layer.crossfade)
    }

    /// How loud the sound of `layer` is, 0 to 1: its volume, master and crossfader side times its
    /// group's master, or nothing when it is bypassed or another layer is soloed and it is not (the
    /// same for its group). The opacity is the picture's.
    pub fn sound_level(&self, vj: &Vj, layer: &flux_project::vj::Layer) -> f32 {
        if Self::muted(vj, layer) {
            return 0.0;
        }
        let group = vj.group_of(layer).map_or(1.0, |group| group.master);
        layer.volume * layer.master * group * self.side_level(layer.crossfade)
    }

    /// How each group that shows is drawn: its mix of the layers is drawn with its master times its
    /// opacity, its blend and its effects. A group bypassed, or left out by the solos, has none.
    pub fn group_looks(&self, vj: &Vj) -> Vec<GroupLook> {
        vj.groups
            .iter()
            .filter(|group| {
                !group.bypass
                    && (!vj.any_solo()
                        || group.solo
                        || vj.group_layers(&group.id).any(|layer| layer.solo))
            })
            .map(|group| GroupLook {
                id: group.id.clone(),
                opacity: group.master * group.opacity,
                blend: group.blend,
                effects: group
                    .effects
                    .iter()
                    .filter(|effect| !effect.bypass)
                    .cloned()
                    .collect(),
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

/// The id of the group a layer's transition mix is drawn as.
fn mix_id(layer: &str) -> String {
    format!("transition:{layer}")
}

/// How many seconds a video of `frames` at `rate` lasts.
pub fn video_seconds(rate: Rate, frames: u64) -> f64 {
    frames as f64 * f64::from(rate.den) / f64::from(rate.num.max(1))
}

/// Which frame of a video of `frames` at `rate` the playhead shows at `tick`, or `None` once a
/// clip played once has ended. A still image (one frame) lasts `still` seconds, and shows its one
/// frame until then.
pub fn frame_at(head: &Playhead, tick: Tick, rate: Rate, frames: u64, still: f64) -> Option<u64> {
    if frames == 0 || rate.num == 0 || rate.den == 0 {
        return None;
    }
    if frames == 1 {
        return head.at(tick, still).map(|_| 0);
    }
    let place = head.at(tick, video_seconds(rate, frames))?;
    // A little over, so that a place that is a whole frame in theory is not the frame before it.
    Some(((place * frames as f64 + 1e-6).floor() as u64).min(frames - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::vj::{Blend, Clip, Column, Deck, Layer as GridLayer, Playback};

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
            speed: 1.0,
            duration: flux_project::vj::DEFAULT_DURATION,
            direction: Default::default(),
            tempo: Default::default(),
            region: Default::default(),
            cues: Default::default(),
            fit: Default::default(),
            scale: 1.0,
            position: [0.0, 0.0],
            effects: Vec::new(),
            modulation: Default::default(),
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
            transition_effect: Blend::Alpha,
            autopilot: Autopilot::Off,
            crossfade,
            bypass: false,
            solo: false,
            collapsed: false,
            group: None,
            effects: Vec::new(),
            modulation: Default::default(),
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
            ..Default::default()
        }
    }

    fn probe(_: &str) -> Option<(Rate, u64)> {
        Some((RATE, 100))
    }

    #[test]
    fn sound_level_should_follow_volume_master_sides_bypass_and_solo() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.set_crossfader(0.5);
        vj.layers[0].volume = 0.5;
        vj.layers[0].master = 0.5;
        vj.layers[0].opacity = 0.1;
        assert_eq!(
            play.sound_level(&vj, &vj.layers[0]),
            0.25,
            "not the opacity"
        );
        play.set_crossfader(1.0);
        assert_eq!(play.sound_level(&vj, &vj.layers[0]), 0.0, "side A is out");
        assert_eq!(play.sound_level(&vj, &vj.layers[1]), 1.0);
        play.set_crossfader(0.75);
        assert_eq!(play.sound_level(&vj, &vj.layers[0]), 0.125);
        vj.layers[1].bypass = true;
        assert_eq!(play.sound_level(&vj, &vj.layers[1]), 0.0);
        vj.layers[1].bypass = false;
        vj.layers[1].solo = true;
        assert_eq!(
            play.sound_level(&vj, &vj.layers[0]),
            0.0,
            "another is soloed"
        );
        assert_eq!(play.sound_level(&vj, &vj.layers[1]), 1.0);
    }

    fn frame(seconds: f64, playback: Playback) -> Option<u64> {
        let head = Playhead::launch(
            Motion {
                speed: 1.0,
                direction: Default::default(),
                tempo: Default::default(),
                region: Default::default(),
                playback,
            },
            Tick::default(),
        );
        frame_at(&head, secs(seconds).into(), RATE, 100, 10.0)
    }

    #[test]
    fn frame_at_should_loop_stop_or_bounce_at_the_end() {
        assert_eq!(frame(1.0, Playback::Loop), Some(25));
        assert_eq!(frame(4.4, Playback::Loop), Some(10));
        assert_eq!(frame(3.96, Playback::Once), Some(99));
        assert_eq!(frame(4.0, Playback::Once), None);
        assert_eq!(frame(3.0, Playback::PingPong), Some(75));
        assert_eq!(frame(4.2, Playback::PingPong), Some(95), "back again");
        assert_eq!(frame(7.92, Playback::PingPong), Some(2));
        assert_eq!(frame(8.0, Playback::PingPong), Some(0));
        assert_eq!(frame(8.2, Playback::PingPong), Some(5));
    }

    #[test]
    fn a_still_image_should_last_its_duration_and_then_follow_its_playback() {
        let still = |playback, seconds: f64| {
            let head = Playhead::launch(
                Motion {
                    speed: 1.0,
                    direction: Default::default(),
                    tempo: Default::default(),
                    region: Default::default(),
                    playback,
                },
                Tick::default(),
            );
            frame_at(&head, secs(seconds).into(), Rate::new(25, 1), 1, 3.0)
        };
        assert_eq!(still(Playback::Once, 2.9), Some(0));
        assert_eq!(still(Playback::Once, 3.1), None, "ends after its duration");
        assert_eq!(still(Playback::Loop, 100.0), Some(0), "restarts for ever");
        assert_eq!(still(Playback::PingPong, 100.0), Some(0));
    }

    #[test]
    fn a_generator_should_last_its_duration_then_end_or_start_over() {
        let mut vj = grid();
        let generator = &mut vj.layers[0].clips[1];
        generator.generator = Some(flux_project::generator::Generator::Bars);
        generator.asset = String::new();
        generator.duration = 3.0;
        generator.playback = Playback::Once;
        let mut play = GridPlay::default();
        play.launch(&vj, "b", secs(0.0));
        assert_eq!(play.layers(&vj, secs(2.0), &probe)[0].frame, 2000);
        assert!(play.layers(&vj, secs(3.5), &probe).is_empty());
        vj.layers[0].clips[1].playback = Playback::Loop;
        play.follow(&vj, secs(3.5).into(), &|_| None);
        let frame = play.layers(&vj, secs(3.5), &probe)[0].frame;
        assert!((499..=500).contains(&frame), "a loop starts over: {frame}");
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
    fn a_generator_should_play_with_no_video() {
        let mut vj = grid();
        let clip = &mut vj.layers[0].clips[1];
        clip.asset.clear();
        clip.generator = Some(flux_project::generator::Generator::Bars);
        let mut play = GridPlay::default();
        play.launch(&vj, "b", secs(0.0));
        assert!(play.clips(&vj, secs(1.0)).is_empty(), "no video to open");
        let none = |_: &str| None;
        let shown = play.layers(&vj, secs(4.0), &none);
        assert_eq!(shown.len(), 1, "played once, still within its duration");
        assert_eq!(shown[0].frame, 4000);
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
    fn a_picture_should_carry_its_clips_effects_then_its_layers_without_the_bypassed() {
        use flux_project::vj::{Effect, EffectKind};
        let effect = |id: &str, kind, bypass| Effect {
            id: id.into(),
            bypass,
            modulation: Default::default(),
            kind,
        };
        let mut vj = grid();
        vj.layers[0].effects = vec![
            effect("e3", EffectKind::Invert { amount: 1.0 }, false),
            effect("e4", EffectKind::Blur { radius: 0.5 }, true),
        ];
        vj.layers[0].clips[0].effects = vec![
            effect("e1", EffectKind::Brightness { amount: 0.2 }, false),
            effect("e2", EffectKind::Zoom { amount: 2.0 }, true),
        ];
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.launch(&vj, "b", secs(0.0));
        let shown = play.layers(&vj, secs(1.0), &probe);
        let kinds = |layer: &crate::Layer| -> Vec<EffectKind> {
            layer.effects.iter().map(|e| e.kind.clone()).collect()
        };
        assert_eq!(
            kinds(&shown[0]),
            [EffectKind::Invert { amount: 1.0 }],
            "the layer's effects reach a clip with none of its own"
        );
        assert_eq!(shown[0].elapsed, secs(1.0));
        play.launch(&vj, "a", secs(1.0));
        let shown = play.layers(&vj, secs(1.5), &probe);
        assert_eq!(
            kinds(&shown[0]),
            [
                EffectKind::Brightness { amount: 0.2 },
                EffectKind::Invert { amount: 1.0 }
            ]
        );
        // The effects carry their ids, which seed their randomness.
        assert_eq!(shown[0].effects[0].id, "e1");
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
        vj.layers[0].transition_effect = Blend::Add;
        let shown = play.layers(&vj, secs(10.5), &probe);
        assert_eq!(shown[1].opacity, 0.75);
        assert_eq!(play.layers(&vj, secs(12.0), &probe).len(), 1);
        assert_eq!(play.clips(&vj, secs(12.0)), [("b", "v1")]);
    }

    #[test]
    fn a_blend_transition_should_mix_both_clips_in_a_group_of_its_own() {
        let mut vj = grid();
        vj.layers[0].transition = 2.0;
        vj.layers[0].blend = Blend::Screen;
        vj.layers[0].transition_effect = Blend::Multiply;
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.launch(&vj, "b", secs(10.0));
        for (at, new, old) in [(10.2, 0.1, 0.9), (11.0, 0.5, 0.5), (11.9, 0.95, 0.05)] {
            let shown = play.layers(&vj, secs(at), &probe);
            let seen: Vec<_> = shown
                .iter()
                .map(|l| (l.clip.as_str(), l.blend, l.group.as_deref()))
                .collect();
            let id = Some("transition:l1");
            assert_eq!(seen, [("b", Blend::Multiply, id), ("a", Blend::Alpha, id)]);
            assert!(
                (shown[0].opacity - new).abs() < 1e-3,
                "{at}: {}",
                shown[0].opacity
            );
            assert!(
                (shown[1].opacity - old).abs() < 1e-3,
                "{at}: {}",
                shown[1].opacity
            );
        }
        let looks = play.transition_looks(&vj, secs(11.0));
        assert_eq!(looks.len(), 1);
        assert_eq!(
            (looks[0].id.as_str(), looks[0].blend),
            ("transition:l1", Blend::Screen)
        );
        // Over, the new clip shows alone, drawn as the layer.
        let shown = play.layers(&vj, secs(12.5), &probe);
        assert_eq!(
            (shown.len(), shown[0].blend, shown[0].group.clone()),
            (1, Blend::Screen, None)
        );
        assert!(play.transition_looks(&vj, secs(12.5)).is_empty());
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
        let shown = play.playing()["l1"].shown(secs(11.5)).unwrap();
        assert_eq!((shown.0, shown.1), ("a", secs(0.0)));
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

    #[test]
    fn a_modulated_grid_should_draw_the_modulated_values_and_leave_the_stored_ones() {
        use flux_project::vj::{Effect, EffectKind};
        use flux_project::vj_effects::{Pace, Sync};
        use flux_project::vj_modulation::{
            self as modulation, Bands, Clock, Modulation, Shape, Source,
        };
        let ramp = |min, max| Modulation {
            source: Source::Lfo {
                shape: Shape::SawUp,
                pace: Pace {
                    sync: Sync::Beats,
                    rate: 0.0,
                    beats: 1.0,
                },
                phase: 0.0,
            },
            min,
            max,
        };
        let mut vj = grid();
        vj.layers[0]
            .modulation
            .insert("opacity".into(), ramp(0.0, 1.0));
        vj.layers[0].clips[0]
            .modulation
            .insert("scale".into(), ramp(1.0, 3.0));
        vj.layers[0].clips[0].effects = vec![Effect {
            id: "e1".into(),
            bypass: false,
            modulation: [("radius".to_owned(), ramp(0.0, 1.0))].into(),
            kind: EffectKind::Blur { radius: 0.1 },
        }];
        let stored = vj.clone();
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        let clock = Clock {
            seconds: 1.0,
            beats: 0.5,
        };
        let drawn = modulation::modulate(&vj, &mut Default::default(), clock, Bands::default());
        let shown = play.layers(&drawn, secs(1.0), &probe);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].opacity, 0.5);
        assert_eq!(shown[0].scale, 2.0);
        assert_eq!(shown[0].effects[0].kind, EffectKind::Blur { radius: 0.5 });
        assert_eq!(vj, stored, "what is stored is never changed by modulation");
        // Drawn from the stored grid, the stored values show.
        let plain = play.layers(&vj, secs(1.0), &probe);
        assert_eq!((plain[0].opacity, plain[0].scale), (1.0, 1.0));
    }

    fn frames(play: &GridPlay, vj: &Vj, tick: Tick) -> Vec<(String, u64)> {
        play.layers(vj, tick, &probe)
            .into_iter()
            .map(|l| (l.clip, l.frame))
            .collect()
    }

    fn beat(seconds: f64, beats: f64) -> Tick {
        Tick {
            time: secs(seconds),
            beats,
        }
    }

    #[test]
    fn a_clip_should_play_reverse_paused_and_fast_without_jumping_when_it_changes() {
        let mut vj = grid();
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        let length = |_: &str| Some((4.0, false));
        assert_eq!(frames(&play, &vj, secs(1.0).into()), [("a".into(), 25)]);
        // Twice as fast from 1 s on: no jump at 1 s.
        vj.layers[0].clips[0].speed = 2.0;
        play.follow(&vj, secs(1.0).into(), &length);
        assert_eq!(frames(&play, &vj, secs(1.0).into()), [("a".into(), 25)]);
        assert_eq!(frames(&play, &vj, secs(1.5).into()), [("a".into(), 50)]);
        vj.layers[0].clips[0].direction = flux_project::vj::Direction::Reverse;
        play.follow(&vj, secs(1.5).into(), &length);
        assert_eq!(frames(&play, &vj, secs(1.5).into()), [("a".into(), 50)]);
        assert_eq!(frames(&play, &vj, secs(2.0).into()), [("a".into(), 25)]);
        vj.layers[0].clips[0].direction = flux_project::vj::Direction::Pause;
        play.follow(&vj, secs(2.0).into(), &length);
        assert_eq!(frames(&play, &vj, secs(60.0).into()), [("a".into(), 25)]);
    }

    #[test]
    fn a_clip_launched_in_reverse_should_start_at_the_end_of_its_region() {
        let mut vj = grid();
        let clip = &mut vj.layers[0].clips[0];
        clip.direction = flux_project::vj::Direction::Reverse;
        clip.region = flux_project::vj::Region {
            start: 0.25,
            end: 0.5,
        };
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        assert_eq!(frames(&play, &vj, secs(0.0).into()), [("a".into(), 50)]);
        assert_eq!(frames(&play, &vj, secs(0.5).into()), [("a".into(), 37)]);
        // The pass is 1 s long: it wraps to the end.
        assert_eq!(frames(&play, &vj, secs(1.02).into()), [("a".into(), 49)]);
    }

    #[test]
    fn a_pass_of_beats_should_follow_the_beat_count() {
        let mut vj = grid();
        vj.layers[0].clips[0].tempo = flux_project::vj::Tempo::Beats { beats: 4 };
        let mut play = GridPlay::default();
        play.launch(&vj, "a", beat(0.0, 0.0));
        // The first two beats take 1 s; the tempo then halves, so the next two take 4 s.
        assert_eq!(frames(&play, &vj, beat(1.0, 2.0)), [("a".into(), 50)]);
        assert_eq!(frames(&play, &vj, beat(3.0, 3.0)), [("a".into(), 75)]);
        assert_eq!(frames(&play, &vj, beat(5.0, 4.0)), [("a".into(), 0)]);
    }

    #[test]
    fn scrub_and_cue_should_move_the_playhead_and_launch_a_clip_that_is_not_playing() {
        let mut vj = grid();
        vj.layers[0].clips[0].cues[1] = Some(0.8);
        let mut play = GridPlay::default();
        assert!(!play.scrub("l1", 0.5, secs(0.0)), "nothing plays there");
        assert_eq!(play.cue(&vj, "a", 0, secs(0.0)), None, "an empty cue");
        assert_eq!(play.cue(&vj, "nope", 1, secs(0.0)), None);
        assert_eq!(play.cue(&vj, "a", 1, secs(10.0)), Some(()));
        assert_eq!(frames(&play, &vj, secs(10.0).into()), [("a".into(), 80)]);
        assert!(play.scrub("l1", 0.1, secs(11.0)));
        assert_eq!(frames(&play, &vj, secs(11.0).into()), [("a".into(), 10)]);
        assert_eq!(frames(&play, &vj, secs(12.0).into()), [("a".into(), 35)]);
        assert_eq!(play.cue(&vj, "a", 1, secs(12.0)), Some(()));
        assert_eq!(frames(&play, &vj, secs(12.0).into()), [("a".into(), 80)]);
    }

    #[test]
    fn a_generator_should_run_on_the_speed_and_direction_of_its_playhead() {
        let mut vj = grid();
        vj.layers[0].clips[1].generator = Some(flux_project::generator::Generator::Bars);
        vj.layers[0].clips[1].asset = String::new();
        vj.layers[0].clips[1].speed = 2.0;
        let mut play = GridPlay::default();
        play.launch(&vj, "b", secs(1.0));
        let shown = play.layers(&vj, secs(3.0), &probe);
        assert_eq!(shown[0].frame, 4000, "milliseconds of its own time");
        assert_eq!(shown[0].elapsed, secs(2.0), "effects keep real time");
    }

    #[test]
    fn the_clip_a_transition_leaves_should_keep_its_playhead() {
        let mut vj = grid();
        vj.layers[0].transition = 4.0;
        vj.layers[0].clips[0].speed = 1.5;
        let mut play = GridPlay::default();
        play.launch(&vj, "a", secs(0.0));
        play.launch(&vj, "b", secs(1.0));
        let shown = play.layers(&vj, secs(2.0), &probe);
        let old = shown.iter().find(|l| l.clip == "a").unwrap();
        assert_eq!(old.frame, 75, "1.5 times as fast for 2 s of a 4 s video");
    }

    #[test]
    fn autopilot_should_launch_the_next_clip_at_the_end_of_a_pass_and_wrap() {
        let mut vj = grid();
        vj.layers[0].autopilot = Autopilot::Next;
        let length = |_: &str| Some((10.0, false));
        let playing = |play: &GridPlay| play.playing()["l1"].clip.clone();
        let mut play = GridPlay::default();
        play.launch(&vj, "a", Duration::ZERO);
        assert!(!play.autopilot(&vj, secs(9.9), &length));
        assert_eq!(playing(&play).as_deref(), Some("a"));
        assert!(play.autopilot(&vj, secs(10.0), &length));
        assert_eq!(playing(&play).as_deref(), Some("b"));
        assert!(!play.autopilot(&vj, secs(19.9), &length), "b started at 10");
        assert!(play.autopilot(&vj, secs(20.0), &length));
        assert_eq!(playing(&play).as_deref(), Some("a"));
        assert!(
            !play.playing().contains_key("l2"),
            "a layer without autopilot is left alone"
        );
    }

    #[test]
    fn autopilot_should_stay_off_until_a_clip_is_launched_after_a_stop() {
        let mut vj = grid();
        vj.layers[0].autopilot = Autopilot::Random;
        let length = |_: &str| Some((10.0, false));
        let mut play = GridPlay::default();
        play.launch(&vj, "a", Duration::ZERO);
        play.stop(&vj, Some("l1"), secs(1.0));
        assert!(!play.autopilot(&vj, secs(30.0), &length));
        play.launch(&vj, "a", secs(30.0));
        for pass in 1..=6 {
            let before = play.playing()["l1"].clip.clone();
            assert!(play.autopilot(&vj, secs(30.0 + 10.0 * f64::from(pass)), &length));
            assert_ne!(play.playing()["l1"].clip, before, "no repeat");
        }
    }

    fn group(id: &str) -> flux_project::vj::Group {
        flux_project::vj::Group {
            id: id.into(),
            name: None,
            master: 1.0,
            opacity: 1.0,
            blend: Blend::Alpha,
            bypass: false,
            solo: false,
            collapsed: false,
            effects: Vec::new(),
        }
    }

    /// `grid()` with both layers in one group, off the crossfader.
    fn grouped() -> Vj {
        let mut vj = grid();
        for layer in &mut vj.layers {
            layer.crossfade = None;
            layer.group = Some("g".into());
        }
        vj.groups = vec![group("g")];
        vj
    }

    #[test]
    fn a_group_should_scale_the_sound_of_its_layers_by_its_master_and_mute_them_when_bypassed() {
        let mut vj = grouped();
        let play = GridPlay::default();
        vj.groups[0].master = 0.5;
        vj.groups[0].opacity = 0.2;
        assert_eq!(play.sound_level(&vj, &vj.layers[0]), 0.5);
        assert_eq!(
            play.level(&vj, &vj.layers[0]),
            1.0,
            "the group draws its mix"
        );
        vj.groups[0].bypass = true;
        assert_eq!(play.sound_level(&vj, &vj.layers[1]), 0.0);
        assert_eq!(play.level(&vj, &vj.layers[1]), 0.0);
    }

    #[test]
    fn a_solo_should_let_a_soloed_group_or_layer_through_and_hold_the_rest_back() {
        let mut vj = grid();
        vj.layers[0].crossfade = None;
        vj.layers[1].crossfade = None;
        vj.layers[1].group = Some("g".into());
        vj.groups = vec![group("g")];
        let play = GridPlay::default();
        vj.groups[0].solo = true;
        assert_eq!(play.level(&vj, &vj.layers[0]), 0.0);
        assert_eq!(play.level(&vj, &vj.layers[1]), 1.0);
        assert_eq!(play.group_looks(&vj).len(), 1);
        vj.groups[0].solo = false;
        vj.layers[0].solo = true;
        assert_eq!(play.level(&vj, &vj.layers[1]), 0.0);
        assert!(
            play.group_looks(&vj).is_empty(),
            "nothing of the group shows"
        );
    }

    #[test]
    fn the_look_of_a_group_should_carry_its_master_times_opacity_blend_and_effects() {
        let mut vj = grouped();
        vj.groups[0].master = 0.5;
        vj.groups[0].opacity = 0.5;
        vj.groups[0].blend = Blend::Add;
        let looks = GridPlay::default().group_looks(&vj);
        assert_eq!(looks.len(), 1);
        assert_eq!((looks[0].opacity, looks[0].blend), (0.25, Blend::Add));
        vj.groups[0].bypass = true;
        assert!(GridPlay::default().group_looks(&vj).is_empty());
    }

    #[test]
    fn the_pictures_of_a_group_should_say_which_group_they_are_in() {
        let vj = grouped();
        let mut play = GridPlay::default();
        play.launch_column(&vj, "c1", secs(0.0));
        let shown = play.layers(&vj, secs(1.0), &probe);
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().all(|l| l.group.as_deref() == Some("g")));
    }

    #[test]
    fn a_group_column_should_launch_only_the_layers_of_the_group() {
        let mut vj = grid();
        vj.layers[0].group = Some("g".into());
        vj.groups = vec![group("g")];
        let mut play = GridPlay::default();
        assert!(play.launch_group_column(&vj, "g", "c1", secs(0.0)));
        assert_eq!(play.playing()["l1"].clip.as_deref(), Some("a"));
        assert!(!play.playing().contains_key("l2"), "outside the group");
        assert!(play.launch_group_column(&vj, "g", "c2", secs(1.0)));
        assert_eq!(play.playing()["l1"].clip.as_deref(), Some("b"));
        assert!(!play.launch_group_column(&vj, "nope", "c1", secs(1.0)));
        assert!(!play.launch_group_column(&vj, "g", "nope", secs(1.0)));
    }
}
