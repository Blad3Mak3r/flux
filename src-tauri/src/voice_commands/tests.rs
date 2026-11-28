use super::*;

#[test]
fn document_transitions_and_reservations_exclude_each_other() {
    let state = VoiceState::default();
    let epoch = state.preview_epoch.load(Ordering::Acquire);
    let transition = state.begin_transition().unwrap();
    assert!(state.reserve().is_err());
    assert!(state.begin_transition().is_err());
    assert_ne!(state.preview_epoch.load(Ordering::Acquire), epoch);
    drop(transition);
    state.reserve().unwrap();
    assert!(state.begin_transition().is_err());
    *state.pending.lock().unwrap() = None;
    assert!(state.begin_transition().is_ok());
}

#[test]
fn an_unwinding_worker_does_not_leave_the_document_permanently_locked() {
    let state = VoiceState::default();
    let token = state.reserve().unwrap();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reservation = Reservation {
            state: &state,
            token,
        };
        panic!("simulated decoder failure");
    }));
    assert!(failed.is_err());
    assert!(state.begin_transition().is_ok());
    assert!(state.reserve().is_ok());
}

#[test]
fn imported_recording_round_trips_offline_with_source_format_and_resets_trims() {
    recording_round_trip(flux_project::operator::RecordingOrigin::Imported);
}

#[test]
fn microphone_take_survives_trim_save_reopen_and_offline_playback() {
    recording_round_trip(flux_project::operator::RecordingOrigin::Recorded);
}

fn recording_round_trip(origin: flux_project::operator::RecordingOrigin) {
    let dir = tempfile::tempdir().unwrap();
    let mut session = flux_project::Session::default();
    session.new_project("Show", flux_project::FrameRate::Fps25);
    session
        .operator_edit(OperatorOp::AddTrack {
            name: "Operator".into(),
        })
        .unwrap();
    let track = session.document().unwrap().project.timelines[0].tracks[0]
        .id
        .clone();
    let mut p = session
        .operator_edit(OperatorOp::AddOrder {
            track_id: track,
            at: 100,
        })
        .unwrap()
        .project;
    let TrackKind::Operator { orders, .. } = &mut p.timelines[0].tracks[0].kind else {
        panic!()
    };
    orders[0].mode = flux_project::operator::VoiceMode::Manual;
    orders[0].dialogue = "Operator output test".into();
    let id = orders[0].id.clone();
    session.sync(p.clone()).unwrap();
    let bytes = include_bytes!("../../assets/operator-test.wav");
    let decoded = decode_recording(bytes.to_vec(), "wav").unwrap();
    assert!(decoded.samples.iter().any(|s| *s != 0));
    let store = JobStore::new(dir.path().join("jobs"));
    let job = store
        .save_recording(
            snapshot(&p, &id, Some("recorded")).unwrap(),
            session.recovery_revision().0.into(),
            Recording {
                bytes,
                samples: decoded.frames() as u64,
                extension: "wav",
                origin,
                voice_name: Some("Human recording.wav".into()),
            },
        )
        .unwrap();
    let audio = store.audio(&job.id).unwrap();
    let mut changed = p.clone();
    let TrackKind::Operator { orders, .. } = &mut changed.timelines[0].tracks[0].kind else {
        panic!()
    };
    orders[0].mode = flux_project::operator::VoiceMode::Generated;
    session.sync(changed.clone()).unwrap();
    assert!(matches!(
        attach_recording(&mut session, changed, job.clone(), &audio, None, false),
        Err(Error::Obsolete)
    ));
    assert!(store.read(&job.id).is_ok());
    session.sync(p.clone()).unwrap();
    let first = attach_recording(&mut session, p, job.clone(), &audio, None, false).unwrap();
    assert!(first.project.assets[0].file.ends_with(".wav"));
    let trimmed = session
        .operator_edit(OperatorOp::SetTrim {
            id: id.clone(),
            start_ms: Some("10".into()),
            end_ms: Some("20".into()),
        })
        .unwrap();
    let restored =
        attach_recording(&mut session, trimmed.project, job, &audio, None, true).unwrap();
    let TrackKind::Operator { orders, .. } = &restored.project.timelines[0].tracks[0].kind else {
        panic!()
    };
    assert_eq!((orders[0].trim_start, orders[0].trim_end), (0, 0));
    assert_eq!(orders[0].status(None), flux_project::VoiceStatus::Synced);
    assert_eq!(orders[0].generated.as_ref().unwrap().origin, origin);
    let archive = dir.path().join("show.flx");
    session.save(&restored.project, &archive).unwrap();
    std::fs::remove_file(audio).unwrap();
    let mut reopened = flux_project::Session::default();
    let p = reopened.open(&archive).unwrap();
    let programme = flux_engine::mixer::build(&p, &p.timelines[0], |asset| {
        Ok(Arc::new(flux_audio::decode(
            reopened.asset_bytes(asset)?,
            Some("wav"),
        )?))
    });
    assert!(programme.warnings.is_empty());
    assert!(!programme.set.is_empty());
}
#[test]
fn duplicate_requests_are_refused_before_contacting_the_provider() {
    let state = VoiceState::default();
    let token = state.reserve().unwrap();
    assert!(matches!(state.reserve(), Err(Error::Busy)));
    assert_eq!(state.pending.lock().unwrap().as_ref().unwrap().token, token);
    *state.pending.lock().unwrap() = None;
    assert_ne!(state.reserve().unwrap(), token);
}

