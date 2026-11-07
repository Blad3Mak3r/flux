//! Brings the `project.json` of an older format up to the current one, on the raw JSON before it is
//! parsed, so the model only ever has to describe the current shape. Opening a file never rewrites
//! it by itself: the migrated project is what the app holds, and saving writes the current version.
//!
//! Each step looks at the shape of the file, not at its version number. Builds that were being
//! developed side by side both wrote "version 2" with different shapes (notes and single outputs on
//! one, event tracks and lists of outputs on the other), and both must open. The single-object
//! outputs of version 1 are read by the model itself (`list_or_one`).

use serde_json::{Value, json};
use uuid::Uuid;

use super::model::FORMAT_VERSION;

/// Migrates `value`, a `project.json`, to [`FORMAT_VERSION`]. `created` is the date given to a file
/// that does not say when it was made (its modification time). A value that does not have the
/// shape it should is left as it is, for parsing to report.
pub(super) fn upgrade(value: &mut Value, created: &str) {
    events_to_notes(value);
    add_meta(value, created);
    wrap_timelines(value);
    if let Some(project) = value.as_object_mut() {
        project
            .entry("autosave")
            .or_insert_with(|| json!({ "enabled": true, "intervalSeconds": 30 }));
    }
    if let Some(version) = value.get_mut("formatVersion") {
        *version = json!(FORMAT_VERSION);
    }
}

/// Before version 4 a project had one timecode and one list of tracks at the top level. They become
/// the first timeline, named like the project.
fn wrap_timelines(value: &mut Value) {
    let Some(project) = value.as_object_mut() else {
        return;
    };
    if project.contains_key("timelines") {
        return;
    }
    let timeline = json!({
        "id": Uuid::new_v4().to_string(),
        "name": project.get("name").cloned().unwrap_or_else(|| json!("")),
        "timecode": project.remove("timecode"),
        "tracks": project.remove("tracks"),
    });
    project.insert("timelines".into(), json!([timeline]));
}

/// Files from before version 3 have no identity: they get a new id, and the creation and last
/// modification are the date of the file. `savedWith` stays empty, which marks the project as one
/// that has to be saved again.
fn add_meta(value: &mut Value, created: &str) {
    let Some(project) = value.as_object_mut() else {
        return;
    };
    project.entry("meta").or_insert_with(|| {
        json!({
            "id": Uuid::new_v4().to_string(),
            "createdAt": created,
            "modifiedAt": created,
            "createdWith": "",
            "savedWith": "",
        })
    });
}

