use super::*;

/// Builds the clip set of a timeline of `project`. `load` supplies the decoded audio of an asset; an asset it
/// cannot supply is skipped with a warning, because a missing file must never stop the show.
pub fn build(
    project: &Project,
    timeline: &Timeline,
    load: impl FnMut(&AudioAsset) -> Result<Arc<Decoded>, AudioError>,
) -> Programme {
    build_with_video(project, timeline, load, |_| Ok(None))
}

/// Like [`build`], with the sound of the video tracks too: `load_video` supplies the decoded sound
/// of a video, or `None` for one without sound. A video clip sounds like an audio clip at 0 dB
/// with no fades, through its track's fader and output.
pub fn build_with_video(
    project: &Project,
    timeline: &Timeline,
    mut load: impl FnMut(&AudioAsset) -> Result<Arc<Decoded>, AudioError>,
    mut load_video: impl FnMut(&VideoAsset) -> Result<Option<Arc<Decoded>>, AudioError>,
) -> Programme {
    // Keep one buffer per asset in this programme even when the bounded global cache evicts it.
    let mut decoded = std::collections::HashMap::<String, Arc<Decoded>>::new();
    let mut load = |asset: &AudioAsset| -> Result<Arc<Decoded>, AudioError> {
        if let Some(audio) = decoded.get(&asset.id) {
            return Ok(Arc::clone(audio));
        }
        let audio = load(asset)?;
        decoded.insert(asset.id.clone(), Arc::clone(&audio));
        Ok(audio)
    };
    let clock = Clock::new(ENGINE_RATE, timeline.timecode.frame_rate);
    let mut clips = Vec::new();
    let mut tracks = Vec::new();
    let mut warnings = Vec::new();
    let mut failed: Vec<&str> = Vec::new();
    let mut operator_tracks = Vec::new();

    for track in timeline.tracks.iter().filter(|track| !track.muted) {
        if let TrackKind::Operator { orders, .. } = &track.kind {
            let index = tracks.len();
            operator_tracks.push(index);
            tracks.push((
                track.id.clone(),
                track.gain_db.map_or(1.0, track_gain_from_db),
            ));
            for order in orders {
                let Some(generated) = &order.generated else {
                    continue;
                };
                let Some((from, to)) = order.interval(clock) else {
                    continue;
                };
                let Some(asset) = project.assets.iter().find(|a| a.id == generated.asset) else {
                    continue;
                };
                match load(asset) {
                    Ok(audio) if audio.frames() as u64 != generated.samples => {
                        warnings.push(format!("{} has inconsistent speech duration", asset.name))
                    }
                    Ok(audio) => clips.push(ClipPlayer {
                        audio,
                        span: Span {
                            origin: from as i64 - order.trim_start as i64,
                            from,
                            to,
                            fade_in: 0,
                            fade_out: 0,
                            fade_in_curve: FadeCurve::Linear,
                            fade_out_curve: FadeCurve::Linear,
                        },
                        envelope: None,
                        gain: 1.0,
                        track: index,
                    }),
                    Err(err) => warnings.push(format!("{} is silent: {err}", asset.name)),
                }
            }
            continue;
        }
        if let TrackKind::Video { clips: video_clips } = &track.kind {
            let index = tracks.len();
            tracks.push((
                track.id.clone(),
                track.gain_db.map_or(1.0, track_gain_from_db),
            ));
            for clip in video_clips {
                let Some(asset) = project.videos.iter().find(|video| video.id == clip.asset) else {
                    continue;
                };
                if failed.contains(&asset.id.as_str()) {
                    continue;
                }
                match load_video(asset) {
                    Ok(Some(audio)) => {
                        let mut as_audio = AudioClip::new(&clip.id, &clip.asset, clip.start);
                        as_audio.offset = clip.offset;
                        as_audio.length = clip.length;
                        // The sound fades with the picture.
                        as_audio.fade_in = clip.fade_in;
                        as_audio.fade_out = clip.fade_out;
                        clips.push(ClipPlayer {
                            span: span(&as_audio, &clock, audio.frames() as u64),
                            envelope: None,
                            audio,
                            gain: 1.0,
                            track: index,
                        });
                    }
                    Ok(None) => {}
                    Err(err) => {
                        failed.push(&asset.id);
                        warnings.push(format!("{} is silent: {err}", asset.name));
                    }
                }
            }
            continue;
        }
        let TrackKind::Audio { clips: track_clips } = &track.kind else {
            continue;
        };
        let index = tracks.len();
        tracks.push((
            track.id.clone(),
            track.gain_db.map_or(1.0, track_gain_from_db),
        ));
        for clip in track_clips {
            let Some(asset) = project.assets.iter().find(|asset| asset.id == clip.asset) else {
                continue;
            };
            if failed.contains(&asset.id.as_str()) {
                continue;
            }
            match load(asset) {
                Ok(audio) => {
                    let span = span(clip, &clock, audio.frames() as u64);
                    clips.push(ClipPlayer {
                        envelope: Envelope::new(clip, span.origin, &clock),
                        span,
                        audio,
                        gain: gain_from_db(clip.gain_db),
                        track: index,
                    });
                }
                Err(err) => {
                    failed.push(&asset.id);
                    warnings.push(format!("{} is silent: {err}", asset.name));
                }
            }
        }
    }
    let mut set = ClipSet::new(clips, tracks);
    set.operator_tracks = operator_tracks;
    Programme { set, warnings }
}
