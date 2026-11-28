//! The keyboard shortcuts of a cue show: the actions, their default keys, and the user's changes.
//!
//! This table is the single source of truth. The window asks the backend for the effective keys of
//! every action and looks a key press up in them; it holds no key of its own. The changes are a
//! machine preference (`Preferences::shortcuts`), never part of a project. The VJ show has keys of
//! its own (`vj_shortcuts.json`) and is not touched here.
//!
//! A key combination is stored as text, `Mod+Alt+Shift+Key`, in that order, with the parts it has:
//! `Mod` is Ctrl, or Cmd on macOS, and `Key` is a character in upper case (`M`, `=`, `,`) or the name
//! of a key (`Space`, `ArrowLeft`, `PageUp`, `F2`). Shift is not written for a character that is not
//! a letter or a digit, since the character already carries it (`Mod++`, not `Mod+Shift+=`).

use serde::Serialize;
use std::collections::BTreeMap;

/// One thing the keyboard can do in a cue show.
pub struct ActionDef {
    pub id: &'static str,
    /// The group the shortcuts page lists it under.
    pub group: &'static str,
    pub keys: &'static [&'static str],
    /// Refused while the show is locked.
    pub locked: bool,
    /// Only in the editor: Live only follows the show.
    pub edit_only: bool,
}

const fn action(
    id: &'static str,
    group: &'static str,
    keys: &'static [&'static str],
    locked: bool,
    edit_only: bool,
) -> ActionDef {
    ActionDef {
        id,
        group,
        keys,
        locked,
        edit_only,
    }
}

/// Every action with its default keys, in the order the shortcuts page lists them.
pub const ACTIONS: &[ActionDef] = &[
    action("playPause", "transport", &["Space"], false, false),
    action("stop", "transport", &["Home"], false, false),
    action("frameBack", "transport", &["ArrowLeft"], false, false),
    action("frameForward", "transport", &["ArrowRight"], false, false),
    action(
        "secondBack",
        "transport",
        &["Shift+ArrowLeft"],
        false,
        false,
    ),
    action(
        "secondForward",
        "transport",
        &["Shift+ArrowRight"],
        false,
        false,
    ),
    action("panic", "transport", &["Mod+Shift+P"], false, false),
    action("new", "file", &["Mod+N"], true, false),
    action("open", "file", &["Mod+O"], true, false),
    action("save", "file", &["Mod+S"], false, false),
    action("saveAs", "file", &["Mod+Shift+S"], false, false),
    action("close", "file", &["Mod+W"], true, false),
    action("setup", "file", &["Mod+,"], false, false),
    action("importAudio", "file", &["I"], true, false),
    action("undo", "edit", &["Mod+Z"], true, false),
    action("redo", "edit", &["Mod+Shift+Z", "Mod+Y"], true, false),
    action("selectAll", "edit", &["Mod+A"], false, true),
    action("delete", "edit", &["Delete", "Backspace"], true, false),
    action("deselect", "edit", &["Escape"], false, false),
    action("split", "edit", &["S"], false, true),
    action("addNote", "edit", &["N"], false, true),
    action("renameTrack", "edit", &["F2"], false, true),
    action("toggleSnap", "edit", &["G"], false, true),
    action("cycleRipple", "edit", &["Alt+P"], false, true),
    action("addMarker", "timeline", &["M"], false, true),
    action("addRegion", "timeline", &["R"], false, true),
    action(
        "previousMarker",
        "timeline",
        &["Alt+ArrowLeft"],
        false,
        true,
    ),
    action("nextMarker", "timeline", &["Alt+ArrowRight"], false, true),
    action("previousTimeline", "timeline", &["Mod+PageUp"], true, false),
    action("nextTimeline", "timeline", &["Mod+PageDown"], true, false),
    action("viewEdit", "view", &["Mod+1"], true, false),
    action("viewLive", "view", &["Mod+2"], false, false),
    action("zoomIn", "view", &["Mod+=", "Mod++"], false, false),
    action("zoomOut", "view", &["Mod+-"], false, false),
    action("zoomFit", "view", &["Mod+0"], false, true),
    action("toggleInspector", "view", &["Mod+B"], false, false),
];

