use tempfile::TempDir;

use super::super::*;
use super::imported_project;
use crate::error::ProjectError;
use crate::model::TrackKind;
use flux_core::FrameRate;

/// A session holding a new 25 fps project that has a second timeline; the first is active.
fn with_two_timelines() -> (Session, String, String) {
    let mut session = Session::default();
    let first = session.new_project("Show", FrameRate::Fps25).timelines[0]
        .id
        .clone();
    let second = session.add_timeline("Timeline").unwrap().active;
    session.select_timeline(&first).unwrap();
    (session, first, second)
}

#[test]
fn new_project_should_activate_its_first_timeline() {
    let mut session = Session::default();
    let project = session.new_project("Show", FrameRate::Fps25);

    assert_eq!(session.active(), project.timelines[0].id);
}

#[test]
fn add_timeline_should_number_it_and_activate_it() {
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);

    let state = session.add_timeline("Timeline").unwrap();

    assert_eq!(state.project.timelines[1].name, "Timeline 2");
    assert_eq!(state.active, state.project.timelines[1].id);
}

#[test]
fn add_timeline_should_copy_the_frame_rate_of_the_active_one() {
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps30);

    let state = session.add_timeline("Timeline").unwrap();

    assert_eq!(
        state.project.timelines[1].timecode.frame_rate,
        FrameRate::Fps30
    );
}

#[test]
fn add_timeline_should_start_the_new_one_at_zero_with_no_tracks() {
    let mut session = Session::default();
    let mut project = Project::new("Show", FrameRate::Fps25);
    project.timelines[0].timecode.start = 90_000;
    session.update(project);

    let added = &session.add_timeline("Timeline").unwrap().project.timelines[1];

    assert_eq!((added.timecode.start, added.tracks.len()), (0, 0));
}

#[test]
fn timeline_operations_should_fail_without_a_document() {
    let mut session = Session::default();

    assert!(session.add_timeline("Timeline").is_err());
    assert!(session.select_timeline("x").is_err());
}

#[test]
fn rename_timeline_should_trim_the_name() {
    let (mut session, first, _) = with_two_timelines();

    let state = session.rename_timeline(&first, "  Song 1 ").unwrap();

    assert_eq!(state.project.timelines[0].name, "Song 1");
}

#[test]
fn rename_timeline_should_refuse_an_empty_name() {
    let (mut session, first, _) = with_two_timelines();

    assert!(matches!(
        session.rename_timeline(&first, "   "),
        Err(ProjectError::Invalid(_))
    ));
}

#[test]
fn rename_timeline_should_refuse_an_unknown_id() {
    let (mut session, ..) = with_two_timelines();

    assert!(session.rename_timeline("nope", "Song").is_err());
}

#[test]
fn select_timeline_should_change_the_active_one() {
    let (mut session, _, second) = with_two_timelines();

    let state = session.select_timeline(&second).unwrap();

    assert_eq!((state.active, session.active()), (second.clone(), second));
}

#[test]
fn select_timeline_should_refuse_an_unknown_id_and_keep_the_active_one() {
    let (mut session, first, _) = with_two_timelines();

    assert!(session.select_timeline("nope").is_err());
    assert_eq!(session.active(), first);
}

#[test]
fn remove_timeline_should_refuse_the_last_one() {
    let mut session = Session::default();
    let only = session.new_project("Show", FrameRate::Fps25).timelines[0]
        .id
        .clone();

    assert!(session.remove_timeline(&only).is_err());
    assert_eq!(session.document().unwrap().project.timelines.len(), 1);
}

#[test]
fn remove_timeline_should_activate_the_one_that_takes_its_place() {
    let (mut session, first, second) = with_two_timelines();

    let state = session.remove_timeline(&first).unwrap();

    assert_eq!(state.active, second);
}

#[test]
fn remove_timeline_should_keep_the_active_one_when_another_goes() {
    let (mut session, first, second) = with_two_timelines();

    let state = session.remove_timeline(&second).unwrap();

    assert_eq!(state.active, first);
}

#[test]
fn remove_timeline_should_drop_the_audio_no_clip_uses_any_more() {
    let dir = TempDir::new().unwrap();
    let (mut session, project) = imported_project(&dir);
    let first = project.timelines[0].id.clone();
    session.update(project);
    session.add_timeline("Timeline").unwrap();

    let state = session.remove_timeline(&first).unwrap();

    assert!(state.project.assets.is_empty());
}

#[test]
fn remove_timeline_should_keep_the_audio_another_timeline_uses() {
    let dir = TempDir::new().unwrap();
    let (mut session, mut project) = imported_project(&dir);
    let first = project.timelines[0].id.clone();
    let mut second = Timeline::new("Second", FrameRate::Fps25);
    let mut track = project.timelines[0].tracks[0].clone();
    track.id = "t2".into();
    if let TrackKind::Audio { clips } = &mut track.kind {
        clips[0].id = "c2".into();
    }
    second.tracks.push(track);
    project.timelines.push(second);
    session.update(project);

    let state = session.remove_timeline(&first).unwrap();

    assert_eq!(state.project.assets.len(), 1);
}

#[test]
fn set_timecode_should_change_the_active_timeline_only() {
    let (mut session, first, second) = with_two_timelines();
    session.select_timeline(&second).unwrap();

    let state = session.set_timecode(FrameRate::Fps24, 86_400).unwrap();

    let timecode = |id: &str| state.project.timeline(id).unwrap().timecode.clone();
    assert_eq!(
        (timecode(&second).frame_rate, timecode(&second).start),
        (FrameRate::Fps24, 86_400)
    );
    assert_eq!(timecode(&first).frame_rate, FrameRate::Fps25);
}