#[test]
fn stopping_preview_invalidates_an_inflight_start() {
    let state = VoiceState::default();
    let epoch = state.preview_epoch.load(Ordering::Acquire);
    stop_preview(&state);
    assert_ne!(state.preview_epoch.load(Ordering::Acquire), epoch);
}

#[test]
fn failed_disk_write_preserves_paid_result_and_retry_never_calls_provider() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("jobs");
    std::fs::write(&root, b"blocks directory creation").unwrap();
    let store = JobStore::new(root.clone());
    let state = VoiceState::default();
    let token = state.reserve().unwrap();
    state.pending.lock().unwrap().as_mut().unwrap().unsaved = Some(Arc::new(Unsaved {
        origin: Default::default(),
        extension: "mp3".into(),
        voice_name: None,
        snapshot: Snapshot {
            mode: Default::default(),
            synthesis_text: None,
            project: "project".into(),
            timeline: "timeline".into(),
            order: "order".into(),
            dialogue: "Go".into(),
            voice: "voice".into(),
            previous_asset: None,
        },
        session_id: "session".into(),
        bytes: b"paid result".to_vec(),
        samples: 48_000,
    }));
    assert!(matches!(
        persist_pending(&state, &store),
        Err(Error::Storage)
    ));
    assert_eq!(state.pending.lock().unwrap().as_ref().unwrap().token, token);
    assert!(matches!(state.reserve(), Err(Error::Busy)));
    std::fs::remove_file(root).unwrap();
    let id = persist_pending(&state, &store).unwrap();
    assert!(state.pending.lock().unwrap().is_none());
    assert_eq!(
        std::fs::read(store.audio(&id).unwrap()).unwrap(),
        b"paid result"
    );
}

#[test]
fn generation_snapshot_detects_dialogue_voice_order_and_project_changes() {
    let mut project = Project::new("Show", flux_project::FrameRate::Fps25);
    let active = project.timelines[0].id.clone();
    flux_project::operator::apply(
        &mut project,
        &active,
        OperatorOp::AddTrack {
            name: "Operator".into(),
        },
    )
    .unwrap();
    let track = project.timelines[0].tracks[0].id.clone();
    flux_project::operator::apply(
        &mut project,
        &active,
        OperatorOp::AddOrder {
            track_id: track,
            at: 10,
        },
    )
    .unwrap();
    let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind else {
        panic!("operator track");
    };
    orders[0].dialogue = "Go".into();
    let id = orders[0].id.clone();
    let before = snapshot(&project, &id, Some("voice")).unwrap();
    project.name = "Renamed show".into();
    flux_project::operator::apply(
        &mut project,
        &active,
        OperatorOp::EditOrder {
            id: id.clone(),
            dialogue: None,
            at: Some(50),
            alignment: Some(flux_project::Alignment::End),
        },
    )
    .unwrap();
    assert!(before == snapshot(&project, &id, Some("voice")).unwrap());
    assert!(before != snapshot(&project, &id, Some("other")).unwrap());
    flux_project::operator::apply(
        &mut project,
        &active,
        OperatorOp::EditOrder {
            id: id.clone(),
            dialogue: Some("Stand by".into()),
            at: None,
            alignment: None,
        },
    )
    .unwrap();
    assert!(before != snapshot(&project, &id, Some("voice")).unwrap());
    flux_project::operator::apply(
        &mut project,
        &active,
        OperatorOp::RemoveOrder { id: id.clone() },
    )
    .unwrap();
    assert!(matches!(
        snapshot(&project, &id, Some("voice")),
        Err(Error::Obsolete)
    ));
}

