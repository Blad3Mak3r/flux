//! Brings the `project.json` of an older format up to the current one, on the raw JSON before it is
//! parsed, so the model only ever has to describe the current shape. Opening a file never rewrites
//! it: the migrated project is what the app holds, and saving writes the current version.

use serde_json::{Value, json};

use super::model::FORMAT_VERSION;

/// Migrates `value`, a `project.json` of format `found`, to [`FORMAT_VERSION`]. A value that does
/// not have the shape of its version is left as it is, for parsing to report.
pub(super) fn upgrade(value: &mut Value, found: u32) {
    if found < 2 {
        events_to_notes(value);
    }
    if let Some(version) = value.get_mut("formatVersion") {
        *version = json!(FORMAT_VERSION);
    }
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
        upgrade(&mut value, 1);
        assert_eq!(
            value["tracks"][1]["kind"],
            json!({ "type": "notes", "notes": [
                { "id": "e1", "at": 300, "text": "Lights up" },
                { "id": "e3", "at": 600, "text": "Blackout" }
            ] })
        );
    }

    #[test]
    fn upgrade_should_drop_midi_triggers() {
        let mut value = v1();
        upgrade(&mut value, 1);
        let text = value.to_string();
        assert!(!text.contains("Program change"));
    }

    #[test]
    fn upgrade_should_keep_an_events_track_that_held_only_midi_as_an_empty_notes_track() {
        let mut value = json!({ "formatVersion": 1, "tracks": [
            { "id": "t", "kind": { "type": "events", "triggers": [
                { "id": "e", "at": 1, "label": "PC", "action": { "type": "midi", "bytes": [192] } }
            ] } }
        ] });
        upgrade(&mut value, 1);
        assert_eq!(
            value["tracks"][0]["kind"],
            json!({ "type": "notes", "notes": [] })
        );
    }

    #[test]
    fn upgrade_should_leave_audio_tracks_and_the_rest_of_the_file_alone() {
        let mut value = v1();
        upgrade(&mut value, 1);
        assert_eq!(value["tracks"][0], v1()["tracks"][0]);
        assert_eq!(value["name"], "Old show");
    }

    #[test]
    fn upgrade_should_write_the_current_version() {
        let mut value = v1();
        upgrade(&mut value, 1);
        assert_eq!(value["formatVersion"], FORMAT_VERSION);
    }

    #[test]
    fn upgrade_should_not_touch_a_file_that_is_already_current() {
        let mut value = json!({ "formatVersion": FORMAT_VERSION, "tracks": [
            { "id": "t", "kind": { "type": "notes", "notes": [] } }
        ] });
        let before = value.clone();
        upgrade(&mut value, FORMAT_VERSION);
        assert_eq!(value, before);
    }

    #[test]
    fn upgrade_should_leave_a_malformed_file_for_parsing_to_report() {
        let mut value = json!({ "formatVersion": 1, "tracks": "nope" });
        upgrade(&mut value, 1);
        assert_eq!(value["tracks"], "nope");
    }
}