#[test]
fn update_should_fall_back_to_the_first_timeline_when_the_active_one_is_gone() {
    let (mut session, first, second) = with_two_timelines();
    session.select_timeline(&second).unwrap();
    let mut project = session.document().unwrap().project;
    project.timelines.retain(|timeline| timeline.id == first);

    session.update(project);

    assert_eq!(session.active(), first);
}

#[test]
fn save_should_keep_the_active_timeline() {
    let dir = TempDir::new().unwrap();
    let (mut session, _, second) = with_two_timelines();
    session.select_timeline(&second).unwrap();
    let project = session.document().unwrap().project;

    session
        .save(&project, &dir.path().join("show.fxtc"))
        .unwrap();

    assert_eq!(session.active(), second);
}

#[test]
fn active_timeline_should_follow_the_selection_in_the_project_it_is_given() {
    let (mut session, _, second) = with_two_timelines();
    let project = session.document().unwrap().project;
    session.select_timeline(&second).unwrap();

    let timeline = session.active_timeline(&project).unwrap();

    assert_eq!(timeline.id, second);
}

#[test]
fn active_timeline_should_fall_back_to_the_first_when_the_project_lacks_it() {
    let (session, first, _) = with_two_timelines();
    let mut project = session.document().unwrap().project;
    project.timelines.reverse();
    project.timelines.retain(|timeline| timeline.id == first);

    assert_eq!(session.active_timeline(&project).unwrap().id, first);
}

#[test]
fn next_timeline_should_activate_the_one_after_the_active_one() {
    let (mut session, _, second) = with_two_timelines();

    let state = session.next_timeline().unwrap().unwrap();

    assert_eq!(state.active, second);
}

#[test]
fn next_timeline_should_stay_on_the_last_one() {
    let (mut session, _, second) = with_two_timelines();
    session.select_timeline(&second).unwrap();

    assert_eq!(session.next_timeline().unwrap(), None);
    assert_eq!(session.active(), second);
}

#[test]
fn next_timeline_should_fail_without_a_document() {
    assert!(Session::default().next_timeline().is_err());
}

#[test]
fn step_timeline_should_go_back_and_stop_at_both_ends() {
    let (mut session, first, second) = with_two_timelines();

    assert_eq!(
        session.step_timeline(-1).unwrap(),
        None,
        "no one before the first"
    );
    assert_eq!(session.step_timeline(1).unwrap().unwrap().active, second);
    assert_eq!(
        session.step_timeline(1).unwrap(),
        None,
        "no one after the last"
    );
    assert_eq!(session.step_timeline(-1).unwrap().unwrap().active, first);
    assert_eq!(session.step_timeline(-5).unwrap(), None);
}

#[test]
fn select_timeline_at_should_count_places_from_one() {
    let (mut session, first, second) = with_two_timelines();

    assert_eq!(
        session.select_timeline_at(2).unwrap().unwrap().active,
        second
    );
    assert_eq!(
        session.select_timeline_at(1).unwrap().unwrap().active,
        first
    );
    assert_eq!(session.select_timeline_at(0).unwrap(), None);
    assert_eq!(session.select_timeline_at(3).unwrap(), None);
    assert_eq!(session.active(), first);
}

#[test]
fn step_and_select_should_fail_without_a_document() {
    assert!(Session::default().step_timeline(1).is_err());
    assert!(Session::default().select_timeline_at(1).is_err());
    assert_eq!(Session::default().active_timecode(), None);
}

#[test]
fn active_timecode_should_be_the_one_of_the_active_timeline() {
    let (mut session, _, _) = with_two_timelines();
    let state = session.set_timecode(FrameRate::Fps30, 500).unwrap();

    let timecode = session.active_timecode().unwrap();

    assert_eq!(timecode.frame_rate, FrameRate::Fps30);
    assert_eq!(timecode.start, 500);
    assert_eq!(
        state.project.timeline(&state.active).unwrap().timecode,
        timecode
    );
}

fn order(state: &ProjectState) -> Vec<&str> {
    state
        .project
        .timelines
        .iter()
        .map(|timeline| timeline.id.as_str())
        .collect()
}

#[test]
fn move_timeline_should_put_it_at_the_new_index() {
    let (mut session, first, second) = with_two_timelines();

    let state = session.move_timeline(&first, 1).unwrap();

    assert_eq!(order(&state), [second.as_str(), first.as_str()]);
}

#[test]
fn move_timeline_should_clamp_the_index_to_the_last_place() {
    let (mut session, first, second) = with_two_timelines();

    let state = session.move_timeline(&first, 99).unwrap();

    assert_eq!(order(&state), [second.as_str(), first.as_str()]);
}

#[test]
fn move_timeline_should_leave_the_order_when_it_is_already_there() {
    let (mut session, first, second) = with_two_timelines();

    let state = session.move_timeline(&first, 0).unwrap();

    assert_eq!(order(&state), [first.as_str(), second.as_str()]);
}

#[test]
fn move_timeline_should_keep_the_active_timeline() {
    let (mut session, first, _) = with_two_timelines();

    let state = session.move_timeline(&first, 1).unwrap();

    assert_eq!(state.active, first);
}

#[test]
fn move_timeline_should_refuse_an_unknown_id() {
    let (mut session, ..) = with_two_timelines();

    assert!(session.move_timeline("nope", 0).is_err());
}
