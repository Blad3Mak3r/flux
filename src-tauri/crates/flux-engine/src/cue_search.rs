//! Finding a cue by what the operator types in the Live view: its number, its name or its text.
//!
//! The match is made here, not in the window, so every front end agrees on which cue a query means.
//! The caller goes to the cue the user picks with the same seek as a click on its row.

use crate::cues::Cue;

/// The ids of the cues that match `query`, best first.
///
/// A query that is a cue's number puts that cue first; then come the cues, in show order, whose
/// name, text, track or notation contain the query in any letter case. An empty query matches
/// every cue, in show order.
pub fn search(cues: &[Cue], query: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return cues.iter().map(|cue| cue.id.clone()).collect();
    }
    let needle = query.to_lowercase();
    let number = query.trim_start_matches('#').parse::<u32>().ok();
    let named = |cue: &Cue| {
        [
            Some(cue.name.as_str()),
            cue.text.as_deref(),
            Some(cue.track_name.as_str()),
            cue.detail.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|field| field.to_lowercase().contains(&needle))
    };
    let (exact, rest): (Vec<&Cue>, Vec<&Cue>) = cues
        .iter()
        .filter(|cue| Some(cue.number) == number || named(cue))
        .partition(|cue| Some(cue.number) == number);
    exact
        .into_iter()
        .chain(rest)
        .map(|cue| cue.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cues::CueList;
    use flux_project::{FrameRate, Note, Project, Track, TrackKind};

    fn note(id: &str, at: u64, text: &str) -> Note {
        Note {
            id: id.into(),
            at,
            length: None,
            text: text.into(),
        }
    }

    /// Cues 1 to 12: eleven notes called "Beat N" and, last, "Finale 1" on a track named "Stage".
    fn cues() -> Vec<Cue> {
        let mut project = Project::new("Show", FrameRate::Fps25);
        let mut notes: Vec<Note> = (1..12)
            .map(|n| note(&format!("n{n}"), n * 10, &format!("Beat {n}")))
            .collect();
        notes.push(note("fin", 500, "Finale 1\nlights out"));
        project.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "t".into(),
            name: "Stage".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Notes { notes },
        });
        CueList::build(&project, &project.timelines[0], |_| None)
            .cues()
            .to_vec()
    }

    #[test]
    fn an_empty_query_should_match_every_cue_in_show_order() {
        let found = search(&cues(), "  ");
        assert_eq!(found.len(), 12);
        assert_eq!(found[0], "n1");
        assert_eq!(found[11], "fin");
    }

    #[test]
    fn a_name_should_match_in_any_letter_case() {
        assert_eq!(search(&cues(), "FINALE"), ["fin"]);
    }

    #[test]
    fn the_text_and_the_track_should_match_too() {
        assert_eq!(search(&cues(), "lights out"), ["fin"]);
        assert_eq!(search(&cues(), "stage").len(), 12);
    }

    #[test]
    fn a_number_should_put_that_cue_first_then_the_names_that_contain_it() {
        assert_eq!(search(&cues(), "1"), ["n1", "n10", "n11", "fin"]);
        assert_eq!(search(&cues(), "#12")[0], "fin");
    }

    #[test]
    fn a_query_that_matches_nothing_should_give_no_cue() {
        assert!(search(&cues(), "zzz").is_empty());
    }
}
