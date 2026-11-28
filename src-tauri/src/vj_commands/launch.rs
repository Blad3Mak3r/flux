use super::*;

/// Plays a clip of the grid on its layer from its first frame; a playing clip starts again.
#[tauri::command]
pub fn vj_launch(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.launch(vj, &clip, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Moves the playhead of the clip playing on a layer to `position`, a fraction of the clip's
/// length (brought into its region), as the hand scrubs.
#[tauri::command]
pub fn vj_scrub(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    layer: String,
    position: f64,
) -> Result<GridState> {
    if !position.is_finite() {
        return Err(ProjectError::Invalid("a position is a number".into()));
    }
    play(&app, &session, &grid, |play, _, now| {
        if play.scrub(&layer, position.clamp(0.0, 1.0), now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip plays on {layer}")))
        }
    })
}

/// Jumps to cue number `index` (0 to 7) of a clip: the clip's playhead moves there if it plays,
/// and the clip is launched there if it does not.
#[tauri::command]
pub fn vj_cue(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
    index: usize,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        play.cue(vj, &clip, index, now)
            .ok_or_else(|| ProjectError::Invalid(format!("clip {clip} has no cue {}", index + 1)))
    })
}

/// Lets go of a clip: one triggered as a piano key stops.
#[tauri::command]
pub fn vj_release(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.release(vj, &clip, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Launches every clip of a column; the deck layers without a clip in it stop, global layers play on.
#[tauri::command]
pub fn vj_launch_column(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    column: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.launch_column(vj, &column, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no column {column}")))
        }
    })
}

/// Launches the clips of a column on the layers of a group only, as the group's own column trigger.
#[tauri::command]
pub fn vj_launch_group_column(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    group: String,
    column: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.launch_group_column(vj, &group, &column, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!(
                "no group {group} or column {column}"
            )))
        }
    })
}

/// Stops a layer, or every layer without one.
#[tauri::command]
pub fn vj_stop(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    layer: Option<String>,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        play.stop(vj, layer.as_deref(), now);
        Ok(())
    })
}

/// Moves the crossfader: 0 is all side A, 1 all side B.
#[tauri::command]
pub fn vj_crossfader(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    position: f32,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, _, _| {
        play.set_crossfader(position);
        Ok(())
    })
}

/// Puts the crossfader back in the middle.
#[tauri::command]
pub fn vj_crossfader_reset(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, _, _| {
        play.set_crossfader(GridPlay::CROSSFADER_DEFAULT);
        Ok(())
    })
}
