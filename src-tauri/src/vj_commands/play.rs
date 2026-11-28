use super::*;

fn grid_of(session: &SharedSession) -> Result<Vj> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .grid_copy()
        .ok_or_else(|| ProjectError::Invalid("only a VJ project has a grid".into()))
}

/// Applies `change` to what the grid plays, with the grid as the project has it, and answers with
/// the new state.
pub(crate) fn play(
    app: &AppHandle,
    session: &SharedSession,
    grid: &SharedGrid,
    change: impl FnOnce(&mut GridPlay, &Vj, Tick) -> Result<()>,
) -> Result<GridState> {
    let vj = grid_of(session)?;
    let bpm = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .tempo_bpm();
    let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
    let before = grid.play.clone();
    grid.play.retain(&vj);
    let now = grid.tick(&vj, bpm);
    grid.play.retime(&vj, now.time);
    change(&mut grid.play, &vj, now)?;
    let answer = state(session, &vj, &grid, now, bpm);
    let advanced = grid.take_advanced();
    let changed = advanced || grid.play != before;
    drop(grid);
    // The window asks for the state all the time; the sound changes only when what plays does.
    if changed {
        crate::vj_audio::refresh(app);
    }
    Ok(answer)
}

/// Where a playing clip is and how it moves, for the window to draw its transport.
#[derive(Debug, PartialEq)]
pub(super) struct Report {
    pub(super) position: Option<f64>,
    pub(super) progress: Option<f64>,
    pub(super) remaining: Option<f64>,
    pub(super) rate: f64,
    pub(super) pass_seconds: Option<f64>,
}

impl Report {
    /// `length` is the video's, in seconds; `None` while it is not known, and for a generator.
    pub(super) fn of(head: &Playhead, tick: Tick, bpm: f64, length: Option<f64>) -> Self {
        let motion = head.motion();
        let Some(length) = length else {
            return Self {
                position: None,
                progress: None,
                remaining: None,
                rate: motion.direction.sign() * f64::from(motion.speed),
                pass_seconds: None,
            };
        };
        let u = head.phase(tick, length);
        let position = motion.place(u);
        let (start, span) = (
            f64::from(motion.region.start),
            f64::from(motion.region.span()),
        );
        let progress = position.map(|x| (x - start) / span);
        let speed = motion.rate(length, bpm);
        // Which edge of the region it moves towards: a ping-pong on its way back turns.
        let back = motion.playback == Playback::PingPong && u.rem_euclid(2.0) > 1.0;
        let towards = if back { -speed } else { speed };
        let remaining = progress
            .filter(|_| speed != 0.0)
            .map(|inside| if towards > 0.0 { 1.0 - inside } else { inside } / speed.abs());
        Self {
            position,
            progress,
            remaining,
            rate: speed * span * length,
            pass_seconds: motion.pass_seconds(length, bpm),
        }
    }
}

pub(super) fn state(
    session: &SharedSession,
    vj: &Vj,
    grid: &Grid,
    tick: Tick,
    bpm: f64,
) -> GridState {
    let now = tick.time;
    let mut playing: Vec<PlayingClip> = grid
        .play
        .playing()
        .iter()
        .filter_map(|(layer, launched)| {
            let (shown, at, head) = launched.shown(now)?;
            let elapsed = now.saturating_sub(at);
            let (_, clip) = vj.find_clip(shown)?;
            // Only the videos playing are copied, not the whole list on every poll. A generator
            // and a still image last the clip's duration.
            let duration = f64::from(clip.duration);
            let length = if let Some(generator) = &clip.generator {
                // A live input has no length: no position, progress or countdown.
                (!generator.is_endless()).then_some(duration)
            } else {
                let asset = session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .video_copy(&clip.asset);
                asset
                    .and_then(|asset| known_probe(session, &asset))
                    .map(|probe| {
                        if probe.still {
                            duration
                        } else {
                            video_seconds(Rate::new(probe.rate_num, probe.rate_den), probe.frames)
                        }
                    })
            };
            let report = Report::of(head, tick, bpm, length);
            let transition = launched.fading(now).map(|fade| fade.progress(now));
            let stopping = launched.clip.is_none();
            let fade = transition.map_or(1.0, |t| if stopping { 1.0 - t } else { t });
            let level = vj
                .layers
                .iter()
                .find(|l| &l.id == layer)
                .map_or(0.0, |l| grid.play.level(vj, l));
            Some(PlayingClip {
                level: level * fade,
                layer: layer.clone(),
                clip: shown.to_owned(),
                seconds: elapsed.as_secs_f64(),
                position: report.position,
                progress: report.progress,
                remaining: report.remaining,
                region: clip.region,
                cues: clip.cues,
                rate: report.rate,
                pass_seconds: report.pass_seconds,
                transition,
                stopping,
                line: clip
                    .generator
                    .as_ref()
                    .and_then(|generator| generator.shown_line(tick.beats))
                    .map(|line| line as u32),
            })
        })
        .collect();
    playing.sort_by(|a, b| a.layer.cmp(&b.layer));
    GridState {
        playing,
        column: grid.play.column().map(str::to_owned),
        crossfader: grid.play.crossfader(),
        beats: tick.beats,
        beats_paused: grid.paused(),
        link_peers: grid.link.as_ref().map(|link| link.peers()),
    }
}