/// The changes to the defaults: the keys an action has now, by action. An empty list is an action
/// with no key.
pub type Overrides = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("unknown shortcut action")]
    UnknownAction,
    #[error("not a valid key combination")]
    BadCombo,
    /// The text is `conflict:` and the id of the action that has the keys, which the window words.
    #[error("conflict:{0}")]
    Conflict(&'static str),
}

const NAMED: &[&str] = &[
    "Space",
    "Escape",
    "Delete",
    "Backspace",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Tab",
    "Enter",
    "Insert",
];

fn valid_key(key: &str) -> bool {
    NAMED.contains(&key)
        || (key.chars().count() == 1 && key.chars().all(|c| !c.is_whitespace() && !c.is_control()))
        || key
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=12).contains(&n))
}

/// The canonical text of a key combination, or why it is not one.
pub fn canonical(text: &str) -> Result<String, KeymapError> {
    let (mut cmd, mut alt, mut shift) = (false, false, false);
    let mut rest = text;
    loop {
        let (flag, tail) = if let Some(tail) = rest.strip_prefix("Mod+") {
            (&mut cmd, tail)
        } else if let Some(tail) = rest.strip_prefix("Alt+") {
            (&mut alt, tail)
        } else if let Some(tail) = rest.strip_prefix("Shift+") {
            (&mut shift, tail)
        } else {
            break;
        };
        if *flag {
            return Err(KeymapError::BadCombo);
        }
        *flag = true;
        rest = tail;
    }
    let key = if rest.chars().count() == 1 {
        rest.to_uppercase()
    } else {
        rest.to_owned()
    };
    if !valid_key(&key) {
        return Err(KeymapError::BadCombo);
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && !c.is_alphanumeric()
    {
        shift = false;
    }
    let mut out = String::new();
    for (on, part) in [(cmd, "Mod+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(part);
        }
    }
    out.push_str(&key);
    Ok(out)
}

fn def(id: &str) -> Result<&'static ActionDef, KeymapError> {
    ACTIONS
        .iter()
        .find(|a| a.id == id)
        .ok_or(KeymapError::UnknownAction)
}

/// The keys an action has now.
pub fn keys(overrides: &Overrides, action: &ActionDef) -> Vec<String> {
    overrides
        .get(action.id)
        .cloned()
        .unwrap_or_else(|| action.keys.iter().map(|k| (*k).to_owned()).collect())
}

/// The action that has these keys now, other than `except`.
fn holder(overrides: &Overrides, combo: &str, except: &str) -> Option<&'static str> {
    ACTIONS
        .iter()
        .filter(|a| a.id != except)
        .find(|a| keys(overrides, a).iter().any(|k| k == combo))
        .map(|a| a.id)
}

/// Whether stored changes are usable: known actions, canonical keys, no key on two actions.
pub fn validate(overrides: &Overrides) -> Result<(), KeymapError> {
    let mut seen: BTreeMap<String, &'static str> = BTreeMap::new();
    for id in overrides.keys() {
        def(id)?;
    }
    for a in ACTIONS {
        for combo in keys(overrides, a) {
            if canonical(&combo)? != combo {
                return Err(KeymapError::BadCombo);
            }
            if let Some(other) = seen.insert(combo, a.id) {
                return Err(KeymapError::Conflict(other));
            }
        }
    }
    Ok(())
}

/// Gives an action these keys alone, or none. Refuses keys another action has.
pub fn set(overrides: &mut Overrides, id: &str, combo: Option<&str>) -> Result<(), KeymapError> {
    let action = def(id)?;
    let new = match combo {
        Some(text) => {
            let combo = canonical(text)?;
            if let Some(other) = holder(overrides, &combo, id) {
                return Err(KeymapError::Conflict(other));
            }
            vec![combo]
        }
        None => Vec::new(),
    };
    if new == action.keys {
        overrides.remove(id);
    } else {
        overrides.insert(id.to_owned(), new);
    }
    Ok(())
}

