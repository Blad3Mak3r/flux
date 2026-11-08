//! Setlist preparation and operation. UI, MIDI and OSC share these entry points.
use crate::{
    commands::SharedSession,
    live_commands::cue_list,
    transport_commands::{self, SharedOutput},
};
use flux_audio::SharedAudio;
use flux_core::Clock;
use flux_engine::{
    output::OutputError,
    show::{PreparedEntry, ShowCommand, ShowPlan, ShowTick},
};
use flux_project::{ControlKind, Project, ProjectState, ShowEdit};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, PoisonError};
use tauri::{AppHandle, Manager, State};

pub(crate) fn prepare_plan(
    project: &Project,
    id: &str,
    session: &SharedSession,
    audio: &SharedAudio,
) -> Result<Arc<ShowPlan>, OutputError> {
    let setlist = project
        .setlists
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| OutputError::Stream("show.error.setlist".into()))?;
    let mut entries: Vec<PreparedEntry> = Vec::new();
    for entry in setlist.entries.iter().filter(|e| !e.omitted) {
        let timeline = project
            .timeline(&entry.timeline_id)
            .ok_or_else(|| OutputError::Stream("show.error.timeline".into()))?;
        if let Some(known) = entries.iter().find(|e| e.timeline.id == timeline.id) {
            entries.push(PreparedEntry {
                entry: entry.clone(),
                timeline: timeline.clone(),
                clips: Arc::clone(&known.clips),
                end: known.end,
                cues: known.cues.clone(),
            });
            continue;
        }
        let cues = cue_list(project, timeline, session, audio);
        for track in timeline.tracks.iter().filter(|t| !t.muted) {
            if let flux_project::TrackKind::Operator { orders, .. } = &track.kind {
                for order in orders {
                    if order.generated.is_none()
                        || order
                            .interval(Clock::new(48_000, timeline.timecode.frame_rate))
                            .is_none()
                    {
                        return Err(OutputError::Stream(format!(
                            "operator order {} is not ready",
                            order.id
                        )));
                    }
                }
            }
        }
        let mut audible = timeline.clone();
        for track in &mut audible.tracks {
            if matches!(
                track.kind,
                flux_project::TrackKind::Audio { .. } | flux_project::TrackKind::Operator { .. }
            ) {
                track.muted = false;
            }
        }
        let programme = transport_commands::programme(project, &audible, session, audio, &cues);
        for track in timeline.tracks.iter().filter(|t| t.muted) {
            programme.set.levels().set_gain_db(&track.id, -60.0);
        }
        if !programme.warnings.is_empty() {
            return Err(OutputError::Stream(programme.warnings.join("\n")));
        }
        let clock = Clock::new(48_000, timeline.timecode.frame_rate);
        let control_end = timeline
            .controls
            .iter()
            .map(|c| match c.kind {
                ControlKind::Wait => c.at + 1,
                ControlKind::Loop { end, .. } => end,
            })
            .max()
            .unwrap_or(0);
        let end = cues.end_samples().max(clock.frames_to_samples(control_end));
        if end == 0 {
            return Err(OutputError::Stream(format!(
                "timeline {} has no duration",
                timeline.name
            )));
        }
        entries.push(PreparedEntry {
            entry: entry.clone(),
            timeline: timeline.clone(),
            clips: Arc::new(programme.set),
            end,
            cues,
        });
    }
    if entries.is_empty() {
        return Err(OutputError::Stream("show.error.empty".into()));
    }
    let execution_lock = Arc::clone(
        &session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .execution_lock,
    );
    Ok(Arc::new(ShowPlan {
        mix: Default::default(),
        execution_lock,
        setlist_id: id.into(),
        entries,
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowState {
    pub live: flux_engine::cues::LiveState,
    pub start: u64,
    pub setlist_id: String,
    pub entry_id: String,
    pub timeline_id: String,
    pub name: String,
    pub notes: String,
    pub next: Option<String>,
    pub tick: ShowTick,
    pub position: flux_engine::output::Position,
    pub frame_rate: flux_project::FrameRate,
    pub cues: Vec<flux_engine::cues::Cue>,
    pub duration_frames: Option<u64>,
    pub duration_text: Option<String>,
}

pub(crate) fn state(output: &SharedOutput) -> Option<ShowState> {
    let reader = transport_commands::show_reader(output)?;
    let (plan, tick, mut position) = reader.show_snapshot()?;
    let entry = &plan.entries[tick.entry];
    let mut tick = tick;

    if position.device_lost {
        tick.status = flux_engine::show::ShowStatus::Blocked;
        tick.go = flux_engine::show::GoAction::None;
        position.state = flux_engine::transport::TransportState::Stopped;
    }
    let name = |e: &PreparedEntry| {
        if e.entry.name.is_empty() {
            e.timeline.name.clone()
        } else {
            e.entry.name.clone()
        }
    };
    let clock = Clock::new(48_000, entry.timeline.timecode.frame_rate);
    Some(ShowState {
        live: entry.cues.state_at(position.frame),
        start: entry.timeline.timecode.start,
        setlist_id: plan.setlist_id.clone(),
        entry_id: entry.entry.id.clone(),
        timeline_id: entry.timeline.id.clone(),
        name: name(entry),
        notes: entry.entry.notes.clone(),
        next: plan.entries.get(tick.entry + 1).map(name),
        tick,
        position,
        frame_rate: entry.timeline.timecode.frame_rate,
        cues: entry.cues.cues().to_vec(),
        duration_frames: entry.duration_samples().map(|n| clock.samples_to_frames(n)),
        duration_text: entry.duration_samples().map(|n| {
            let seconds = n / 48_000;
            format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        }),
    })
}

#[tauri::command]
pub fn show_state(output: State<'_, SharedOutput>) -> Option<ShowState> {
    state(&output)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    Go,
    Devamp,
    CancelDevamp,
    Prepare { entry_id: String },
}

pub(crate) fn execute(
    app: &AppHandle,
    output: &SharedOutput,
    action: Action,
) -> Result<(), OutputError> {
    let command = match action {
        Action::Go => ShowCommand::Go,
        Action::Devamp => ShowCommand::Devamp,
        Action::CancelDevamp => ShowCommand::CancelDevamp,
        Action::Prepare { entry_id } => {
            let reader = transport_commands::show_reader(output).ok_or(OutputError::NotRunning)?;
            let (plan, _) = reader.show().ok_or(OutputError::NotRunning)?;
            let index = plan
                .entries
                .iter()
                .position(|e| e.entry.id == entry_id)
                .ok_or_else(|| OutputError::Stream("show.error.entry".into()))?;
            ShowCommand::Prepare(index)
        }
    };
    crate::voice_commands::send_show(
        &app.state::<Arc<crate::voice_commands::VoiceState>>(),
        output,
        command,
    )
}

#[tauri::command]
pub fn show_action(
    app: AppHandle,
    output: State<'_, SharedOutput>,
    action: Action,
) -> Result<(), OutputError> {
    execute(&app, &output, action)
}

#[tauri::command]
pub async fn show_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: ShowEdit,
) -> Result<ProjectState, String> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        session.ensure_editable().map_err(show_edit_error)?;
        project.validate().map_err(|e| e.to_string())?;
        session.sync(project).map_err(show_edit_error)?;
        session.show_edit(op).map_err(show_edit_error)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn show_control_add(
    session: State<'_, SharedSession>,
    project: Project,
    timeline_id: String,
    frame: u64,
    kind: flux_project::show::ShowControlKind,
    name: String,
) -> Result<flux_project::CueEdited, String> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        session.ensure_editable().map_err(show_edit_error)?;
        session.sync(project).map_err(show_edit_error)?;
        session
            .add_show_control(&timeline_id, frame, kind, name)
            .map_err(show_edit_error)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Used by OSC without needing a browser window or subscriptions.
pub(crate) async fn prepare(app: AppHandle, setlist_id: String) -> Result<(), String> {
    let project = app
        .state::<SharedSession>()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .document()
        .ok_or("no project is open")?
        .project;
    if transport_commands::is_playing(&app.state::<SharedOutput>()) {
        return Err("stop the show first".into());
    }
    transport_commands::output_start(
        app.clone(),
        app.state(),
        app.state(),
        app.state(),
        app.state(),
        project,
        false,
        false,
        Some(setlist_id),
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn track_level(app: &AppHandle, id: &str, value: f32, mute: bool) -> Result<(), String> {
    if !value.is_finite()
        || (!mute && !(-60.0..=6.0).contains(&value))
        || (mute && value != 0.0 && value != 1.0)
    {
        return Err("invalidTrackLevel".into());
    }
    let output = app.state::<SharedOutput>();
    let reader = transport_commands::show_reader(&output).ok_or("outputNotRunning")?;
    if let Some((plan, tick)) = reader.show() {
        let track = plan.entries[tick.entry]
            .timeline
            .tracks
            .iter()
            .find(|t| t.id == id)
            .ok_or("unknownTrack")?;
        if !matches!(
            track.kind,
            flux_project::TrackKind::Audio { .. } | flux_project::TrackKind::Operator { .. }
        ) {
            return Err("trackHasNoAudioLevel".into());
        }
        let mut levels = plan.mix.lock().unwrap_or_else(PoisonError::into_inner);
        let level = levels
            .entry(id.into())
            .or_insert((track.gain_db.unwrap_or(0.0), track.muted));
        if mute {
            level.1 = value != 0.0;
        } else {
            level.0 = value;
        }
        for entry in &plan.entries {
            entry
                .clips
                .levels()
                .set_gain_db(id, if level.1 { -60.0 } else { level.0 });
        }
        Ok(())
    } else {
        Err("prepareSetlistFirst".into())
    }
}

pub(crate) async fn prepare_timeline(app: AppHandle, timeline_id: &str) -> Result<(), String> {
    let state = {
        let session = app.state::<SharedSession>();
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        session.ensure_editable().map_err(show_edit_error)?;
        session
            .select_timeline(timeline_id)
            .map_err(|e| e.to_string())?
    };
    transport_commands::output_start(
        app.clone(),
        app.state(),
        app.state(),
        app.state(),
        app.state(),
        state.project.clone(),
        false,
        false,
        None,
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .await
    .map_err(|e| e.to_string())?;
    use tauri::Emitter;
    let _ = app.emit(transport_commands::TIMELINE_ADVANCED, state);
    Ok(())
}

fn show_edit_error(error: flux_project::ProjectError) -> String {
    match error {
        flux_project::ProjectError::Invalid(code) if code.starts_with("show.error.") => code,
        other => other.to_string(),
    }
}
