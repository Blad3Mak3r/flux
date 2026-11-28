use super::*;

/// What a frame of the VJ grid is drawn from.
pub(crate) struct GridSources<'a> {
    pub grid: &'a crate::vj_commands::SharedGrid,
    pub modulation: &'a crate::modulation::SharedModulation,
    pub deck: &'a SharedDeck,
}

/// The grid at one frame: the pictures the deck shows, how the groups look, the clock and the
/// composition's effects. The output windows and the recorder both draw from it.
pub(crate) struct GridFrame {
    pub shown: Vec<flux_video::Shown>,
    pub looks: Vec<flux_video::GroupLook>,
    pub time: flux_render::Time,
    pub post: flux_render::Post,
}

/// Moves the grid's clock on to now: what is playing, the instant, and the clock the modulation
/// runs on.
pub(super) fn tick_grid(
    grid: &crate::vj_commands::SharedGrid,
    vj: &flux_project::vj::Vj,
    bpm: f64,
) -> (
    flux_video::GridPlay,
    flux_project::playhead::Tick,
    flux_project::vj_modulation::Clock,
) {
    let (play, now) = {
        let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
        let now = grid.tick(vj, bpm);
        (grid.play.clone(), now)
    };
    let clock = flux_project::vj_modulation::Clock {
        seconds: now.time.as_secs_f64(),
        beats: now.beats,
    };
    (play, now, clock)
}

/// Moves the grid's clock on and gathers what `vj` shows at `bpm` beats a minute, `dt` seconds after
/// the frame before, in a composition of `size`. `drawn` is the grid with its modulated values,
/// kept between frames and patched; it is reset when `vj` changes.
pub(crate) fn grid_frame(
    vj: &flux_project::vj::Vj,
    bpm: f64,
    sources: &GridSources<'_>,
    drawn: &mut Option<flux_project::vj::Vj>,
    size: (u32, u32),
    dt: f32,
) -> GridFrame {
    let (play, now, clock) = tick_grid(sources.grid, vj, bpm);
    let beats = now.beats;
    // What is drawn is the grid with its modulated values; the stored one is not changed.
    let drawn = sources.modulation.draw(vj, drawn, clock);
    let shown = sources
        .deck
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .map(|deck| deck.show_grid(drawn, &play, now, size))
        .unwrap_or_default();
    let post = composition_post(drawn, clock, dt);
    let mut looks = play.group_looks(drawn);
    looks.extend(play.transition_looks(drawn, now));
    GridFrame {
        shown,
        looks,
        time: flux_render::Time {
            seconds: clock.seconds,
            beats,
        },
        post,
    }
}

pub(super) fn masks_of(masks: &[flux_project::mapping::Mask]) -> Vec<Mask> {
    masks
        .iter()
        .filter(|mask| !mask.hidden)
        .map(|mask| Mask {
            points: mask.points.iter().map(|p| [p.x, p.y]).collect(),
            invert: mask.invert,
        })
        .collect()
}

pub(super) fn quad_of(slice: &flux_project::mapping::Slice) -> Quad {
    let r = &slice.input;
    Quad {
        input: [r.x, r.y, r.width, r.height],
        corners: slice.corners.map(|p| [p.x, p.y]),
        soft: [
            slice.soft_edge.left,
            slice.soft_edge.right,
            slice.soft_edge.top,
            slice.soft_edge.bottom,
        ],
        gamma: slice.soft_edge.gamma,
    }
}

/// How the groups and the layers mixing a transition are drawn `now`.
pub(super) fn looks_of(
    play: &flux_video::GridPlay,
    vj: &flux_project::vj::Vj,
    now: flux_project::playhead::Tick,
) -> Vec<flux_video::GroupLook> {
    let mut looks = play.group_looks(vj);
    looks.extend(play.transition_looks(vj, now));
    looks
}