/// Version 1 had `events` tracks of triggers. A marker becomes a note whose text is its label. A
/// MIDI trigger is dropped: nothing ever sent it, and MIDI cues will come back as a track of their
/// own.
fn events_to_notes(value: &mut Value) {
    let Some(tracks) = value.get_mut("tracks").and_then(Value::as_array_mut) else {
        return;
    };
    for track in tracks {
        let Some(kind) = track.get_mut("kind") else {
            continue;
        };
        if kind.get("type").and_then(Value::as_str) != Some("events") {
            continue;
        }
        let notes: Vec<Value> = kind
            .get("triggers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|trigger| {
                trigger.pointer("/action/type").and_then(Value::as_str) == Some("marker")
            })
            .map(|trigger| {
                json!({
                    "id": trigger.get("id").cloned().unwrap_or(Value::Null),
                    "at": trigger.get("at").cloned().unwrap_or(Value::Null),
                    "text": trigger.get("label").cloned().unwrap_or_else(|| json!("")),
                })
            })
            .collect();
        *kind = json!({ "type": "notes", "notes": notes });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: &str = "2026-11-05T14:29:03Z";

    fn v1() -> Value {
        json!({
            "formatVersion": 1,
            "name": "Old show",
            "tracks": [
                { "id": "t1", "kind": { "type": "audio", "clips": [] } },
                { "id": "t2", "name": "Cues", "muted": false, "kind": { "type": "events", "triggers": [
                    { "id": "e1", "at": 300, "label": "Lights up", "action": { "type": "marker" } },
                    { "id": "e2", "at": 450, "label": "Program change",
                      "action": { "type": "midi", "bytes": [192, 5] } },
                    { "id": "e3", "at": 600, "label": "Blackout", "action": { "type": "marker" } }
                ] } }
            ]
        })
    }

    #[test]
    fn upgrade_should_turn_marker_triggers_into_notes_with_the_label_as_text() {
        let mut value = v1();
        upgrade(&mut value, DATE);
        assert_eq!(
            value["timelines"][0]["tracks"][1]["kind"],
            json!({ "type": "notes", "notes": [
                { "id": "e1", "at": 300, "text": "Lights up" },
                { "id": "e3", "at": 600, "text": "Blackout" }
            ] })
        );
    }

    #[test]
    fn upgrade_should_turn_event_tracks_of_a_file_that_says_version_2() {
        let mut value = v1();
        value["formatVersion"] = json!(2);
        upgrade(&mut value, DATE);
        assert_eq!(value["timelines"][0]["tracks"][1]["kind"]["type"], "notes");
    }

    #[test]
    fn upgrade_should_drop_midi_triggers() {
        let mut value = v1();
        upgrade(&mut value, DATE);
        assert!(!value.to_string().contains("Program change"));
    }

    #[test]
    fn upgrade_should_keep_an_events_track_that_held_only_midi_as_an_empty_notes_track() {
        let mut value = json!({ "formatVersion": 1, "tracks": [
            { "id": "t", "kind": { "type": "events", "triggers": [
                { "id": "e", "at": 1, "label": "PC", "action": { "type": "midi", "bytes": [192] } }
            ] } }
        ] });
        upgrade(&mut value, DATE);
        assert_eq!(
            value["timelines"][0]["tracks"][0]["kind"],
            json!({ "type": "notes", "notes": [] })
        );
    }

    #[test]
    fn upgrade_should_leave_audio_tracks_and_the_rest_of_the_file_alone() {
        let mut value = v1();
        upgrade(&mut value, DATE);
        assert_eq!(value["timelines"][0]["tracks"][0], v1()["tracks"][0]);
        assert_eq!(value["name"], "Old show");
    }

    #[test]
    fn upgrade_should_write_the_current_version() {
        let mut value = v1();
        upgrade(&mut value, DATE);
        assert_eq!(value["formatVersion"], FORMAT_VERSION);
    }

    #[test]
    fn upgrade_should_give_a_file_without_meta_an_id_and_the_date_of_the_file() {
        let mut first = v1();
        let mut second = v1();
        upgrade(&mut first, DATE);
        upgrade(&mut second, DATE);

        let meta = &first["meta"];
        assert_eq!(meta["createdAt"], DATE);
        assert_eq!(meta["modifiedAt"], DATE);
        assert_eq!(meta["savedWith"], "");
        assert!(Uuid::parse_str(meta["id"].as_str().unwrap()).is_ok());
        assert_ne!(meta["id"], second["meta"]["id"]);
    }

    #[test]
    fn upgrade_should_keep_the_meta_a_file_already_has() {
        let mut value = json!({ "formatVersion": 5, "meta": { "id": "kept" }, "timelines": [] });
        let before = value.clone();
        upgrade(&mut value, DATE);
        assert_eq!(value["meta"], before["meta"]);
    }

    #[test]
    fn upgrade_should_leave_a_malformed_file_for_parsing_to_report() {
        let mut value = json!({ "formatVersion": 1, "tracks": "nope" });
        upgrade(&mut value, DATE);
        assert_eq!(value["timelines"][0]["tracks"], "nope");
    }

    fn v3() -> Value {
        json!({
            "formatVersion": 3,
            "name": "Show",
            "meta": { "id": "kept" },
            "timecode": { "frameRate": "fps25", "start": 90000 },
            "tracks": [{ "id": "t1" }],
            "output": {}
        })
    }

    #[test]
    fn upgrade_should_move_the_timecode_and_tracks_into_a_first_timeline() {
        let mut value = v3();
        upgrade(&mut value, DATE);
        let timeline = &value["timelines"][0];
        assert_eq!(timeline["timecode"], v3()["timecode"]);
        assert_eq!(timeline["tracks"], v3()["tracks"]);
    }

    #[test]
    fn upgrade_should_name_the_first_timeline_after_the_project() {
        let mut value = v3();
        upgrade(&mut value, DATE);
        assert_eq!(value["timelines"][0]["name"], "Show");
    }

    #[test]
    fn upgrade_should_give_the_first_timeline_an_id() {
        let mut value = v3();
        upgrade(&mut value, DATE);
        assert!(Uuid::parse_str(value["timelines"][0]["id"].as_str().unwrap()).is_ok());
    }

    #[test]
    fn upgrade_should_leave_no_timecode_or_tracks_at_the_top_level() {
        let mut value = v3();
        upgrade(&mut value, DATE);
        assert!(value.get("timecode").is_none() && value.get("tracks").is_none());
    }

    #[test]
    fn upgrade_should_keep_the_timelines_of_a_current_file() {
        let mut value =
            json!({ "formatVersion": 5, "meta": { "id": "kept" }, "timelines": [{ "id": "l1" }] });
        let before = value.clone();
        upgrade(&mut value, DATE);
        assert_eq!(value["timelines"], before["timelines"]);
    }

    #[test]
    fn upgrade_should_default_old_autosave_options_and_preserve_existing_options() {
        let mut old = v3();
        upgrade(&mut old, DATE);
        assert_eq!(
            old["autosave"],
            json!({ "enabled": true, "intervalSeconds": 30 })
        );
        old["autosave"] = json!({ "enabled": false, "intervalSeconds": 60 });
        let before = old.clone();
        upgrade(&mut old, DATE);
        assert_eq!(old, before);
    }
}
