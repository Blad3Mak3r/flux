//! Writing and reading the text of a `.flxmap` file.

use super::*;
use crate::vj::Vj;
use crate::vj_mapping::{Follow, Link};
use crate::{ProjectError, Result};

/// The file of the mappings and dashboard links of `vj`, as the text to write. What cannot be
/// named by position (nothing a valid grid holds) is left out.
pub fn export(vj: &Vj) -> Result<String> {
    let mappings = vj
        .mappings
        .iter()
        .filter_map(|m| {
            let target = match (&m.place, m.follow) {
                (Some(place), Follow::Position) => place.clone(),
                _ => to_places(vj, &m.target)?,
            };
            Some(FileMapping {
                source: m.source.clone(),
                target,
                ids: (!m.parked).then(|| m.target.clone()),
                follow: m.follow,
                min: m.min,
                max: m.max,
                invert: m.invert,
                mode: m.mode,
                lights: m.lights,
            })
        })
        .collect();
    let dashboard = vj
        .dashboard
        .iter()
        .map(|knob| FileKnob {
            name: knob.name.clone(),
            links: knob
                .links
                .iter()
                .filter_map(|link| {
                    Some(Link {
                        target: to_places(vj, &link.target)?,
                        ..link.clone()
                    })
                })
                .collect(),
        })
        .collect();
    let file = MapFile {
        format: FORMAT.into(),
        version: VERSION,
        mappings,
        dashboard,
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

/// Reads the text of a `.flxmap`, refusing what is not one and a version newer than this one.
pub fn parse(text: &str) -> Result<MapFile> {
    let file: MapFile = serde_json::from_str(text)
        .map_err(|err| ProjectError::Invalid(format!("not a mappings file: {err}")))?;
    if file.format != FORMAT {
        return Err(ProjectError::Invalid("not a mappings file".into()));
    }
    if file.version > VERSION {
        return Err(ProjectError::Invalid(format!(
            "the mappings file is version {}, and this FLUX reads up to {VERSION}",
            file.version
        )));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vj_mapfile::support::*;

    #[test]
    fn a_file_of_a_newer_version_or_another_kind_should_be_refused() {
        let text = export(&mapped().vj).unwrap();
        let newer = text.replace("\"version\": 1", "\"version\": 2");
        let other = text.replace(FORMAT, "something-else");

        assert!(parse(&newer).is_err());
        assert!(parse(&other).is_err());
        assert!(parse("not json").is_err());
        assert!(parse(&text).is_ok());
    }

    #[test]
    fn the_file_should_name_places_and_never_ids() {
        let text = export(&mapped().vj).unwrap();

        let file = parse(&text).unwrap();
        let ids_in_places = format!(
            "{:?}",
            file.mappings.iter().map(|m| &m.target).collect::<Vec<_>>()
        );
        assert!(!ids_in_places.contains("\"a"), "{ids_in_places}");
        assert!(text.contains("\"layer\": \"1\""));
        assert!(text.contains("\"column\": \"0.2\""));
        assert!(text.contains("\"effect\": \"l1:0\""));
    }
}
