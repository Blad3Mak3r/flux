use super::*;

/// One OSC message.
#[derive(Debug, Clone, PartialEq)]
pub struct OscMessage {
    pub address: String,
    pub args: Vec<OscArg>,
}

/// Bundles inside bundles stop being followed at this depth.
pub(super) const MAX_BUNDLE_DEPTH: usize = 4;

/// The messages in an OSC packet: one message, or the messages of a bundle (their time tags are
/// ignored, a control surface wants them now). A malformed packet gives none.
pub fn decode_osc(packet: &[u8]) -> Vec<OscMessage> {
    let mut messages = Vec::new();
    collect(packet, 0, &mut messages);
    messages
}

pub(super) fn collect(packet: &[u8], depth: usize, out: &mut Vec<OscMessage>) {
    if let Some(elements) = packet.strip_prefix(b"#bundle\0") {
        if depth >= MAX_BUNDLE_DEPTH {
            return;
        }
        // 8 bytes of time tag, then elements each led by its size.
        let mut rest = elements.get(8..).unwrap_or_default();
        while let Some((size, tail)) = rest.split_first_chunk::<4>() {
            let size = i32::from_be_bytes(*size);
            let Ok(size) = usize::try_from(size) else {
                return;
            };
            let Some((element, tail)) = tail.split_at_checked(size) else {
                return;
            };
            collect(element, depth + 1, out);
            rest = tail;
        }
    } else if let Some(message) = decode_message(packet) {
        out.push(message);
    }
}

/// An OSC string at the start of `bytes` and what follows its padding.
pub(super) fn take_string(bytes: &[u8]) -> Option<(&str, &[u8])> {
    let end = bytes.iter().position(|&byte| byte == 0)?;
    let text = std::str::from_utf8(&bytes[..end]).ok()?;
    let rest = bytes.get((end + 1).next_multiple_of(4)..)?;
    Some((text, rest))
}

pub(super) fn decode_message(packet: &[u8]) -> Option<OscMessage> {
    let (address, rest) = take_string(packet)?;
    if !address.starts_with('/') {
        return None;
    }
    // OSC 1.0 senders may leave the type tags out, which means no arguments.
    let Some((tags, mut data)) = take_string(rest).filter(|(tags, _)| tags.starts_with(',')) else {
        return Some(OscMessage {
            address: address.to_owned(),
            args: Vec::new(),
        });
    };
    let mut args = Vec::new();
    for tag in tags[1..].chars() {
        match tag {
            'i' => {
                let (value, tail) = data.split_first_chunk::<4>()?;
                args.push(OscArg::Int(i32::from_be_bytes(*value)));
                data = tail;
            }
            'f' => {
                let (value, tail) = data.split_first_chunk::<4>()?;
                args.push(OscArg::Float(f32::from_be_bytes(*value)));
                data = tail;
            }
            's' => {
                let (text, tail) = take_string(data)?;
                args.push(OscArg::String(text.to_owned()));
                data = tail;
            }
            'T' => args.push(OscArg::Bool(true)),
            'F' => args.push(OscArg::Bool(false)),
            // Nil and impulse carry no data and mean nothing here.
            'N' | 'I' => {}
            // A type whose size is not known cannot be skipped.
            _ => return None,
        }
    }
    Some(OscMessage {
        address: address.to_owned(),
        args,
    })
}

/// The first argument as a whole number: an integer, a float (rounded) or a string with a
/// number, because the plugins of control surfaces send any of them.
pub(super) fn number(args: &[OscArg]) -> Option<i32> {
    let value = match args.first()? {
        OscArg::Int(value) => f64::from(*value),
        OscArg::Float(value) => f64::from(*value),
        OscArg::String(text) => text.trim().parse().ok()?,
        OscArg::Bool(_) => return None,
    };
    // Rounding to an integer is the point, and out of range values are refused first.
    (value.is_finite() && value.abs() < f64::from(i32::MAX)).then(|| value.round() as i32)
}

/// The action an OSC message asks for. The addresses are fixed:
///
/// | Address | Argument | Action |
/// | --- | --- | --- |
/// | `/flux/play`, `/flux/pause`, `/flux/toggle`, `/flux/stop` | none | transport |
/// | `/flux/cue/next`, `/flux/cue/prev` | none | cue before or after the position |
/// | `/flux/cue/goto` | number, from 1 | the cue with that number |
/// | `/flux/seek/frames`, `/flux/seek/seconds` | number, negative goes back | move the position |
/// | `/flux/timeline/next`, `/flux/timeline/prev` | none | neighbour timeline |
/// | `/flux/timeline/select` | number, from 1 | the timeline in that place |
/// | `/flux/view/edit`, `/flux/view/live` | none | switch view |
///
/// A button of a control surface sends an argument (usually `1`) with the actions that need
/// none; it is ignored.
pub fn osc_action(message: &OscMessage) -> Option<ControlAction> {
    let args = &message.args;
    let position = |make: fn(u32) -> ControlAction| {
        number(args)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 1)
            .map(make)
    };
    match message.address.strip_prefix("/flux/")? {
        "go" => Some(ControlAction::Go),
        "devamp" => Some(ControlAction::Devamp),
        "devamp/cancel" => Some(ControlAction::CancelDevamp),
        "play" => Some(ControlAction::Play),
        "pause" => Some(ControlAction::Pause),
        "toggle" => Some(ControlAction::Toggle),
        "stop" => Some(ControlAction::Stop),
        "cue/next" => Some(ControlAction::NextCue),
        "cue/prev" => Some(ControlAction::PrevCue),
        "cue/goto" => position(ControlAction::GotoCue),
        "seek/frames" => number(args).map(ControlAction::StepFrames),
        "seek/seconds" => number(args).map(ControlAction::StepSeconds),
        "timeline/next" => Some(ControlAction::NextTimeline),
        "timeline/prev" => Some(ControlAction::PrevTimeline),
        "timeline/select" => position(ControlAction::SelectTimeline),
        "view/edit" => Some(ControlAction::ViewEdit),
        "view/live" => Some(ControlAction::ViewLive),
        _ => None,
    }
}