/// Back to the default keys: one action's, or all.
pub fn reset(overrides: &mut Overrides, id: Option<&str>) -> Result<(), KeymapError> {
    match id {
        Some(id) => {
            def(id)?;
            // Refused when another action has taken its keys since.
            if let Some(old) = overrides.remove(id)
                && let Err(error) = validate(overrides)
            {
                overrides.insert(id.to_owned(), old);
                return Err(error);
            }
        }
        None => overrides.clear(),
    }
    Ok(())
}

/// What the window gets for an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionView {
    pub id: &'static str,
    pub group: &'static str,
    pub keys: Vec<String>,
    pub defaults: Vec<String>,
    pub locked: bool,
    pub edit_only: bool,
}

pub fn view(overrides: &Overrides) -> Vec<ActionView> {
    ACTIONS
        .iter()
        .map(|a| ActionView {
            id: a.id,
            group: a.group,
            keys: keys(overrides, a),
            defaults: a.keys.iter().map(|k| (*k).to_owned()).collect(),
            locked: a.locked,
            edit_only: a.edit_only,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_canonical_and_do_not_conflict() {
        assert_eq!(validate(&Overrides::new()), Ok(()));
        let ids: std::collections::BTreeSet<_> = ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), ACTIONS.len());
    }

    #[test]
    fn combos_are_normalised() {
        assert_eq!(canonical("Shift+Mod+s").as_deref(), Ok("Mod+Shift+S"));
        assert_eq!(
            canonical("Alt+Mod+ArrowLeft").as_deref(),
            Ok("Mod+Alt+ArrowLeft")
        );
        assert_eq!(canonical("Mod++").as_deref(), Ok("Mod++"));
        assert_eq!(canonical("Mod+Shift+=").as_deref(), Ok("Mod+="));
        assert_eq!(canonical("Shift+1").as_deref(), Ok("Shift+1"));
        assert_eq!(canonical("m").as_deref(), Ok("M"));
        assert_eq!(canonical("F12").as_deref(), Ok("F12"));
        for bad in [
            "",
            "Mod+",
            "Mod+Mod+S",
            "Ctrl+S",
            "Mod+Nope",
            "F13",
            "Mod+ ",
        ] {
            assert_eq!(canonical(bad), Err(KeymapError::BadCombo), "{bad}");
        }
    }

    #[test]
    fn a_key_in_use_is_refused() {
        let mut o = Overrides::new();
        assert_eq!(
            set(&mut o, "addMarker", Some("R")),
            Err(KeymapError::Conflict("addRegion"))
        );
        assert_eq!(
            set(&mut o, "addMarker", Some("Mod+Y"))
                .unwrap_err()
                .to_string(),
            "conflict:redo"
        );
        assert!(o.is_empty());
        assert_eq!(
            set(&mut o, "nope", Some("K")),
            Err(KeymapError::UnknownAction)
        );
    }

    #[test]
    fn set_clear_and_reset() {
        let mut o = Overrides::new();
        let marker = |o: &Overrides| {
            view(o)
                .into_iter()
                .find(|a| a.id == "addMarker")
                .unwrap()
                .keys
        };
        set(&mut o, "addMarker", Some("k")).unwrap();
        assert_eq!(marker(&o), ["K"]);
        // M is free now, and K is taken.
        set(&mut o, "addNote", Some("M")).unwrap();
        assert_eq!(
            set(&mut o, "addRegion", Some("K")),
            Err(KeymapError::Conflict("addMarker"))
        );
        set(&mut o, "addMarker", None).unwrap();
        assert!(marker(&o).is_empty());
        // Back to M while addNote holds it is refused and changes nothing.
        assert_eq!(
            reset(&mut o, Some("addMarker")),
            Err(KeymapError::Conflict("addNote"))
        );
        assert!(marker(&o).is_empty());
        reset(&mut o, None).unwrap();
        assert!(o.is_empty());
        // Setting the default again leaves no change behind.
        set(&mut o, "addMarker", Some("M")).unwrap();
        assert!(o.is_empty());
    }
}