/// The composition: the size the clips are sized into, the slices cut and the card drawn.
pub(crate) fn composition_size(session: &SharedSession) -> (u32, u32) {
    crate::video_commands::picture_size(session)
        .filter(|&(w, h)| {
            let sides = flux_project::mapping::COMPOSITION_SIDES;
            sides.contains(&w) && sides.contains(&h)
        })
        .unwrap_or(CARD_SIZE)
}

/// What to draw of `shown` (topmost first) in a composition of `composition` pixels, the bottom
/// first: from the top down to the first picture that hides everything under it. The pictures of
/// a group are drawn as one item, with the group's look in `looks`; a group that has none (it is
/// bypassed or left out by the solos) shows nothing.
pub(crate) fn items<'a>(
    shown: &'a [flux_video::Shown],
    looks: &[flux_video::GroupLook],
    composition: (u32, u32),
    time: flux_render::Time,
) -> Vec<flux_render::Item<'a>> {
    use flux_render::{Group, Item};
    let mut items = Vec::new();
    for run in shown.chunk_by(|a, b| a.layer.group == b.layer.group) {
        let (inner, mut hides) = run_pictures(run, composition, time.beats);
        match &run[0].layer.group {
            None => items.extend(inner.into_iter().rev().map(Item::Picture)),
            Some(id) => {
                let Some(look) = looks.iter().find(|look| &look.id == id) else {
                    continue;
                };
                if inner.is_empty() {
                    continue;
                }
                let effects = flux_render::Effects::new(&look.effects, time);
                hides &= look.blend == flux_project::vj::Blend::Alpha
                    && look.opacity >= 1.0
                    && effects.is_identity();
                items.push(Item::Group(Group {
                    pictures: inner,
                    opacity: look.opacity,
                    blend: blend(look.blend),
                    effects,
                }));
            }
        }
        if hides {
            break;
        }
    }
    items.reverse();
    items
}

/// The pictures of `shown` (topmost first) without groups, the bottom first.
#[cfg(test)]
pub(super) fn pictures(
    shown: &[flux_video::Shown],
    composition: (u32, u32),
    beats: f64,
) -> Vec<Picture<'_>> {
    run_pictures(shown, composition, beats).0
}

fn run_pictures(
    shown: &[flux_video::Shown],
    composition: (u32, u32),
    beats: f64,
) -> (Vec<Picture<'_>>, bool) {
    let mut pictures = Vec::new();
    let mut hides = false;
    for shown in shown {
        let Some(frame) = &shown.frame else { continue };
        let layer = &shown.layer;
        let mut hash = DefaultHasher::new();
        (&layer.clip, frame.index).hash(&mut hash);
        let place = flux_render::transformed(
            flux_render::placement(sizing(layer.fit), (frame.width, frame.height), composition),
            layer.scale,
            layer.position,
        );
        let time = flux_render::Time {
            seconds: layer.elapsed.as_secs_f64(),
            beats,
        };
        let effects = flux_render::Effects::new(&layer.effects, time);
        pictures.push(Picture {
            id: hash.finish(),
            width: frame.width,
            height: frame.height,
            rgba: &frame.rgba,
            place,
            opacity: layer.opacity,
            blend: blend(layer.blend),
            effects,
            nearest: false,
        });
        let alpha = layer.blend == flux_project::vj::Blend::Alpha;
        // Effects can leave parts of the picture clear (a zoom out, a blur's soft edge), so a
        // picture with any hides nothing below it.
        if alpha && layer.opacity >= 1.0 && !frame.alpha && covers(place) && effects.is_identity() {
            hides = true;
            break;
        }
    }
    pictures.reverse();
    (pictures, hides)
}

/// What the composition's effects of `vj` come to at `clock`, `dt` seconds after the last frame;
/// none while they are bypassed.
pub(super) fn composition_post(
    vj: &flux_project::vj::Vj,
    clock: flux_project::vj_modulation::Clock,
    dt: f32,
) -> flux_render::Post {
    if vj.composition_bypass {
        return flux_render::Post::default();
    }
    let time = flux_render::Time {
        seconds: clock.seconds,
        beats: clock.beats,
    };
    flux_render::Post::new(&vj.composition_effects, time, dt)
}
