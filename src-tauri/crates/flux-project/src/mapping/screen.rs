//! Screens: the windows of the output, each on a display of its own.

use serde::{Deserialize, Serialize};

use super::*;
use crate::{ProjectError, Result};

/// A window of the output, on a display of its own, and the slices drawn on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoScreen {
    pub id: String,
    pub name: String,
    /// None is the whole picture fitted to the screen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slices: Vec<Slice>,
    /// Drawn over the slices, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
    /// Off: the window stays open and shows black.
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

fn is_enabled(on: &bool) -> bool {
    *on
}

impl VideoScreen {
    fn main() -> Self {
        Self {
            id: MAIN_SCREEN.into(),
            name: "Screen 1".into(),
            slices: Vec::new(),
            masks: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScreenOp {
    /// A new screen with no slices, named "Screen n".
    Add,
    /// Any screen but the main one.
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    /// Off: the screen's window outputs black. The main screen too.
    Enable {
        id: String,
        enabled: bool,
    },
}

/// The screens of an output with the main one first, as `screens` lists them or made when it does
/// not.
pub fn screens(screens: &[VideoScreen]) -> Vec<VideoScreen> {
    let mut all = screens.to_vec();
    with_main(&mut all);
    all
}

fn with_main(screens: &mut Vec<VideoScreen>) {
    if !screens.iter().any(|screen| screen.id == MAIN_SCREEN) {
        screens.insert(0, VideoScreen::main());
    }
}

/// The screen `id` of `screens`, the main one added first when it is missing.
pub fn screen_mut<'a>(screens: &'a mut Vec<VideoScreen>, id: &str) -> Result<&'a mut VideoScreen> {
    with_main(screens);
    screens
        .iter_mut()
        .find(|screen| screen.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no screen {id}")))
}

/// Applies `op` to `screens`; `new_id` names a new screen. The caller validates the result.
pub fn edit_screens(
    screens: &mut Vec<VideoScreen>,
    op: ScreenOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    with_main(screens);
    match op {
        ScreenOp::Add => {
            if screens.len() >= MAX_SCREENS {
                return Err(ProjectError::Invalid("too many screens".into()));
            }
            let number = (1..)
                .find(|n| !screens.iter().any(|s| s.name == format!("Screen {n}")))
                .unwrap_or(1);
            screens.push(VideoScreen {
                id: new_id(),
                name: format!("Screen {number}"),
                slices: Vec::new(),
                masks: Vec::new(),
                enabled: true,
            });
        }
        ScreenOp::Remove { id } => {
            if id == MAIN_SCREEN {
                return Err(ProjectError::Invalid("the main screen stays".into()));
            }
            let before = screens.len();
            screens.retain(|screen| screen.id != id);
            if screens.len() == before {
                return Err(ProjectError::Invalid(format!("no screen {id}")));
            }
        }
        ScreenOp::Rename { id, name } => {
            screen_mut(screens, &id)?.name = name.trim().to_owned();
        }
        ScreenOp::Enable { id, enabled } => screen_mut(screens, &id)?.enabled = enabled,
    }
    Ok(())
}

pub fn validate_screens(screens: &[VideoScreen]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video screens: {why}")));
    if screens.len() > MAX_SCREENS {
        return invalid("too many screens");
    }
    let mut ids = std::collections::HashSet::new();
    for screen in screens {
        if screen.id.is_empty() || !ids.insert(screen.id.as_str()) {
            return invalid("a screen id is empty or repeated");
        }
        if screen.name.is_empty() || screen.name.chars().count() > MAX_NAME {
            return invalid("a screen name is empty or too long");
        }
        validate(&screen.slices)?;
        validate_masks(&screen.masks)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screens_should_keep_the_main_one() {
        let mut list = Vec::new();
        edit_screens(&mut list, ScreenOp::Add, || "s2".into()).unwrap();
        assert_eq!(
            list.iter()
                .map(|s| (s.id.as_str(), s.name.as_str()))
                .collect::<Vec<_>>(),
            [(MAIN_SCREEN, "Screen 1"), ("s2", "Screen 2")]
        );
        let remove_main = ScreenOp::Remove {
            id: MAIN_SCREEN.into(),
        };
        assert!(edit_screens(&mut list, remove_main, String::new).is_err());
        let rename = ScreenOp::Rename {
            id: "s2".into(),
            name: " Left wall ".into(),
        };
        edit_screens(&mut list, rename, String::new).unwrap();
        assert_eq!(list[1].name, "Left wall");
        apply(
            &mut screen_mut(&mut list, "s2").unwrap().slices,
            MappingOp::Add,
            || "a".into(),
        )
        .unwrap();
        assert!(validate_screens(&list).is_ok());
        edit_screens(&mut list, ScreenOp::Remove { id: "s2".into() }, String::new).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(screens(&[])[0].id, MAIN_SCREEN);
        assert!(screen_mut(&mut list, "gone").is_err());
    }

    #[test]
    fn screens_should_switch_off_and_default_on() {
        let mut list = Vec::new();
        let off = ScreenOp::Enable {
            id: MAIN_SCREEN.into(),
            enabled: false,
        };
        edit_screens(&mut list, off, String::new).unwrap();
        assert!(!list[0].enabled);
        let json = serde_json::to_string(&list[0]).unwrap();
        assert!(json.contains("\"enabled\":false"));
        let on = ScreenOp::Enable {
            id: MAIN_SCREEN.into(),
            enabled: true,
        };
        edit_screens(&mut list, on, String::new).unwrap();
        assert!(!serde_json::to_string(&list[0]).unwrap().contains("enabled"));
        let old: VideoScreen = serde_json::from_str(r#"{"id":"a","name":"A"}"#).unwrap();
        assert!(old.enabled);
        let gone = ScreenOp::Enable {
            id: "gone".into(),
            enabled: false,
        };
        assert!(edit_screens(&mut list, gone, String::new).is_err());
    }
}
