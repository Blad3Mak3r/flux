//! The cue list written out for people who do not look at FLUX: a CSV for a spreadsheet and a
//! self-contained HTML page for the printer.
//!
//! Both come from the cues the Live view already shows ([`crate::cues`]), so the sheet and the
//! screen cannot disagree. Cues on muted tracks never play and are left out. The words come from
//! the user's language, so the caller passes them in [`SheetLabels`]; nothing here reads a device or
//! waits.

use std::collections::HashMap;
use std::fmt::Write;

use serde::Deserialize;

use crate::cues::{Cue, CueList};
use flux_project::FrameRate;

/// The words of a sheet, in the user's language.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetLabels {
    /// The document's language, e.g. `en`, for the HTML `lang` attribute.
    pub lang: String,
    /// Name of the document, e.g. "Cue sheet".
    pub title: String,
    pub timeline: String,
    pub frame_rate: String,
    /// What a timeline with no cues says.
    pub empty: String,
    pub number: String,
    pub timecode: String,
    pub name: String,
    pub kind: String,
    pub action: String,
    pub detail: String,
    pub track: String,
    pub duration: String,
    /// By [`CueKind::key`](crate::cues::CueKind::key).
    pub kinds: HashMap<String, String>,
    /// By [`CueAction::key`](crate::cues::CueAction::key).
    pub actions: HashMap<String, String>,
    /// By the frame rate's serialized name: `fps24`, `fps25`, `fps2997Drop`, `fps30`.
    pub rates: HashMap<String, String>,
}

/// One timeline of the sheet.
pub struct SheetTimeline<'a> {
    pub name: &'a str,
    pub cues: &'a CueList,
}

fn rate_key(rate: FrameRate) -> &'static str {
    match rate {
        FrameRate::Fps24 => "fps24",
        FrameRate::Fps25 => "fps25",
        FrameRate::Fps2997Drop => "fps2997Drop",
        FrameRate::Fps30 => "fps30",
    }
}

fn pick<'a>(words: &'a HashMap<String, String>, key: &'a str) -> &'a str {
    words.get(key).map_or(key, String::as_str)
}

/// What a row says besides its timecode, already in the user's words.
struct Row<'a> {
    number: u32,
    timecode: &'a str,
    name: &'a str,
    kind: &'a str,
    action: &'a str,
    detail: &'a str,
    track: &'a str,
    duration: &'a str,
}

fn row<'a>(cue: &'a Cue, labels: &'a SheetLabels) -> Row<'a> {
    Row {
        number: cue.number,
        timecode: &cue.timecode,
        // A note's whole text, not only its first line: the sheet is where it is read in full.
        name: cue.text.as_deref().unwrap_or(&cue.name),
        kind: pick(&labels.kinds, cue.kind.key()),
        action: cue.action.map_or("", |a| pick(&labels.actions, a.key())),
        detail: cue.detail.as_deref().unwrap_or(""),
        track: &cue.track_name,
        duration: cue.duration.as_deref().unwrap_or(""),
    }
}

/// The cues that play, in the order the show reaches them.
fn playing(list: &CueList) -> impl Iterator<Item = &Cue> {
    list.cues().iter().filter(|cue| !cue.muted)
}

// ---- CSV ----