#[test]
fn provider_errors_do_not_serialize_private_details() {
    let json = serde_json::to_string(&Error::Provider {
        error: flux_voice::Error::Credentials,
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"code":"provider","error":{"code":"credentials"}}"#
    );
}

#[test]
fn attaching_preserves_anchor_and_rejects_stale_edits_without_losing_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().join("jobs"));
    let mut session = flux_project::Session::default();
    session.new_project("Show", flux_project::FrameRate::Fps25);
    session
        .operator_edit(OperatorOp::AddTrack {
            name: "Operator".into(),
        })
        .unwrap();
    let track = session.document().unwrap().project.timelines[0].tracks[0]
        .id
        .clone();
    let mut project = session
        .operator_edit(OperatorOp::AddOrder {
            track_id: track,
            at: 50,
        })
        .unwrap()
        .project;
    let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind else {
        panic!()
    };
    orders[0].dialogue = "Go".into();
    let id = orders[0].id.clone();
    session.sync(project.clone()).unwrap();
    let job = store
        .save(
            snapshot(&project, &id, Some("voice")).unwrap(),
            session.recovery_revision().0.into(),
            b"audio fixture",
            48_000,
        )
        .unwrap();
    let audio = store.audio(&job.id).unwrap();
    let old_command = project;
    let moved = session
        .operator_edit(OperatorOp::EditOrder {
            id: id.clone(),
            dialogue: None,
            at: Some(100),
            alignment: Some(flux_project::Alignment::End),
        })
        .unwrap()
        .project;
    assert!(matches!(
        attach_recording(
            &mut session,
            old_command,
            job.clone(),
            &audio,
            Some("voice"),
            false
        ),
        Err(Error::Obsolete)
    ));
    assert_eq!(session.document().unwrap().project, moved);
    let attached = attach_recording(
        &mut session,
        moved,
        job.clone(),
        &audio,
        Some("voice"),
        false,
    )
    .unwrap();
    let TrackKind::Operator { orders, .. } = &attached.project.timelines[0].tracks[0].kind else {
        panic!()
    };
    assert_eq!(
        (orders[0].at, orders[0].alignment),
        (100, flux_project::Alignment::End)
    );
    assert_eq!(orders[0].generated.as_ref().unwrap().asset, job.id);
    let changed = session
        .operator_edit(OperatorOp::EditOrder {
            id,
            dialogue: Some("New draft".into()),
            at: None,
            alignment: None,
        })
        .unwrap()
        .project;
    assert!(matches!(
        attach_recording(
            &mut session,
            changed.clone(),
            job.clone(),
            &audio,
            Some("voice"),
            false
        ),
        Err(Error::Obsolete)
    ));
    let recovered = attach_recording(
        &mut session,
        changed,
        job.clone(),
        &audio,
        Some("other"),
        true,
    )
    .unwrap();
    let TrackKind::Operator { orders, .. } = &recovered.project.timelines[0].tracks[0].kind else {
        panic!()
    };
    assert_eq!(orders[0].dialogue, "New draft");
    assert_eq!(orders[0].generated.as_ref().unwrap().dialogue, "Go");
    assert_eq!(
        orders[0].status(Some("other")),
        flux_project::VoiceStatus::Stale
    );
    assert_eq!(
        recovered.project.assets.len(),
        1,
        "recovering twice reuses the durable asset id"
    );
    assert_eq!(store.list(&recovered.project.meta.id).unwrap().len(), 1);
}
