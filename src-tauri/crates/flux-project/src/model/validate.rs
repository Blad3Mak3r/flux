//! The checks of a project's tracks, clips and paths.

use std::collections::HashSet;

use super::*;
use crate::error::{ProjectError, Result};

pub(super) fn validate_track<'a>(
    track: &'a Track,
    asset_ids: &HashSet<&str>,
    video_ids: &HashSet<&str>,
    destination_ids: &HashSet<&str>,
    dmx_destination_ids: &HashSet<&str>,
    track_ids: &mut HashSet<&'a str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    ensure_unique(track_ids, "track id", &track.id)?;
    if track.gain_db.is_some_and(|db| !db.is_finite()) {
        return Err(ProjectError::Invalid(format!(
            "track {} has a gain that is not a number",
            track.id
        )));
    }
    if let Some(color) = &track.color {
        check_color(color)?;
    }
    match &track.kind {
        TrackKind::Operator { voice, orders } => {
            crate::operator::validate_voice(voice.as_deref())?;
            for order in orders {
                ensure_unique(item_ids, "order id", &order.id)?;
                order.validate(asset_ids)?;
            }
        }
        TrackKind::Audio { clips } => {
            for clip in clips {
                ensure_unique(item_ids, "clip/note id", &clip.id)?;
                check_clip_name(&clip.id, clip.name.as_deref())?;
                check_envelope(clip)?;
                if clip.length == Some(0) {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} plays no time; leave the length out instead",
                        clip.id
                    )));
                }
                if !asset_ids.contains(clip.asset.as_str()) {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} references unknown asset {}",
                        clip.id, clip.asset
                    )));
                }
            }
        }
        TrackKind::Notes { notes } => {
            for note in notes {
                ensure_unique(item_ids, "clip/note id", &note.id)?;
                if note.length == Some(0) {
                    return Err(ProjectError::Invalid(format!(
                        "note {} lasts no time; leave the length out instead",
                        note.id
                    )));
                }
            }
        }
        TrackKind::Midi { events, .. } => {
            for event in events {
                ensure_unique(item_ids, "clip/note id", &event.id)?;
                event.validate()?;
            }
        }
        TrackKind::Osc {
            destination,
            events,
        } => {
            if let Some(id) = destination
                && !destination_ids.contains(id.as_str())
            {
                return Err(ProjectError::Invalid(format!(
                    "track {} sends to unknown OSC destination {id}",
                    track.id
                )));
            }
            for event in events {
                ensure_unique(item_ids, "clip/note id", &event.id)?;
                event.validate()?;
            }
        }
        TrackKind::Dmx {
            destination,
            scenes,
        } => {
            if let Some(id) = destination
                && !dmx_destination_ids.contains(id.as_str())
            {
                return Err(ProjectError::Invalid(format!(
                    "track {} sends to unknown DMX destination {id}",
                    track.id
                )));
            }
            for scene in scenes {
                ensure_unique(item_ids, "clip/note id", &scene.id)?;
                scene.validate()?;
            }
        }
        TrackKind::Video { clips } => {
            for clip in clips {
                ensure_unique(item_ids, "clip/note id", &clip.id)?;
                check_clip_name(&clip.id, clip.name.as_deref())?;
                if clip.length == Some(0) {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} plays no time; leave the length out instead",
                        clip.id
                    )));
                }
                if !video_ids.contains(clip.asset.as_str()) {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} references unknown video {}",
                        clip.id, clip.asset
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn check_clip_name(id: &str, name: Option<&str>) -> Result<()> {
    if let Some(name) = name
        && (name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME)
    {
        return Err(ProjectError::Invalid(format!(
            "clip {id} needs a name of 1 to {MAX_CLIP_NAME} characters; leave it out instead"
        )));
    }
    Ok(())
}

/// An envelope has at most [`MAX_ENVELOPE_POINTS`] points, in order, each within the range of gains.
pub(super) fn check_envelope(clip: &AudioClip) -> Result<()> {
    let invalid =
        |why: &str| ProjectError::Invalid(format!("the envelope of clip {} {why}", clip.id));
    if clip.envelope.len() > MAX_ENVELOPE_POINTS {
        return Err(invalid(&format!(
            "has more than {MAX_ENVELOPE_POINTS} points"
        )));
    }
    if clip
        .envelope
        .windows(2)
        .any(|pair| pair[0].at >= pair[1].at)
    {
        return Err(invalid(
            "must have its points in order, none at the same place",
        ));
    }
    if clip
        .envelope
        .iter()
        .any(|point| !(ENVELOPE_MIN_DB..=ENVELOPE_MAX_DB).contains(&point.gain_db))
    {
        return Err(invalid(&format!(
            "has a gain outside {ENVELOPE_MIN_DB} to {ENVELOPE_MAX_DB} dB"
        )));
    }
    Ok(())
}

pub(super) fn check_offset(offset_ms: Option<i32>) -> Result<()> {
    if offset_ms.is_some_and(|ms| ms.abs() > MAX_OFFSET_MS) {
        return Err(ProjectError::Invalid(format!(
            "the offset must be within {MAX_OFFSET_MS} ms either way"
        )));
    }
    Ok(())
}

/// `#rrggbb`, so it can go into a stylesheet as it is.
pub(super) fn check_color(color: &str) -> Result<()> {
    match color.strip_prefix('#') {
        Some(digits) if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(())
        }
        _ => Err(ProjectError::Invalid(format!(
            "track colour must look like #rrggbb: {color}"
        ))),
    }
}

/// An asset file must be `audio/<name>` with a plain file name: no separators, no `.`/`..`.
pub(crate) fn check_asset_path(file: &str) -> Result<()> {
    check_path_in(AUDIO_DIR, file)
}

/// A video file must be `video/<name>` with a plain file name, like [`check_asset_path`].
pub(crate) fn check_video_path(file: &str) -> Result<()> {
    check_path_in(VIDEO_DIR, file)
}

pub(super) fn check_link(link: Option<&MediaLink>) -> Result<()> {
    match link {
        Some(link) if link.path.is_empty() => {
            Err(ProjectError::Invalid("a media link has no path".into()))
        }
        _ => Ok(()),
    }
}

pub(super) fn check_path_in(dir: &str, file: &str) -> Result<()> {
    let valid = file
        .strip_prefix(dir)
        .is_some_and(|name| !matches!(name, "" | "." | "..") && !name.contains(['/', '\\', '\0']));
    if valid {
        Ok(())
    } else {
        Err(ProjectError::InvalidAssetPath(file.to_owned()))
    }
}

pub(super) fn ensure_unique<'a>(
    seen: &mut HashSet<&'a str>,
    what: &str,
    value: &'a str,
) -> Result<()> {
    if seen.insert(value) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate {what}: {value}")))
    }
}
