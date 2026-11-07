//! Prepared audio lanes. Membership and channel validation happen off the audio thread.
use crate::mixer::{ClipSet, MIX_CHUNK};
use flux_project::{OutputConfig, routing::ResolvedAudioBus};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRouteState {
    pub id: String,
    pub device: String,
    pub channels: Vec<u16>,
    pub status: AudioRouteStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioRouteStatus {
    Ready,
    Missing,
    Conflict,
    Lost,
    Unused,
}

#[derive(Debug)]
pub(crate) struct RouteHealth {
    pub state: AudioRouteState,
    pub lost: Option<Arc<AtomicBool>>,
}
impl RouteHealth {
    pub fn state(&self) -> AudioRouteState {
        let mut state = self.state.clone();
        if state.status == AudioRouteStatus::Ready
            && self
                .lost
                .as_ref()
                .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            state.status = AudioRouteStatus::Lost;
        }
        state
    }
}

#[derive(Debug)]
pub(crate) struct AudioLane {
    pub channels: Vec<usize>,
    pub tracks: Vec<String>,
    pub left: [f32; MIX_CHUNK],
    pub right: [f32; MIX_CHUNK],
}

impl AudioLane {
    pub fn mix(&mut self, clips: &ClipSet, at: u64, frames: usize) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        clips.mix_tracks(
            at,
            &mut self.left[..frames],
            &mut self.right[..frames],
            &self.tracks,
        );
    }
    pub fn sample(&self, channel: usize, at: usize) -> f32 {
        match self.channels.as_slice() {
            [only] if *only == channel => (self.left[at] + self.right[at]) * 0.5,
            [left, _] if *left == channel => self.left[at],
            [_, right] if *right == channel => self.right[at],
            _ => 0.0,
        }
    }
}

pub(crate) fn used(config: &OutputConfig, bus: &ResolvedAudioBus) -> bool {
    config
        .track_routes
        .iter()
        .any(|t| t.output.as_deref() == Some(&bus.id))
}

pub(crate) fn conflict(
    config: &OutputConfig,
    bus: &ResolvedAudioBus,
    same: impl Fn(&str, &str) -> bool,
) -> bool {
    bus.route.as_ref().is_some_and(|r| {
        config
            .ltc
            .iter()
            .any(|ltc| same(&r.device_name, &ltc.device_name) && r.channels.contains(&ltc.channel))
    })
}

pub(crate) fn lanes(
    config: &OutputConfig,
    device: &str,
    opened: u16,
    same: impl Fn(&str, &str) -> bool,
) -> Option<Vec<AudioLane>> {
    if config.audio_routes.is_empty() {
        return None;
    }
    Some(
        config
            .audio_routes
            .iter()
            .filter(|b| used(config, b) && !conflict(config, b, &same))
            .filter_map(|bus| {
                let route = bus.route.as_ref()?;
                if !same(device, &route.device_name)
                    || route.channels.iter().any(|c| *c == 0 || *c > opened)
                {
                    return None;
                }
                Some(AudioLane {
                    channels: route.channels.iter().map(|c| usize::from(*c) - 1).collect(),
                    tracks: config
                        .track_routes
                        .iter()
                        .filter(|t| t.output.as_deref() == Some(&bus.id))
                        .map(|t| t.track_id.clone())
                        .collect(),
                    left: [0.0; MIX_CHUNK],
                    right: [0.0; MIX_CHUNK],
                })
            })
            .collect(),
    )
}
