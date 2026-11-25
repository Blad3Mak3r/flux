//! The sound of the VJ grid: what plays on it, as voices for the running output to mix (see
//! [`flux_engine::grid_audio`]). It is rebuilt from the grid and the project whenever either
//! changes, and handed to the output without waiting for it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::SharedSession;
use crate::transport_commands::SharedOutput;
use crate::video_commands::{audio_key, load_audio, probed, read_probe};
use crate::vj_commands::SharedGrid;
use flux_audio::SharedAudio;
use flux_engine::grid_audio::{GridMix, GridVoice, Ramp};
use flux_project::VideoAsset;
use flux_project::playhead::{Playhead, Tick};
use flux_project::vj::Vj;
use flux_project::vj_modulation::Clock;

use crate::modulation::SharedModulation;

/// The videos whose sound is being decoded now.
static LOADING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Rebuilds the grid's sound and gives it to the running output, if there is one. Never waits for
/// a video: one not decoded yet is decoded on a thread of its own, which calls this again.
pub fn refresh(app: &AppHandle) {
    let output = app.state::<SharedOutput>();
    if output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_none()
    {
        return;
    }
    let session = app.state::<SharedSession>();
    let mix = build(
        app,
        &session,
        &app.state::<SharedGrid>(),
        &app.state::<SharedAudio>(),
    );
    if let Some(running) = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        running.set_grid_audio(mix);
    }
}

fn build(
    app: &AppHandle,
    session: &SharedSession,
    grid: &SharedGrid,
    cache: &SharedAudio,
) -> GridMix {
    let vj = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .grid_copy();
    // The grid as it sounds now: a layer's volume or master may be modulated.
    let bpm = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .tempo_bpm();
    let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
    let mut mix = GridMix {
        origin: grid.origin(),
        voices: Vec::new(),
    };
    let Some(vj) = vj else {
        return mix;
    };
    let tick = grid.tick(&vj, bpm);
    let now = tick.time;
    let clock = Clock {
        seconds: now.as_secs_f64(),
        beats: tick.beats,
    };
    let modulation = Arc::clone(&app.state::<SharedModulation>());
    let stored = vj;
    let vj = modulation.apply(&stored, clock);
    for (layer_id, launched) in grid.play.playing() {
        let Some(layer) = vj.layers.iter().find(|l| &l.id == layer_id) else {
            continue;
        };
        let gain = grid.play.sound_level(&vj, layer);
        let fade = launched.fading(now);
        let ramp = |rising| {
            fade.map(|fade| Ramp {
                since: fade.since,
                done: fade.done,
                over: fade.over,
                rising,
            })
        };
        let leaving = fade.and_then(|fade| fade.from.as_ref());
        let entering = launched
            .clip
            .as_deref()
            .zip(launched.head.as_ref())
            .map(|(clip, head)| (clip, launched.at, head, true));
        let leaving = leaving.map(|left| (left.clip.as_str(), left.at, &left.head, false));
        for (clip, at, head, rising) in entering.into_iter().chain(leaving) {
            let moving = Moving { head, tick, bpm };
            if let Some(voice) =
                voice(app, session, cache, &vj, clip, moving, gain).map(|voice| GridVoice {
                    key: key(layer_id, clip, at),
                    ramp: ramp(rising),
                    ..voice
                })
            {
                mix.voices.push(voice);
            }
        }
    }
    mix
}

/// What tells a clip launched on a layer apart from another, whatever its level.
fn key(layer: &str, clip: &str, at: Duration) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (layer, clip, at.as_nanos()).hash(&mut hasher);
    hasher.finish()
}

/// Where a clip is on the grid's clock, and the tempo, which a voice follows.
#[derive(Clone, Copy)]
struct Moving<'a> {
    head: &'a Playhead,
    tick: Tick,
    bpm: f64,
}

/// The sound of `clip`; `None` for a clip without sound, or until its video is read
/// and decoded.
fn voice(
    app: &AppHandle,
    session: &SharedSession,
    cache: &SharedAudio,
    vj: &Vj,
    clip: &str,
    moving: Moving<'_>,
    gain: f32,
) -> Option<GridVoice> {
    let (_, clip) = vj.find_clip(clip)?;
    if clip.generator.is_some() {
        return None;
    }
    let asset = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .video_copy(&clip.asset)?;
    let (Some(probe), Some(audio)) = (probed(&asset.id), cache.get(&audio_key(&asset.id))) else {
        prepare(app, session, &asset);
        return None;
    };
    if probe.still || audio.frames() == 0 || probe.rate_num == 0 {
        return None;
    }
    let length = probe.frames as f64 * f64::from(probe.rate_den) / f64::from(probe.rate_num);
    Some(GridVoice {
        key: 0,
        audio,
        course: moving.head.course(moving.tick, length, moving.bpm),
        length: Duration::from_secs_f64(length),
        gain,
        ramp: None,
    })
}

/// Reads and decodes a video's sound on a thread of its own, once, and then refreshes.
fn prepare(app: &AppHandle, session: &SharedSession, asset: &VideoAsset) {
    let first = LOADING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .insert(asset.id.clone());
    if !first {
        return;
    }
    let (app, session, asset) = (app.clone(), Arc::clone(session), asset.clone());
    std::thread::spawn(move || {
        let cache = app.state::<SharedAudio>();
        let ready = if read_probe(&session, &asset).is_none() {
            log::warn!("could not read video {} for its sound", asset.name);
            false
        } else {
            match load_audio(&session, &cache, &asset) {
                Ok(_) => true,
                Err(err) => {
                    log::warn!("could not read the sound of video {}: {err}", asset.name);
                    false
                }
            }
        };
        // A failure is tried again the next time a clip of the video plays.
        if let Some(set) = LOADING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            set.remove(&asset.id);
        }
        if ready {
            refresh(&app);
        }
    });
}