/// A CSV cell. A text that starts like a formula gets a leading `'`, so a spreadsheet shows it
/// instead of running it: names and notes are typed by people and travel to other people.
fn csv_cell(text: &str) -> String {
    let guarded = text.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let text = if guarded {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn csv_line(cells: &[&str]) -> String {
    let mut line = cells
        .iter()
        .map(|cell| csv_cell(cell))
        .collect::<Vec<_>>()
        .join(",");
    line.push_str("\r\n");
    line
}

/// One row per cue, every timeline in one table. UTF-8 with a byte order mark, which is what makes
/// Excel read the accents; lines end in CRLF (RFC 4180).
pub fn csv(timelines: &[SheetTimeline], labels: &SheetLabels) -> String {
    let mut out = String::from('\u{feff}');
    out.push_str(&csv_line(&[
        &labels.timeline,
        &labels.number,
        &labels.timecode,
        &labels.name,
        &labels.kind,
        &labels.action,
        &labels.detail,
        &labels.track,
        &labels.duration,
    ]));
    for timeline in timelines {
        for cue in playing(timeline.cues) {
            let r = row(cue, labels);
            out.push_str(&csv_line(&[
                timeline.name,
                &r.number.to_string(),
                r.timecode,
                r.name,
                r.kind,
                r.action,
                r.detail,
                r.track,
                r.duration,
            ]));
        }
    }
    out
}

// ---- HTML ----

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = "\
body{font:14px/1.4 system-ui,-apple-system,'Segoe UI',sans-serif;color:#111;background:#fff;margin:2rem}\
h1{font-size:1.6rem;margin:0}\
.doc{margin:.2rem 0 1.5rem;color:#555}\
section{margin-bottom:2.5rem}\
h2{font-size:1.15rem;margin:0}\
.rate{margin:.1rem 0 .6rem;color:#555}\
table{width:100%;border-collapse:collapse}\
th,td{text-align:left;vertical-align:top;padding:.3rem .6rem;border-bottom:1px solid #ccc}\
th{border-bottom:2px solid #111;font-size:.8rem;text-transform:uppercase;letter-spacing:.04em}\
td.num,td.tc,td.dur{font-variant-numeric:tabular-nums;white-space:nowrap}\
td.tc{font-weight:600}\
td.text{white-space:pre-line}\
.empty{color:#555}\
@page{margin:1.5cm}\
@media print{body{margin:0}section{break-before:page}section:first-of-type{break-before:auto}\
thead{display:table-header-group}tr{break-inside:avoid}}";

/// A page that stands on its own: no scripts, no outside files, light on white for the printer.
pub fn html(project: &str, timelines: &[SheetTimeline], labels: &SheetLabels) -> String {
    let mut out = String::new();
    // Writing into a String cannot fail.
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} · {}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
         <h1>{}</h1>\n<p class=\"doc\">{}</p>\n",
        escape(&labels.lang),
        escape(project),
        escape(&labels.title),
        escape(project),
        escape(&labels.title),
    );
    for timeline in timelines {
        let rate = pick(&labels.rates, rate_key(timeline.cues.rate()));
        let _ = write!(
            out,
            "<section>\n<h2>{}</h2>\n<p class=\"rate\">{}: {}</p>\n",
            escape(timeline.name),
            escape(&labels.frame_rate),
            escape(rate),
        );
        let mut cues = playing(timeline.cues).peekable();
        if cues.peek().is_none() {
            let _ = writeln!(out, "<p class=\"empty\">{}</p>", escape(&labels.empty));
        } else {
            out.push_str("<table>\n<thead><tr>");
            for head in [
                &labels.number,
                &labels.timecode,
                &labels.name,
                &labels.kind,
                &labels.action,
                &labels.detail,
                &labels.track,
                &labels.duration,
            ] {
                let _ = write!(out, "<th>{}</th>", escape(head));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            for cue in cues {
                let r = row(cue, labels);
                let _ = writeln!(
                    out,
                    "<tr><td class=\"num\">{}</td><td class=\"tc\">{}</td><td class=\"text\">{}</td>\
                     <td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"dur\">{}</td></tr>",
                    r.number,
                    escape(r.timecode),
                    escape(r.name),
                    escape(r.kind),
                    escape(r.action),
                    escape(r.detail),
                    escape(r.track),
                    escape(r.duration),
                );
            }
            out.push_str("</tbody>\n</table>\n");
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::ENGINE_RATE;
    use flux_project::{AudioAsset, AudioClip, Note, Project, Track, TrackKind};

    const SECOND: u64 = ENGINE_RATE as u64;

    fn track(id: &str, name: &str, muted: bool, kind: TrackKind) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: name.into(),
            muted,
            gain_db: None,
            color: None,
            kind,
        }
    }

    fn note(id: &str, at: u64, text: &str) -> Note {
        Note {
            id: id.into(),
            at,
            length: None,
            text: text.into(),
        }
    }

    /// 25 fps from 01:00:00:00 (frame 90 000): a two-second song at 0, tricky notes, and a muted
    /// notes track.
    fn project(rate: FrameRate) -> Project {
        let mut project = Project::new("Show", rate);
        project.timelines[0].timecode.start = 90_000;
        project.assets.push(AudioAsset {
            id: "a1".into(),
            name: "Intro".into(),
            file: "audio/intro.wav".into(),
            bpm: None,
        });
        let tracks = &mut project.timelines[0].tracks;
        tracks.push(track(
            "t1",
            "Playback",
            false,
            TrackKind::Audio {
                clips: vec![AudioClip::new("c1", "a1", 0)],
            },
        ));
        tracks.push(track(
            "t2",
            "Lights, \"front\"",
            false,
            TrackKind::Notes {
                notes: vec![
                    note("n1", 50, "Blackout, then \"go\"\nsecond line"),
                    note("n2", 75, "=SUM(A1)"),
                ],
            },
        ));
        tracks.push(track(
            "t3",
            "Hidden",
            true,
            TrackKind::Notes {
                notes: vec![note("n3", 10, "Never plays")],
            },
        ));
        project
    }

    fn labels() -> SheetLabels {
        let pairs = |items: &[(&str, &str)]| {
            items
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        SheetLabels {
            lang: "es".into(),
            title: "Hoja de cues".into(),
            timeline: "Línea".into(),
            frame_rate: "Fotogramas".into(),
            empty: "Sin cues".into(),
            number: "N.º".into(),
            timecode: "Timecode".into(),
            name: "Nombre".into(),
            kind: "Tipo".into(),
            action: "Acción".into(),
            detail: "Detalle".into(),
            track: "Pista".into(),
            duration: "Duración".into(),
            kinds: pairs(&[("audio", "Audio"), ("note", "Nota")]),
            actions: pairs(&[]),
            rates: pairs(&[("fps25", "25 fps"), ("fps2997Drop", "29.97 fps DF")]),
        }
    }

    fn cues(project: &Project) -> CueList {
        CueList::build(project, &project.timelines[0], |_| Some(2 * SECOND))
    }

    #[test]
    fn csv_should_list_the_playing_cues_in_show_order_with_a_header() {
        let p = project(FrameRate::Fps25);
        let list = cues(&p);
        let out = csv(
            &[SheetTimeline {
                name: &p.timelines[0].name,
                cues: &list,
            }],
            &labels(),
        );
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert!(lines[0].starts_with("\u{feff}Línea,N.º,Timecode,Nombre,Tipo,"));
        assert!(lines[1].contains(",1,01:00:00:00,Intro,Audio,,,Playback,00:00:02:00"));
        // The muted cue is #2 in the Live list; the sheet keeps Live's numbers, so the number on
        // paper is the one on screen.
        assert!(lines[2].contains(",3,01:00:02:00,"), "{}", lines[2]);
        assert!(!out.contains("Never plays"), "a muted track is left out");
    }

    #[test]
    fn csv_should_escape_commas_quotes_and_newlines() {
        let p = project(FrameRate::Fps25);
        let list = cues(&p);
        let out = csv(
            &[SheetTimeline {
                name: "Show",
                cues: &list,
            }],
            &labels(),
        );
        assert!(out.contains("\"Blackout, then \"\"go\"\"\nsecond line\""));
        assert!(out.contains("\"Lights, \"\"front\"\"\""));
    }

    #[test]
    fn csv_should_not_let_a_spreadsheet_run_a_note_as_a_formula() {
        let p = project(FrameRate::Fps25);
        let list = cues(&p);
        let out = csv(
            &[SheetTimeline {
                name: "Show",
                cues: &list,
            }],
            &labels(),
        );
        assert!(out.contains(",'=SUM(A1),"));
        assert!(!out.contains(",=SUM(A1),"));
    }

    #[test]
    fn csv_should_write_drop_frame_timecode_with_the_drop_separator() {
        let p = project(FrameRate::Fps2997Drop);
        let list = cues(&p);
        let out = csv(
            &[SheetTimeline {
                name: "Show",
                cues: &list,
            }],
            &labels(),
        );
        let expected = list.cues()[0].timecode.clone();
        assert!(expected.contains(';'), "drop-frame uses ';': {expected}");
        assert!(out.contains(&expected));
    }

    #[test]
    fn csv_should_hold_every_timeline_in_one_table() {
        let p = project(FrameRate::Fps25);
        let list = cues(&p);
        let out = csv(
            &[
                SheetTimeline {
                    name: "First",
                    cues: &list,
                },
                SheetTimeline {
                    name: "Second",
                    cues: &list,
                },
            ],
            &labels(),
        );
        assert!(out.contains("\r\nFirst,1,"));
        assert!(out.contains("\r\nSecond,1,"));
    }

    #[test]
    fn html_should_stand_alone_and_escape_what_people_typed() {
        let mut p = project(FrameRate::Fps25);
        p.timelines[0].tracks.push(track(
            "t4",
            "<b>Bold</b> & co",
            false,
            TrackKind::Notes {
                notes: vec![note("n4", 100, "<script>alert('x')</script>")],
            },
        ));
        let list = cues(&p);
        let out = html(
            "Show <1>",
            &[SheetTimeline {
                name: "Act \"I\"",
                cues: &list,
            }],
            &labels(),
        );
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("<html lang=\"es\">"));
        assert!(!out.contains("<script"), "nothing typed becomes markup");
        assert!(out.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
        assert!(out.contains("&lt;b&gt;Bold&lt;/b&gt; &amp; co"));
        assert!(out.contains("<h1>Show &lt;1&gt;</h1>"));
        assert!(out.contains("<h2>Act &quot;I&quot;</h2>"));
        assert!(out.contains("Fotogramas: 25 fps"));
        assert!(!out.contains("Never plays"));
        assert!(!out.contains("src="), "no outside files");
    }

    #[test]
    fn html_should_say_so_when_a_timeline_has_no_cues() {
        let p = Project::new("Empty", FrameRate::Fps25);
        let list = cues(&p);
        let out = html(
            "Empty",
            &[SheetTimeline {
                name: "Main",
                cues: &list,
            }],
            &labels(),
        );
        assert!(out.contains("<p class=\"empty\">Sin cues</p>"));
        assert!(!out.contains("<table>"));
    }
}
