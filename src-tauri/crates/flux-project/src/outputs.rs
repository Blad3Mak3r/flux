//! Portable show signals. Physical routing is supplied by the local installation.
use serde::{Deserialize, Serialize};

/// Explicitly unassigned physical audio route; it must never open a default device.
pub const UNASSIGNED_DEVICE: &str = "__flux_unassigned__";

use crate::{OscDestination, OutputConfig, ProjectError, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowOutputs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metronome: Option<ClickSettings>,
    pub ltc: Vec<LtcSignal>,
    pub mtc: Vec<MtcSignal>,
    pub osc: Vec<OscDestination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_routes: Option<OutputConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickSettings {
    #[serde(default)]
    pub gain_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LtcSignal {
    pub slot: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MtcSignal {
    pub slot: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum SignalOp {
    AddLtc,
    AddMtc,
    RemoveLtc { slot: u32 },
    RemoveMtc { slot: u32 },
    SetLtcLevel { slot: u32, text: String },
    SetMetronome { enabled: bool },
    SetMetronomeGain { text: String },
}

impl ShowOutputs {
    pub fn validate(&self) -> Result<()> {
        fn slots(values: impl Iterator<Item = u32>) -> Result<()> {
            let mut seen = std::collections::HashSet::new();
            for slot in values {
                if slot == 0 || !seen.insert(slot) {
                    return Err(ProjectError::Invalid(
                        "invalid or duplicate output slot".into(),
                    ));
                }
            }
            Ok(())
        }
        slots(self.ltc.iter().map(|s| s.slot))?;
        slots(self.mtc.iter().map(|s| s.slot))?;
        if self
            .ltc
            .iter()
            .any(|s| !s.level.is_none_or(|v| (0.0..=1.0).contains(&v)))
        {
            return Err(ProjectError::Invalid(
                "the LTC level must be between 0 and 1".into(),
            ));
        }
        if self
            .metronome
            .as_ref()
            .is_some_and(|s| !s.gain_db.is_finite())
        {
            return Err(ProjectError::Invalid(
                "the metronome level is not a number".into(),
            ));
        }
        Ok(())
    }

    pub fn edit(&mut self, op: SignalOp) -> Result<()> {
        fn free(slots: impl Iterator<Item = u32>) -> u32 {
            let used: std::collections::HashSet<_> = slots.collect();
            (1..).find(|n| !used.contains(n)).unwrap_or(1)
        }
        let number = |text: &str| {
            text.trim()
                .parse::<f32>()
                .map_err(|_| ProjectError::Invalid("invalid output level".into()))
        };
        match op {
            SignalOp::AddLtc => self.ltc.push(LtcSignal {
                slot: free(self.ltc.iter().map(|s| s.slot)),
                level: None,
            }),
            SignalOp::AddMtc => self.mtc.push(MtcSignal {
                slot: free(self.mtc.iter().map(|s| s.slot)),
            }),
            SignalOp::RemoveLtc { slot } => self.ltc.retain(|s| s.slot != slot),
            SignalOp::RemoveMtc { slot } => self.mtc.retain(|s| s.slot != slot),
            SignalOp::SetLtcLevel { slot, text } => {
                let signal = self
                    .ltc
                    .iter_mut()
                    .find(|s| s.slot == slot)
                    .ok_or_else(|| ProjectError::Invalid("unknown LTC output".into()))?;
                signal.level = Some(number(&text)?);
            }
            SignalOp::SetMetronome { enabled } => {
                if enabled {
                    self.metronome.get_or_insert(ClickSettings { gain_db: 0.0 });
                } else {
                    self.metronome = None;
                }
            }
            SignalOp::SetMetronomeGain { text } => {
                let click = self
                    .metronome
                    .as_mut()
                    .ok_or_else(|| ProjectError::Invalid("metronome is disabled".into()))?;
                click.gain_db = number(&text)?;
            }
        }
        self.validate()
    }
}

impl From<OutputConfig> for ShowOutputs {
    fn from(routes: OutputConfig) -> Self {
        Self {
            metronome: routes
                .metronome
                .as_ref()
                .map(|m| ClickSettings { gain_db: m.gain_db }),
            ltc: routes
                .ltc
                .iter()
                .enumerate()
                .map(|(i, s)| LtcSignal {
                    slot: i as u32 + 1,
                    level: s.level,
                })
                .collect(),
            mtc: routes
                .mtc
                .iter()
                .enumerate()
                .map(|(i, _)| MtcSignal { slot: i as u32 + 1 })
                .collect(),
            osc: routes.osc.clone(),
            legacy_routes: Some(routes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removing_a_signal_preserves_other_slots() {
        let mut outputs = ShowOutputs::default();
        outputs.edit(SignalOp::AddLtc).unwrap();
        outputs.edit(SignalOp::AddLtc).unwrap();
        outputs.edit(SignalOp::RemoveLtc { slot: 1 }).unwrap();
        assert_eq!(outputs.ltc[0].slot, 2);
        outputs.edit(SignalOp::AddLtc).unwrap();
        assert_eq!(outputs.ltc[1].slot, 1);
    }
    #[test]
    fn levels_and_duplicate_slots_are_validated() {
        let mut outputs = ShowOutputs::default();
        outputs.edit(SignalOp::AddLtc).unwrap();
        assert!(
            outputs
                .edit(SignalOp::SetLtcLevel {
                    slot: 1,
                    text: "NaN".into()
                })
                .is_err()
        );
        outputs.ltc = vec![
            LtcSignal {
                slot: 1,
                level: None
            };
            2
        ];
        assert!(outputs.validate().is_err());
    }
}
