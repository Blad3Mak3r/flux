pub const DMX_CHANNELS: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmxFrame {
    slots: [u8; DMX_CHANNELS],
}

impl DmxFrame {
    pub fn from_channels(channels: &[u8]) -> Self {
        let mut frame = Self::default();
        frame.update(channels);
        frame
    }

    pub fn update(&mut self, channels: &[u8]) {
        debug_assert!(channels.len() <= DMX_CHANNELS);
        self.slots.fill(0);
        self.slots[..channels.len()].copy_from_slice(channels);
    }

    pub fn slots(&self) -> &[u8; DMX_CHANNELS] {
        &self.slots
    }
}

impl Default for DmxFrame {
    fn default() -> Self {
        Self {
            slots: [0; DMX_CHANNELS],
        }
    }
}

pub fn validate_channel_count(channels: u16) -> Result<usize, &'static str> {
    let channels = usize::from(channels);
    if !(1..=DMX_CHANNELS).contains(&channels) {
        return Err("channels must be between 1 and 512");
    }
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_channels_into_a_frame() {
        let frame = DmxFrame::from_channels(&[1, 2, 3]);
        assert_eq!(&frame.slots()[..3], &[1, 2, 3]);
        assert!(frame.slots()[3..].iter().all(|&value| value == 0));
    }

    #[test]
    fn partial_updates_clear_previous_channels() {
        let mut frame = DmxFrame::from_channels(&[10, 20, 30]);
        frame.update(&[99]);

        assert_eq!(&frame.slots()[..3], &[99, 0, 0]);
    }

    #[test]
    fn validates_channel_limits() {
        assert_eq!(validate_channel_count(1), Ok(1));
        assert_eq!(validate_channel_count(512), Ok(512));
        assert!(validate_channel_count(0).is_err());
    }
}
